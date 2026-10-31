#!/usr/bin/env python3
"""Tesseract and RapidOCR runners -> same JSON schema as vision_ocr / run_vlm."""
import glob
import json
import os
import subprocess
import sys
import time

import numpy as np

OUT = "preds"


def lines_from_tsv(tsv: str):
    """Group tesseract TSV words into (block,par,line) rows with a pixel bbox."""
    rows = {}
    for ln in tsv.splitlines()[1:]:
        f = ln.split("\t")
        if len(f) < 12 or f[0] != "5":
            continue
        text = f[11].strip()
        if not text:
            continue
        key = (f[2], f[3], f[4])
        x, y, w, h = (int(v) for v in f[6:10])
        r = rows.setdefault(key, {"words": [], "x0": x, "y0": y, "x1": x + w, "y1": y + h})
        r["words"].append((x, text))
        r["x0"] = min(r["x0"], x)
        r["x1"] = max(r["x1"], x + w)
        r["y0"] = min(r["y0"], y)
        r["y1"] = max(r["y1"], y + h)
    out = []
    for key in sorted(rows, key=lambda k: (int(k[0]), int(k[1]), int(k[2]))):
        r = rows[key]
        joined = " ".join(w for _, w in sorted(r["words"]))
        out.append({"text": joined, "bbox": [r["x0"], r["y0"], r["x1"], r["y1"]]})
    return out


def run_tesseract(globpat):
    for img in sorted(glob.glob(globpat)):
        scene = os.path.basename(img).split("@")[0]
        t = time.time()
        tsv = subprocess.run(["tesseract", img, "stdout", "-l", "eng", "--psm", "6", "tsv"],
                             capture_output=True, text=True).stdout
        ms = (time.time() - t) * 1000
        lines = lines_from_tsv(tsv)
        with open(f"{OUT}/tesseract.{scene}.json", "w") as f:
            json.dump({"engine": "tesseract", "ms": ms, "lines": lines,
                       "text": "\n".join(l["text"] for l in lines)}, f, ensure_ascii=False)
        print(f"  {scene:18s} {ms:7.0f}ms", flush=True)


def run_rapidocr(globpat):
    from rapidocr import RapidOCR
    engine = RapidOCR()
    for img in sorted(glob.glob(globpat)):
        scene = os.path.basename(img).split("@")[0]
        t = time.time()
        res = engine(img)
        ms = (time.time() - t) * 1000
        lines = []
        if res is not None and getattr(res, "boxes", None) is not None:
            for box, txt in zip(res.boxes, res.txts):
                a = np.array(box)
                lines.append({"text": txt,
                              "bbox": [float(a[:, 0].min()), float(a[:, 1].min()),
                                       float(a[:, 0].max()), float(a[:, 1].max())]})
        with open(f"{OUT}/rapidocr.{scene}.json", "w") as f:
            json.dump({"engine": "rapidocr", "ms": ms, "lines": lines,
                       "text": "\n".join(l["text"] for l in lines)}, f, ensure_ascii=False)
        print(f"  {scene:18s} {ms:7.0f}ms", flush=True)


if __name__ == "__main__":
    os.makedirs(OUT, exist_ok=True)
    which, pat = sys.argv[1], (sys.argv[2] if len(sys.argv) > 2 else "images/*@2x.png")
    {"tesseract": run_tesseract, "rapidocr": run_rapidocr}[which](pat)
