set -x
PY=/Users/hfwang/workspace/ocr4code/.venv/bin/python
M=$HOME/models
$PY run_vlm.py --model $M/PaddleOCR-VL-1.6-4bit --tag paddle16 --prompt "OCR:" --glob "images/*@2x.png" 2>/dev/null
$PY run_vlm.py --model $M/GLM-OCR-4bit --tag glmocr --prompt "OCR:" --glob "images/*@2x.png" 2>/dev/null
$PY run_vlm.py --model $M/Qwen3-VL-4B-Instruct-4bit --tag qwen3vl4b --prompt "Transcribe every character in this image exactly as shown, preserving indentation, spaces, punctuation, case and line breaks. Do not correct, explain or summarize. Output only the raw text." --glob "images/*@2x.png" 2>/dev/null
$PY run_vlm.py --model $M/DeepSeek-OCR-4bit --tag deepseekocr --prompt "<|grounding|>Convert the document to markdown." --glob "images/*@2x.png" 2>/dev/null
