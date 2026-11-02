### per-scene content_acc

| engine | ansi-highlight | ascii-table | cargo-build | cjk-mixed | conflict | editor-rust | git-diff | git-log-graph | huge-4k | json-logs | light-ambiguous | nerd-symbols | nested-diff | powerline | pytest-traceback | real-editor-code | real-editor-term | small-text | softwrap | split-panes | symbols | tiny-crop | tree | ultra-tall | ultra-wide | wide-table |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| llamacpp | 0.995 | 0.966 | 0.972 | 0.994 | 1.000 | 0.934 | 0.996 | 0.996 | 1.000 | 1.000 | 0.985 | 0.969 | 1.000 | 0.915 | 0.951 | 0.996 | 0.953 | 0.991 | 0.997 | 0.990 | 0.996 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 |

### baseline (2x, 26 scenes)

| engine | content_acc | text_acc | box_recall | sym_recall | indent_exact | rows_ratio | ms/img | peak MB |
|---|---|---|---|---|---|---|---|---|
| llamacpp | 0.984 | 0.987 | 0.99 | 0.996 | 0.93 | 1.00 | 3548 | 71 |
