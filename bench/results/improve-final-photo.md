### per-scene content_acc

| engine | cargo-build | git-diff | git-log-graph | pytest-traceback |
|---|---|---|---|---|
| llamacpp | 0.972 | 0.997 | 0.996 | 0.839 |

### baseline (photo, 4 scenes)

| engine | content_acc | text_acc | box_recall | sym_recall | indent_exact | rows_ratio | ms/img | peak MB |
|---|---|---|---|---|---|---|---|---|
| llamacpp | 0.951 | 0.951 | 1.00 | 0.975 | 0.73 | 1.05 | 1178 | 9 |
