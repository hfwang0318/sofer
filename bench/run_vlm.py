#!/usr/bin/env python3
"""Run an MLX VLM (via mlx-vlm) over the benchmark images in one process.

usage: run_vlm.py --model ~/models/X --prompt "OCR:" --tag paddle [--glob 'images/*@2x.png']
Writes preds/<tag>.<scene>.json with {"engine","lines":[{"text","bbox":[]}],"ms","peak_mb"}.
VLMs do not return geometric boxes, so bbox is empty and layout recovery is skipped.
"""
import argparse
import glob
import json
import os
import time

import mlx.core as mx
from mlx_vlm import generate, load
from mlx_vlm.prompt_utils import apply_chat_template
from mlx_vlm.utils import load_config


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", required=True)
    ap.add_argument("--tag", required=True)
    ap.add_argument("--prompt", default="OCR:")
    ap.add_argument("--glob", default="images/*@2x.png")
    ap.add_argument("--max-tokens", type=int, default=3072)
    ap.add_argument("--outdir", default="preds")
    args = ap.parse_args()

    t0 = time.time()
    model, processor = load(args.model)
    config = load_config(args.model)
    load_s = time.time() - t0
    print(f"loaded {args.tag} in {load_s:.1f}s", flush=True)

    os.makedirs(args.outdir, exist_ok=True)
    for img in sorted(glob.glob(args.glob)):
        scene = os.path.basename(img).split("@")[0]
        prompt = apply_chat_template(processor, config, args.prompt, num_images=1)
        mx.reset_peak_memory()
        t = time.time()
        res = generate(model, processor, prompt, image=img,
                       max_tokens=args.max_tokens, verbose=False)
        ms = (time.time() - t) * 1000
        text = res.text if hasattr(res, "text") else str(res)
        peak = mx.get_peak_memory() / 1e6
        out = {"engine": args.tag, "prompt": args.prompt, "ms": ms, "peak_mb": round(peak),
               "lines": [{"text": ln, "bbox": []} for ln in text.split("\n")], "text": text}
        with open(f"{args.outdir}/{args.tag}.{scene}.json", "w") as f:
            json.dump(out, f, ensure_ascii=False)
        print(f"  {scene:18s} {ms:7.0f}ms {peak:6.0f}MB  {len(text):5d} chars", flush=True)


if __name__ == "__main__":
    main()
