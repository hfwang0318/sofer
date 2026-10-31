#!/usr/bin/env python3
"""Can we recover indentation WITHOUT an OCR engine's bboxes?

Pure-CV geometry: row projection -> text bands -> leftmost ink per band -> lattice.
Then pair the bands with whatever text lines an engine produced (here: GLM-OCR),
and measure indent_exact with cer_eval's text-aligned metric.
"""
import json
import sys

import numpy as np
from PIL import Image

sys.path.insert(0, ".")
import cer_eval


def bands(img_path):
    im = Image.open(img_path).convert("L")
    a = np.asarray(im, dtype=np.float32)
    # polarity-agnostic: ink = pixels far from the (uniform) background colour
    bg = float(np.median(a))
    dev = np.abs(a - bg)
    thr = max(18.0, float(np.percentile(dev, 90)) * 0.35)
    ink = dev > thr
    rows = ink.sum(axis=1)
    h = a.shape[0]
    active = rows > max(1, ink.shape[1] * 0.002)
    out, start = [], None
    for y in range(h):
        if active[y] and start is None:
            start = y
        elif not active[y] and start is not None:
            if y - start >= 8:
                out.append((start, y))
            start = None
    if start is not None:
        out.append((start, h))
    res = []
    for y0, y1 in out:
        sub = ink[y0:y1]
        cols = np.where(sub.any(axis=0))[0]
        res.append({"y0": int(y0), "y1": int(y1), "x0": int(cols[0]), "x1": int(cols[-1]) + 1})
    return res, a.shape


def indent_lines(band_list, texts):
    """Pair bands with text lines (1:1 when counts match, else align by order) and
    rebuild leading spaces from x0 with a lattice fit on the non-zero offsets."""
    left = min(b["x0"] for b in band_list)
    rel = [b["x0"] - left for b in band_list if b["x0"] - left > 0.5]
    # advance prior: ink width / chars for long lines
    ratios = [(b["x1"] - b["x0"]) / len(t) for b, t in zip(band_list, texts)
              if t.strip() and len(t) >= 10]
    adv = float(np.median(ratios)) if ratios else 8.0
    offsets = sorted(set(rel))
    best, cost = adv, 1e18
    step = adv * 0.85
    while step <= adv * 1.15:
        c = sum((x / step - round(x / step)) ** 2 for x in offsets) / max(1, len(offsets))
        if c < cost - 1e-12:
            best, cost = step, c
        step += 0.02
    adv = best
    out = []
    for b, t in zip(band_list, texts):
        col = max(0, int(round((b["x0"] - left) / adv)))
        out.append(" " * col + t.strip())
    return out


def evaluate(scene, pred_path, img_path):
    d = json.load(open(pred_path))
    texts = [l["text"] for l in d["lines"]]
    while texts and not texts[-1].strip():
        texts.pop()
    bl, shape = bands(img_path)
    # drop bands that are likely blank separators vs text lines
    print(f"  {scene}: bands={len(bl)} textlines={len(texts)}")
    if len(bl) != len(texts):
        return None
    nonempty = [t for t in texts if t.strip()]
    if len(bl) != len(nonempty):
        return None
    merged = indent_lines(bl, nonempty)
    truth = [l.rstrip() for l in open(f"truth/{scene}.txt").read().split("\n")]
    while truth and not truth[-1]:
        truth.pop()
    # positional comparison: the i-th visual line should get the i-th indent.
    # (text-key matching collapses the moment a model drops the line-number gutter)
    tlines = [x for x in truth if x.strip()]
    n = min(len(merged), len(tlines))
    hit = sum(1 for a, b in zip(tlines[:n], merged[:n])
              if cer_eval.indent_of(a) == cer_eval.indent_of(b))
    return {"indent_exact": round(hit / max(1, n), 3),
            "n": n, "bands": len(bl), "textlines": len(nonempty)}


if __name__ == "__main__":
    pred = sys.argv[1] if len(sys.argv) > 1 else "preds/glmocr.{}.2xpng.json"
    for s in ["git-diff", "pytest-traceback", "tree", "ascii-table",
              "editor-rust", "light-ambiguous", "softwrap"]:
        p = pred.format(s)
        try:
            r = evaluate(s, p, f"images/{s}@2x.png")
            print(f"    -> {r}")
        except Exception as e:
            print(f"  {s}: ERR {e}")
