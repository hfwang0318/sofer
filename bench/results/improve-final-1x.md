### per-scene content_acc

| engine | ansi-highlight | ascii-table | cargo-build | cjk-mixed | conflict | editor-rust | git-diff | git-log-graph | huge-4k | json-logs | light-ambiguous | nerd-symbols | nested-diff | powerline | pytest-traceback | real-editor-code | real-editor-term | small-text | softwrap | split-panes | symbols | tiny-crop | tree | ultra-tall | ultra-wide | wide-table |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| llamacpp | 0.995 | 0.581 | 0.972 | 0.994 | 1.000 | 0.934 | 0.996 | 0.996 | 1.000 | 1.000 | 0.970 | 0.963 | 1.000 | 0.915 | 0.908 | 0.996 | 0.894 | 0.991 | 0.997 | 0.985 | 0.996 | 1.000 | 0.949 | 1.000 | 1.000 | 1.000 |

### baseline (1x, 26 scenes)

| engine | content_acc | text_acc | box_recall | sym_recall | indent_exact | rows_ratio | ms/img | peak MB |
|---|---|---|---|---|---|---|---|---|
| llamacpp | 0.963 | 0.985 | 0.92 | 0.996 | 0.87 | 0.99 | 3349 | 138 |
