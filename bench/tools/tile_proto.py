#!/usr/bin/env python3
"""Prototype of row-band tiling: cut only at blank gaps between text rows.

The rule, in one sentence: cut where the image has **no ink for a full row**,
never through a row, and give each tile one line of overlap so a row that touches
the boundary can still be recovered.
"""
import json
import subprocess
import sys
import tempfile

import numpy as np
from PIL import Image

CLI = "/Users/hfwang/workspace/ocr4code/target/release/ocr4code"


def bands_of(img):
    a = np.asarray(img.convert("L"), dtype=np.float32)
    bg = float(np.median(a))
    dev = np.abs(a - bg)
    thr = max(18.0, float(np.percentile(dev, 90)) * 0.35)
    ink = dev > thr
    rows = ink.sum(axis=1)
    min_ink = max(3, int(img.width * 0.002))
    active = rows >= min_ink
    out, s = [], None
    for y, v in enumerate(active):
        if v and s is None:
            s = y
        elif not v and s is not None:
            if y - s >= 3:
                out.append((s, y))
            s = None
    if s is not None and len(active) - s >= 3:
        out.append((s, len(active)))
    return out


def tile_ranges(bands, per_tile):
    """Group band indices into vertical tiles of ~per_tile rows."""
    ranges = []
    i = 0
    while i < len(bands):
        j = min(i + per_tile, len(bands))
        top = bands[i][0]
        bottom = bands[j - 1][1]
        # one row of overlap on each side (clipped later)
        lo = bands[max(0, i - 1)][0]
        hi = bands[min(len(bands) - 1, j)][1]
        ranges.append((i, j, max(0, lo - 2), min(bands[-1][1] + 4, hi + 2)))
        i = j
    return ranges


def run(img_path):
    r = subprocess.run([CLI, "ocr", img_path, "--engine", "llamacpp", "--format", "json"],
                       capture_output=True, text=True, timeout=600)
    if r.returncode != 0:
        return None
    return json.loads(r.stdout)


def main():
    src = sys.argv[1]
    truth = open(sys.argv[2]).read().rstrip("\n").split("\n")
    per_tile = int(sys.argv[3]) if len(sys.argv) > 3 else 14
    img = Image.open(src).convert("RGB")
    bands = bands_of(img)
    tiles = tile_ranges(bands, per_tile)
    print(f"{src}: {img.size}, {len(bands)} bands -> {len(tiles)} tiles of <= {per_tile} rows")

    out_lines = []
    for k, (i, j, top, bottom) in enumerate(tiles):
        crop = img.crop((0, top, img.width, bottom))
        path = f"/tmp/tile-{k}.png"
        crop.save(path)
        doc = run(path)
        if doc is None:
            print(f"  tile {k}: FAILED")
            continue
        lines = [l["text"].rstrip() for l in doc["lines"]]
        # Stitch by *band index*, not by text: the tile was cut with one band of
        # overlap on each side, so drop exactly that many lines. Text-based dedupe
        # is unsound when the model rewrites spacing or the overlap line differs.
        if i > 0 and lines:
            lines.pop(0)
        if j < len(bands) and lines:
            lines.pop()
        out_lines.extend(lines)
        print(f"  tile {k}: bands {i}..{j} {crop.size} -> {len(lines)} lines, {doc['ms']}ms")

    got = "\n".join(out_lines)
    open("/tmp/tiled.txt", "w").write(got + "\n")
    sys.path.insert(0, "/Users/hfwang/workspace/ocr4code/bench/tools")
    import cer_eval

    pred = {"lines": [{"text": l, "bbox": []} for l in out_lines], "ms": 0}
    json.dump(pred, open("/tmp/tiled.json", "w"))
    print("lines:", len(out_lines), "vs truth", len(truth))
    print(cer_eval.evaluate(sys.argv[2], "/tmp/tiled.json"))


if __name__ == "__main__":
    main()
