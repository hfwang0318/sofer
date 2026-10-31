#!/usr/bin/env python3
"""Aggregate cer_eval.py output over preds/ into a markdown table."""
import glob
import json
import os
import re
import subprocess
import sys

SCENES = ["git-diff", "pytest-traceback", "tree", "ascii-table",
          "editor-rust", "light-ambiguous", "softwrap"]


def score(engine, scene, variant):
    p = f"preds/{engine}.{scene}.{variant}.json"
    if not os.path.exists(p):
        return None
    out = subprocess.run([sys.executable, "cer_eval.py", f"truth/{scene}.txt", p],
                         capture_output=True, text=True).stdout
    return json.loads(out)


def main():
    variant = sys.argv[1] if len(sys.argv) > 1 else "2xpng"
    engines = sorted({os.path.basename(f).split(".")[0] for f in glob.glob("preds/*.json")})
    rows = []
    for e in engines:
        per = [score(e, s, variant) for s in SCENES]
        per = [x for x in per if x]
        if not per:
            continue
        rows.append({
            "engine": e,
            "n": len(per),
            "content_acc": round(sum(x["content_acc"] for x in per) / len(per), 4),
            "sym_recall": round(sum(x["sym_recall"] for x in per) / len(per), 4),
            "indent_cov": round(sum(x["indent_cov"] for x in per) / len(per), 2),
            "indent_exact": round(sum(x["indent_exact"] for x in per) / len(per), 3),
            "rows_ratio": round(sum(x["rows_ratio"] for x in per) / len(per), 2),
            "ms": round(sum(x["ms"] for x in per) / len(per)),
            "peak_mb": max((x["peak_mb"] or 0) for x in per) or None,
        })
    rows.sort(key=lambda r: -r["content_acc"])
    hdr = "| engine | content_acc | sym_recall | indent_exact | indent_cov | rows_ratio | ms/img | peak MB |"
    print(f"\n### variant {variant}\n")
    print(hdr)
    print("|---|---|---|---|---|---|---|")
    for r in rows:
        print(f"| {r['engine']} | {r['content_acc']:.3f} | {r['sym_recall']:.3f} | {r['indent_exact']:.2f} | "
              f"{r['indent_cov']:.2f} | {r['rows_ratio']:.2f} | {r['ms']} | {r['peak_mb'] or '-'} |")

    # per-scene matrix for content_acc
    print("\n### per-scene content_acc\n")
    print("| engine | " + " | ".join(SCENES) + " |")
    print("|---" * (len(SCENES) + 1) + "|")
    for r in rows:
        cells = []
        for s in SCENES:
            x = score(r["engine"], s, variant)
            cells.append(f"{x['content_acc']:.3f}" if x else "-")
        print(f"| {r['engine']} | " + " | ".join(cells) + " |")


if __name__ == "__main__":
    main()
