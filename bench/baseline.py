#!/usr/bin/env python3
"""Baseline harness: run the `ocr4code` CLI over the bench corpus and score it.

This replaces the ad-hoc per-run scripts: every number in the report should come
from here, so that engine and layout changes are comparable across commits.

usage:
  python3 bench/baseline.py                    # all engines that are available
  python3 bench/baseline.py --engines mock,llamacpp
  python3 bench/baseline.py --variant 2xpng --md bench/results/baseline.md
"""
import argparse
import glob
import json
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
CLI = os.path.join(ROOT, "target", "release", "ocr4code")
CLI_DEBUG = os.path.join(ROOT, "target", "debug", "ocr4code")
sys.path.insert(0, os.path.join(HERE, "tools"))
import cer_eval  # noqa: E402


def cli_bin():
    if os.path.exists(CLI):
        return CLI
    if os.path.exists(CLI_DEBUG):
        return CLI_DEBUG
    raise SystemExit("build first: cargo build --release")


def scenes(only=None):
    all_scenes = sorted(
        os.path.basename(p).split("@")[0] for p in glob.glob(os.path.join(HERE, "images", "*@2x.png"))
    )
    if only:
        want = {x.strip() for x in only.split(",") if x.strip()}
        return [s for s in all_scenes if s in want]
    return all_scenes


def run(engine, scene, variant, prompt=None, timeout=1200):
    peak_mb = None
    # variant -> file suffix; "2xjpg" is the JPEG re-encode of the same 2x frame
    SUFFIX = {"2x": "@2x.png", "1x": "@1x.png", "2xjpg": "@2x.jpg", "photo": "@photo.jpg"}
    suffix = SUFFIX.get(variant, f"@{variant}.png")
    img = os.path.join(HERE, "images", f"{scene}{suffix}")
    if not os.path.exists(img):
        return None, 0, "missing-variant"
    cmd = [cli_bin(), "ocr", img, "--engine", engine, "--format", "json"]
    if prompt:
        cmd += ["--prompt", prompt]
    if engine == "mock":
        # the mock engine replays the whole truth text regardless of the image, so
        # tiling would just duplicate it; mock measures the geometry layer only
        cmd += ["--tile", "off", "--mock-text-file", os.path.join(HERE, "truth", f"{scene}.txt")]
    t0 = time.time()
    p = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
    wall = (time.time() - t0) * 1000
    # peak RSS of the CLI process itself (the model lives in llama-server, see report)
    try:
        tcmd = ["/usr/bin/time", "-l"] + cmd
        tp = subprocess.run(tcmd, capture_output=True, text=True, timeout=timeout)
        for line in tp.stderr.splitlines():
            if "maximum resident set size" in line:
                peak_mb = int(line.split()[0]) / 1048576
                break
    except Exception:
        peak_mb = None
    if p.returncode != 0:
        return None, wall, p.stderr.strip()
    try:
        doc = json.loads(p.stdout)
    except json.JSONDecodeError:
        return None, wall, p.stdout[:200]
    doc["_peak_mb"] = peak_mb
    return doc, wall, None


def score(scene, doc, wall):
    pred = {
        "engine": doc.get("engine", "?"),
        "ms": doc.get("ms", wall),
        "lines": [
            {"text": (" " * l["indent"]) + l["text"], "bbox": []}
            for l in doc["lines"]
        ],
    }
    path = os.path.join("/tmp", f"baseline-{scene}.json")
    with open(path, "w") as f:
        json.dump(pred, f)
    r = cer_eval.evaluate(os.path.join(HERE, "truth", f"{scene}.txt"), path)
    r["wall_ms"] = round(wall)
    r["peak_mb"] = doc.get("_peak_mb")
    return r


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--engines", default="mock,llamacpp,tesseract")
    ap.add_argument("--variant", default="2x", help="2x | 1x | 2xjpg | photo")
    ap.add_argument("--md", default=None)
    ap.add_argument("--prompt", default=None, help="prompt for the llamacpp engine")
    ap.add_argument("--scenes", default=None, help="comma separated subset of scenes")
    args = ap.parse_args()

    engines = [e.strip() for e in args.engines.split(",") if e.strip()]
    rows = []
    matrix = {}
    for eng in engines:
        per = []
        for s in scenes(args.scenes):
            doc, wall, err = run(eng, s, args.variant, args.prompt)
            if doc is None:
                if err != "missing-variant":
                    print(f"  {eng:12s} {s:18s} FAILED {err[:80] if err else ''}")
                continue
            r = score(s, doc, wall)
            per.append(r)
            matrix.setdefault(eng, {})[s] = r["content_acc"]
            print(
                f"  {eng:12s} {s:18s} acc={r['content_acc']:.3f} sym={r['sym_recall']:.3f} "
                f"indent={r['indent_exact']:.2f} rows={r['rows_ratio']:.2f} {r['wall_ms']}ms"
            )
        if not per:
            continue
        n = len(per)
        rows.append(
            {
                "engine": eng,
                "n": n,
                "content_acc": sum(x["content_acc"] for x in per) / n,
                "text_acc": sum(x["text_acc"] for x in per) / n,
                "box_recall": sum(x["box_recall"] for x in per) / n,
                "sym_recall": sum(x["sym_recall"] for x in per) / n,
                "indent_exact": sum(x["indent_exact"] for x in per) / n,
                "rows_ratio": sum(x["rows_ratio"] for x in per) / n,
                "ms": sum(x["wall_ms"] for x in per) / n,
                "peak_mb": max((x["peak_mb"] or 0) for x in per),
            }
        )

    # per-scene matrix for the worst-first reading
    md = []
    names = sorted({n for eng in matrix.values() for n in eng})
    n_scenes_with_data = len(names)
    if names:
        md += ["### per-scene content_acc", "",
               "| engine | " + " | ".join(names) + " |",
               "|---" * (len(names) + 1) + "|"]
        for eng in sorted(matrix):
            md.append("| " + eng + " | " + " | ".join(
                f"{matrix[eng][n]:.3f}" if n in matrix[eng] else "-" for n in names) + " |")
        md.append("")

    md += [
        f"### baseline ({args.variant}, {n_scenes_with_data} scenes)",
        "",
        "| engine | content_acc | text_acc | box_recall | sym_recall | indent_exact | rows_ratio | ms/img | peak MB |",
        "|---|---|---|---|---|---|---|---|---|",
    ]
    for r in sorted(rows, key=lambda r: -r["content_acc"]):
        md.append(
            f"| {r['engine']} | {r['content_acc']:.3f} | {r['text_acc']:.3f} | {r['box_recall']:.2f} | {r['sym_recall']:.3f} | "
            f"{r['indent_exact']:.2f} | {r['rows_ratio']:.2f} | {r['ms']:.0f} | {r['peak_mb']:.0f} |"
        )
    out = "\n".join(md)
    print()
    print(out)
    if args.md:
        os.makedirs(os.path.dirname(args.md), exist_ok=True)
        with open(args.md, "w") as f:
            f.write(out + "\n")
        print(f"\nwrote {args.md}")


if __name__ == "__main__":
    main()
