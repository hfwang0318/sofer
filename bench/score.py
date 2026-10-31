#!/usr/bin/env python3
"""Score OCR output against bench ground truth: CER, line-exact, indent recovery.

usage: python3 score.py [--engines vision,...] [--json out.json]
Ground truth lives in truth/*.txt; images in images/<scene>@{1x,2x}.{png,jpg}.
"""
from __future__ import annotations

import json
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

BENCH = Path(__file__).resolve().parent
sys.path.insert(0, str(BENCH))
TRUTH = BENCH / "truth"
IMAGES = BENCH / "images"
VISION_BIN = Path("/tmp/ocr_vision")


def lev(a: str, b: str) -> int:
    if len(a) < len(b):
        a, b = b, a
    prev = list(range(len(b) + 1))
    for i, ca in enumerate(a, 1):
        cur = [i]
        for j, cb in enumerate(b, 1):
            cur.append(min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (ca != cb)))
        prev = cur
    return prev[-1]


def cer(ref: str, hyp: str) -> float:
    return lev(ref, hyp) / max(1, len(ref))


def dedent(text: str) -> str:
    return "\n".join(line.strip() for line in text.splitlines())


@dataclass(frozen=True)
class Result:
    scene: str
    image: str
    engine: str
    cer: float
    cer_dedent: float
    line_exact: int
    line_total: int
    lines_out: int
    ms: int | None
    text: str


def run_vision(img: Path, flags: list[str]) -> dict:
    proc = subprocess.run([str(VISION_BIN), str(img), *flags], capture_output=True, text=True)
    if proc.returncode != 0:
        raise RuntimeError(f"vision failed on {img.name}: {proc.stderr.strip()}")
    return json.loads(proc.stdout)


def vision_engine(flags: list[str], post: bool = False) -> Callable[[Path], dict]:
    def run(img: Path) -> dict:
        data = run_vision(img, flags)
        if post:
            from post import parse, rebuild

            text = rebuild(parse(data))
        else:
            text = "\n".join(line["text"] for line in data["lines"]).rstrip("\n")
        return {"text": text, "ms": data.get("ms")}

    return run


def score(scene: str, img: Path, engine_name: str, run: Callable[[Path], dict]) -> Result:
    truth = (TRUTH / f"{scene}.txt").read_text().rstrip("\n")
    out = run(img)
    hyp = out["text"]
    ref_lines = truth.splitlines()
    hyp_lines = hyp.splitlines()
    exact = sum(1 for a, b in zip(ref_lines, hyp_lines) if a == b)
    return Result(
        scene=scene,
        image=img.name,
        engine=engine_name,
        cer=round(cer(truth, hyp), 4),
        cer_dedent=round(cer(dedent(truth), dedent(hyp)), 4),
        line_exact=exact,
        line_total=len(ref_lines),
        lines_out=len(hyp_lines),
        ms=out.get("ms"),
        text=hyp,
    )


ENGINES = {
    "vision-accurate": vision_engine([]),
    "vision+corr": vision_engine(["--correction"]),
    "vision-fast": vision_engine(["--fast"]),
    "vision-doc": vision_engine(["--doc"]),
    "vision+post": vision_engine([], post=True),
    "vision+post+corr": vision_engine(["--correction"], post=True),
}


def summarize(results: list[Result]) -> None:
    groups: dict[tuple[str, str], list[Result]] = {}
    for r in results:
        variant = r.image.split("@")[1] if "@" in r.image else r.image
        groups.setdefault((r.engine, variant), []).append(r)
    print(f"{'engine':16s} {'variant':10s} {'meanCER':>8s} {'CERdedent':>10s} {'lineExact':>10s} {'ms':>6s}")
    for (engine, variant), group in sorted(groups.items()):
        n = len(group)
        exact = sum(r.line_exact for r in group)
        total = sum(r.line_total for r in group)
        ms = [r.ms for r in group if r.ms]
        ms_avg = sum(ms) / len(ms) if ms else 0
        print(
            f"{engine:16s} {variant:10s} "
            f"{sum(r.cer for r in group) / n:8.4f} "
            f"{sum(r.cer_dedent for r in group) / n:10.4f} "
            f"{exact:>4d}/{total:<5d} "
            f"{ms_avg:6.1f}"
        )


def main() -> int:
    if not VISION_BIN.exists():
        print(f"missing {VISION_BIN}; compile vision_ocr.swift first", file=sys.stderr)
        return 2
    wanted = [a.split("=", 1)[1] for a in sys.argv if a.startswith("--engines=")]
    names = wanted[0].split(",") if wanted else list(ENGINES)
    results: list[Result] = []
    for scene in sorted(p.stem for p in TRUTH.glob("*.txt")):
        for img in sorted(IMAGES.glob(f"{scene}@*")):
            for name in names:
                if name == "vision-doc" and img.suffix != ".png":
                    continue
                try:
                    results.append(score(scene, img, name, ENGINES[name]))
                except RuntimeError as exc:
                    print(f"skip {img.name}: {exc}", file=sys.stderr)
    summarize(results)
    for r in results:
        print(f"  {r.engine:16s} {r.scene:18s} {r.image:24s} CER={r.cer:.4f} lines={r.lines_out}/{r.line_total}")
    out_arg = [a for a in sys.argv if a.startswith("--json=")]
    if out_arg:
        payload = [r.__dict__ | {"text": r.text} for r in results]
        Path(out_arg[0].split("=", 1)[1]).write_text(json.dumps(payload, ensure_ascii=False, indent=1))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
