#!/usr/bin/env python3
"""Benchmark a local Ollama vision model against the same truth set as score.py."""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from ollama_ocr import run  # noqa: E402
from score import IMAGES, TRUTH, Result, cer, dedent, summarize  # noqa: E402


def strip_fence(text: str) -> str:
    text = text.strip()
    m = re.search(r"```[a-zA-Z0-9_+-]*\n(.*?)```", text, re.S)
    if m:
        return m.group(1).rstrip("\n")
    if text.startswith("```"):
        text = text.split("\n", 1)[1] if "\n" in text else ""
        text = text.rsplit("```", 1)[0]
    return text.rstrip("\n")


def main() -> int:
    model = sys.argv[1] if len(sys.argv) > 1 else "qwen2.5vl:3b"
    variants = [v for v in sys.argv[2:]] or ["2x.png"]
    results: list[Result] = []
    for scene in sorted(p.stem for p in TRUTH.glob("*.txt")):
        for variant in variants:
            img = IMAGES / f"{scene}@{variant}"
            if not img.exists():
                print(f"missing {img}", file=sys.stderr)
                continue
            truth = (TRUTH / f"{scene}.txt").read_text().rstrip("\n")
            out = run(model, img)
            hyp = strip_fence(out["text"])
            ref_lines = truth.splitlines()
            hyp_lines = hyp.splitlines()
            exact = sum(1 for a, b in zip(ref_lines, hyp_lines) if a == b)
            r = Result(
                scene=scene,
                image=img.name,
                engine=model,
                cer=round(cer(truth, hyp), 4),
                cer_dedent=round(cer(dedent(truth), dedent(hyp)), 4),
                line_exact=exact,
                line_total=len(ref_lines),
                lines_out=len(hyp_lines),
                ms=out["ms"],
                text=hyp,
            )
            results.append(r)
            print(
                f"{scene:18s} {variant:8s} CER={r.cer:.4f} dedent={r.cer_dedent:.4f} "
                f"lines={r.lines_out}/{r.line_total} exact={r.line_exact}/{r.line_total} "
                f"{r.ms}ms",
                flush=True,
            )
    summarize(results)
    out_path = Path(__file__).resolve().parent / f"results-{model.replace(':', '-')}.json"
    out_path.write_text(
        json.dumps([r.__dict__ | {"text": r.text} for r in results], ensure_ascii=False, indent=1)
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
