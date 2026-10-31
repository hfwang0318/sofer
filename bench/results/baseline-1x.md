### per-scene content_acc

| engine | ansi-highlight | ascii-table | cargo-build | cjk-mixed | conflict | editor-rust | git-diff | git-log-graph | huge-4k | json-logs | light-ambiguous | nerd-symbols | nested-diff | powerline | pytest-traceback | real-editor-code | real-editor-term | small-text | softwrap | split-panes | symbols | tiny-crop | tree | ultra-tall | ultra-wide | wide-table |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| llamacpp | 0.984 | 0.578 | 0.982 | 0.994 | 1.000 | 0.934 | 0.984 | 0.996 | 0.957 | 0.990 | 0.970 | 0.969 | 1.000 | 0.789 | 0.848 | 0.993 | 0.894 | 0.982 | 1.000 | 0.985 | 0.988 | 0.708 | 0.767 | 1.000 | 1.000 | 1.000 |
| mock | 1.000 | 1.000 | 1.000 | 0.994 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 |
| tesseract | 0.963 | 0.569 | 0.917 | 0.738 | 0.929 | 0.973 | 0.971 | 0.964 | 0.993 | 0.946 | 0.920 | 0.896 | 0.985 | 0.901 | 0.700 | 0.974 | 0.875 | 0.955 | 0.974 | 0.291 | 0.944 | 0.124 | 0.705 | 0.993 | 1.000 | 0.928 |

### baseline (1x, 26 scenes)

| engine | content_acc | text_acc | box_recall | sym_recall | indent_exact | rows_ratio | ms/img | peak MB |
|---|---|---|---|---|---|---|---|---|
| mock | 1.000 | 1.000 | 1.00 | 1.000 | 0.95 | 1.00 | 5 | 14 |
| llamacpp | 0.934 | 0.959 | 0.88 | 0.970 | 0.88 | 1.01 | 3886 | 23 |
| tesseract | 0.851 | 0.871 | 0.88 | 0.869 | 0.76 | 0.91 | 381 | 60 |
