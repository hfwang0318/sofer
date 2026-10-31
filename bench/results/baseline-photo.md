### per-scene content_acc

| engine | cargo-build | git-diff | git-log-graph | pytest-traceback |
|---|---|---|---|---|
| llamacpp | 0.972 | 0.997 | 0.996 | 0.819 |
| mock | 1.000 | 1.000 | 1.000 | 1.000 |
| tesseract | 0.920 | 0.977 | 0.976 | 0.863 |

### baseline (photo, 4 scenes)

| engine | content_acc | text_acc | box_recall | sym_recall | indent_exact | rows_ratio | ms/img | peak MB |
|---|---|---|---|---|---|---|---|---|
| mock | 1.000 | 1.000 | 1.00 | 1.000 | 0.90 | 1.00 | 6 | 7 |
| llamacpp | 0.946 | 0.946 | 1.00 | 0.960 | 0.86 | 1.05 | 3107 | 8 |
| tesseract | 0.934 | 0.934 | 1.00 | 0.851 | 0.90 | 0.96 | 403 | 44 |
