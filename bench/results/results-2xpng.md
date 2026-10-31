
### variant 2xpng

| engine | content_acc | sym_recall | indent_exact | indent_cov | rows_ratio | ms/img | peak MB |
|---|---|---|---|---|---|---|
| qwen3vl4b | 0.903 | 0.987 | 0.69 | 0.61 | 0.96 | 5368 | 4101 |
| llamacpp | 0.881 | 0.998 | 0.72 | 0.56 | 1.05 | 2691 | - |
| glmocr | 0.875 | 0.992 | 0.67 | 0.48 | 1.00 | 1763 | 2200 |
| tesseract | 0.861 | 0.906 | 0.86 | 0.60 | 0.96 | 511 | - |
| rapidocr | 0.838 | 0.858 | 0.86 | 0.52 | 0.92 | 614 | - |
| vision-accurate | 0.827 | 0.883 | 0.84 | 0.53 | 0.90 | 110 | - |
| vision-doc | 0.778 | 0.842 | 0.00 | 0.00 | 0.07 | 188 | - |
| paddle16 | 0.231 | 0.792 | 0.76 | 0.35 | 0.74 | 3247 | 1853 |

### per-scene content_acc

| engine | git-diff | pytest-traceback | tree | ascii-table | editor-rust | light-ambiguous | softwrap |
|---|---|---|---|---|---|---|---|
| qwen3vl4b | 0.988 | 0.848 | 1.000 | 0.578 | 0.934 | 0.970 | 1.000 |
| llamacpp | 0.996 | 0.911 | 0.767 | 0.581 | 0.934 | 0.985 | 0.997 |
| glmocr | 0.986 | 0.901 | 0.750 | 0.581 | 0.934 | 0.970 | 1.000 |
| tesseract | 0.978 | 0.784 | 0.767 | 0.578 | 0.994 | 0.931 | 0.992 |
| rapidocr | 0.971 | 0.708 | 0.761 | 0.578 | 0.989 | 0.866 | 0.994 |
| vision-accurate | 0.974 | 0.697 | 0.750 | 0.565 | 0.966 | 0.889 | 0.949 |
| vision-doc | 0.972 | 0.689 | 0.750 | 0.313 | 0.867 | 0.897 | 0.957 |
| paddle16 | -3.402 | 0.695 | 0.761 | 0.578 | 0.996 | 0.985 | 1.000 |

## 峰值内存（补充测量，2026-10-30 19:20）

上表的 peak MB 列由各 runner 自报：只有 MLX 系填了（`mx.get_peak_memory()`），
子进程型引擎（tesseract / vision / llama-server）没有插桩，故显示 `-`。补测如下：

| 引擎 | 口径 | 峰值内存 |
|---|---|---|
| Apple Vision (accurate) | 子进程 `ru_maxrss` | 47–69 MB |
| Tesseract (psm 6) | 子进程 `ru_maxrss` | 42–57 MB |
| RapidOCR (PP-OCRv6 ONNX) | Python 进程 RSS | 1.06 GB（含 onnxruntime arena，模型仅十几 MB） |
| GLM-OCR Q8 (llama.cpp server) | 服务进程 RSS 峰值（10 Hz 采样） | 1.8–2.4 GB |
| GLM-OCR 0.9B (MLX 4bit) | `mx.get_peak_memory()` | 2.2 GB |
| Qwen3-VL-4B (MLX 4bit) | `mx.get_peak_memory()` | 4.1 GB |

测量命令：
```bash
/usr/bin/time -l tesseract images/git-diff@2x.png stdout -l eng --psm 6 tsv   # -> maximum resident set size
python -c "import resource; ...; print(resource.getrusage(resource.RUSAGE_SELF).ru_maxrss/1048576)"
```
