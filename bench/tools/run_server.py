#!/usr/bin/env python3
"""Run the llama.cpp server (GGUF, portable path) over the bench scenes.

usage: run_server.py [--host 127.0.0.1:8099] [--prompt "OCR:"] [--tag llamacpp] [--glob ...]
"""
import argparse
import base64
import glob
import json
import os
import time
import urllib.request

PROMPT = "OCR:"


def call(host, image_path, prompt, max_tokens=2048):
    with open(image_path, "rb") as f:
        b64 = base64.b64encode(f.read()).decode()
    payload = {
        "messages": [{"role": "user", "content": [
            {"type": "image_url", "image_url": {"url": f"data:image/png;base64,{b64}"}},
            {"type": "text", "text": prompt},
        ]}],
        "temperature": 0.0,
        "max_tokens": max_tokens,
        "stream": False,
    }
    req = urllib.request.Request(
        f"http://{host}/v1/chat/completions",
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json"},
    )
    t = time.time()
    with urllib.request.urlopen(req, timeout=600) as r:
        d = json.loads(r.read())
    ms = (time.time() - t) * 1000
    text = d["choices"][0]["message"]["content"]
    usage = d.get("usage", {})
    return text, ms, usage


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--host", default="127.0.0.1:8099")
    ap.add_argument("--prompt", default=PROMPT)
    ap.add_argument("--tag", default="llamacpp")
    ap.add_argument("--glob", default="images/*@2x.png")
    ap.add_argument("--outdir", default="preds")
    a = ap.parse_args()
    os.makedirs(a.outdir, exist_ok=True)
    for img in sorted(glob.glob(a.glob)):
        scene = os.path.basename(img).split("@")[0]
        text, ms, usage = call(a.host, img, a.prompt)
        out = {"engine": a.tag, "prompt": a.prompt, "ms": ms, "usage": usage,
               "lines": [{"text": ln, "bbox": []} for ln in text.split("\n")], "text": text}
        with open(f"{a.outdir}/{a.tag}.{scene}.2xpng.json", "w") as f:
            json.dump(out, f, ensure_ascii=False)
        print(f"  {scene:18s} {ms:7.0f}ms  {usage.get('completion_tokens', 0):4d} tok  {len(text):5d} chars", flush=True)


if __name__ == "__main__":
    main()
