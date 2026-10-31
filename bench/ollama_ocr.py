#!/usr/bin/env python3
"""Run a local Ollama vision model over an image and print raw text + timing."""
from __future__ import annotations

import base64
import json
import sys
import time
import urllib.request
from pathlib import Path

HOST = "http://127.0.0.1:11434"
PROMPT = (
    "Transcribe every character in this image exactly as shown. Preserve indentation, "
    "spaces, punctuation, case, and line breaks. Do not fix, explain, or summarize. "
    "Return only the transcription inside a single fenced code block."
)


def run(model: str, image: Path, prompt: str = PROMPT, think: bool = False) -> dict:
    body = {
        "model": model,
        "prompt": prompt,
        "images": [base64.b64encode(image.read_bytes()).decode()],
        "stream": False,
        "think": think,
        "options": {"temperature": 0, "num_predict": 1500, "num_ctx": 4096},
    }
    req = urllib.request.Request(
        f"{HOST}/api/generate",
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json"},
    )
    t0 = time.perf_counter()
    with urllib.request.urlopen(req, timeout=600) as resp:
        data = json.loads(resp.read())
    return {
        "text": data.get("response", ""),
        "ms": int((time.perf_counter() - t0) * 1000),
        "eval_count": data.get("eval_count"),
        "eval_duration_ns": data.get("eval_duration"),
    }


def main() -> int:
    model, path = sys.argv[1], Path(sys.argv[2])
    out = run(model, path)
    print(json.dumps(out, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
