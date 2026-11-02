### per-scene content_acc

| engine | ansi-highlight | ascii-table | cargo-build | cjk-mixed | conflict | editor-rust | git-diff | git-log-graph | huge-4k | json-logs | light-ambiguous | nerd-symbols | nested-diff | powerline | pytest-traceback | real-editor-code | real-editor-term | small-text | softwrap | split-panes | symbols | tiny-crop | tree | ultra-tall | ultra-wide | wide-table |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| llamacpp | 1.000 | 0.581 | 0.972 | 0.994 | 0.995 | 0.934 | 0.997 | 0.996 | 1.000 | 1.000 | 0.970 | 0.976 | 1.000 | 0.915 | 0.904 | 0.996 | 0.894 | 0.982 | 0.997 | 0.990 | 0.996 | 1.000 | 0.750 | 1.000 | 1.000 | 1.000 |
| mock | 1.000 | 1.000 | 1.000 | 0.994 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 |

### baseline (2x, 26 scenes)

| engine | content_acc | text_acc | box_recall | sym_recall | indent_exact | rows_ratio | ms/img | peak MB |
|---|---|---|---|---|---|---|---|---|
| mock | 1.000 | 1.000 | 1.00 | 1.000 | 0.99 | 1.00 | 10 | 77 |
| llamacpp | 0.955 | 0.972 | 0.88 | 0.994 | 0.91 | 0.98 | 5481 | 94 |
