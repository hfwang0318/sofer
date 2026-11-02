#!/usr/bin/env python3
"""Paired before/after comparison for two baseline logs.

baseline.py prints one line per scene:

    llamacpp     tree               acc=1.000 sym=1.000 indent=1.00 rows=1.00 450ms

This script pairs the scenes, reports the per-scene delta, a bootstrap 95%
interval for the mean delta, and a two-sided sign test over the non-zero deltas.
A "stable" improvement is one where the interval stays above zero and no scene
regresses beyond the noise floor.

usage:
  python3 bench/compare.py /tmp/new-before.log /tmp/new-after.log
  python3 bench/compare.py before.log after.log --noise 0.002
"""
import argparse
import math
import random
import re
import sys

LINE = re.compile(r"llamacpp\s+(\S+)\s+acc=([\d.]+).*indent=([\d.]+).*rows=([\d.]+)")


def parse(path):
    out = {}
    with open(path, encoding="utf-8") as f:
        for line in f:
            m = LINE.search(line)
            if m:
                out[m.group(1)] = {
                    "acc": float(m.group(2)),
                    "indent": float(m.group(3)),
                    "rows": float(m.group(4)),
                }
    return out


def bootstrap_ci(deltas, draws=10000, seed=7):
    if not deltas:
        return 0.0, 0.0
    rng = random.Random(seed)
    means = []
    for _ in range(draws):
        means.append(sum(rng.choices(deltas, k=len(deltas))) / len(deltas))
    means.sort()
    return means[int(draws * 0.025)], means[int(draws * 0.975)]


def sign_test(deltas, noise):
    pos = sum(1 for d in deltas if d > noise)
    neg = sum(1 for d in deltas if d < -noise)
    n = pos + neg
    if n == 0:
        return pos, neg, 1.0
    k = min(pos, neg)
    tail = sum(math.comb(n, i) for i in range(k + 1)) / (2**n)
    return pos, neg, min(1.0, 2 * tail)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("before")
    ap.add_argument("after")
    ap.add_argument("--noise", type=float, default=0.001,
                    help="deltas within +/- noise count as ties (default 0.001)")
    ap.add_argument("--scenes", default=None,
                    help="comma-separated subset, e.g. for a Dev/Targeted split")
    args = ap.parse_args()

    before, after = parse(args.before), parse(args.after)
    scenes = sorted(set(before) & set(after))
    if args.scenes:
        want = {s.strip() for s in args.scenes.split(",") if s.strip()}
        scenes = [s for s in scenes if s in want]
    if not scenes:
        sys.exit("no overlapping scenes in the two logs")

    rows = []
    for s in scenes:
        d = after[s]["acc"] - before[s]["acc"]
        rows.append((d, s, before[s], after[s]))
    rows.sort(reverse=True)
    deltas = [r[0] for r in rows]

    mean_before = sum(before[s]["acc"] for s in scenes) / len(scenes)
    mean_after = sum(after[s]["acc"] for s in scenes) / len(scenes)
    lo, hi = bootstrap_ci(deltas)
    pos, neg, p = sign_test(deltas, args.noise)

    print(f"scenes: {len(scenes)}")
    print(f"mean content_acc: {mean_before:.4f} -> {mean_after:.4f} "
          f"(delta {mean_after - mean_before:+.4f})")
    print(f"bootstrap 95% CI of mean delta: [{lo:+.4f}, {hi:+.4f}]")
    print(f"sign test: {pos} improved, {neg} regressed, "
          f"{len(scenes) - pos - neg} within +/-{args.noise}: p={p:.4f}")
    print()
    print("| scene | before | after | delta |")
    print("|---|---:|---:|---:|")
    for d, s, b, a in rows:
        print(f"| {s} | {b['acc']:.4f} | {a['acc']:.4f} | {d:+.4f} |")


if __name__ == "__main__":
    main()
