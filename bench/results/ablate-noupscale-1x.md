### per-scene content_acc

| engine | ansi-highlight | ascii-table | cargo-build | cjk-mixed | conflict | editor-rust | git-diff | git-log-graph | huge-4k | json-logs | light-ambiguous | nerd-symbols | nested-diff | powerline | pytest-traceback | real-editor-code | real-editor-term | small-text | softwrap | split-panes | symbols | tiny-crop | tree | ultra-tall | ultra-wide | wide-table |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| llamacpp | 0.984 | 0.578 | 0.982 | 0.994 | 1.000 | 0.934 | 0.999 | 0.996 | 0.960 | 0.990 | 0.970 | 0.969 | 1.000 | 0.915 | 0.848 | 0.977 | 0.867 | 0.982 | 1.000 | 0.985 | 0.988 | 0.708 | 0.767 | 1.000 | 0.858 | 1.000 |

### baseline (1x, 26 scenes)

| engine | content_acc | text_acc | box_recall | sym_recall | indent_exact | rows_ratio | ms/img | peak MB |
|---|---|---|---|---|---|---|---|---|
| llamacpp | 0.933 | 0.957 | 0.88 | 0.968 | 0.89 | 1.01 | 2994 | 28 |
