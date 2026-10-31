### per-scene content_acc

| engine | ansi-highlight | ascii-table | cargo-build | cjk-mixed | conflict | editor-rust | git-diff | git-log-graph | huge-4k | json-logs | light-ambiguous | nerd-symbols | nested-diff | powerline | pytest-traceback | real-editor-code | real-editor-term | small-text | softwrap | split-panes | symbols | tiny-crop | tree | ultra-tall | ultra-wide | wide-table |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| llamacpp | 0.995 | 0.581 | 0.972 | 0.994 | 1.000 | 0.934 | 0.996 | 0.996 | 1.000 | 1.000 | 0.985 | 0.969 | 1.000 | 0.915 | 0.911 | 0.996 | 0.894 | 0.991 | 0.997 | 0.990 | 0.996 | 1.000 | 0.767 | 1.000 | 1.000 | 1.000 |
| mock | 1.000 | 1.000 | 1.000 | 0.994 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 |
| tesseract | 0.974 | 0.578 | 0.929 | 0.707 | 0.954 | 0.996 | 0.984 | 0.976 | 0.991 | 0.996 | 0.954 | 0.902 | 0.996 | 0.873 | 0.786 | 0.977 | 0.887 | 0.982 | 1.000 | 0.270 | 0.916 | 1.000 | 0.767 | 0.998 | 0.997 | 0.969 |

### baseline (2x, 26 scenes)

| engine | content_acc | text_acc | box_recall | sym_recall | indent_exact | rows_ratio | ms/img | peak MB |
|---|---|---|---|---|---|---|---|---|
| mock | 1.000 | 1.000 | 1.00 | 1.000 | 0.99 | 1.00 | 8 | 44 |
| llamacpp | 0.957 | 0.983 | 0.88 | 0.996 | 0.91 | 0.98 | 3884 | 61 |
| tesseract | 0.899 | 0.916 | 0.88 | 0.949 | 0.87 | 0.93 | 596 | 93 |
