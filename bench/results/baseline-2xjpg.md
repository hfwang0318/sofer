### per-scene content_acc

| engine | ansi-highlight | ascii-table | cargo-build | cjk-mixed | conflict | editor-rust | git-diff | git-log-graph | huge-4k | json-logs | light-ambiguous | nerd-symbols | nested-diff | powerline | pytest-traceback | real-editor-code | real-editor-term | small-text | softwrap | split-panes | symbols | tiny-crop | tree | ultra-tall | ultra-wide | wide-table |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| llamacpp | 0.995 | 0.581 | 0.972 | 0.994 | 1.000 | 0.934 | 0.997 | 0.996 | 1.000 | 1.000 | 0.985 | 0.969 | 1.000 | 0.915 | 0.911 | 0.996 | 0.894 | 0.991 | 0.997 | 0.990 | 0.996 | 1.000 | 0.767 | 1.000 | 1.000 | 1.000 |
| mock | 1.000 | 1.000 | 1.000 | 0.994 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 |
| tesseract | 0.974 | 0.578 | 0.935 | 0.726 | 0.959 | 0.981 | 0.987 | 0.980 | 0.990 | 0.996 | 0.950 | 0.890 | 0.985 | 0.859 | 0.792 | 0.977 | 0.871 | 0.982 | 0.992 | 0.270 | 0.781 | 1.000 | 0.756 | 0.979 | 1.000 | 0.964 |

### baseline (2xjpg, 26 scenes)

| engine | content_acc | text_acc | box_recall | sym_recall | indent_exact | rows_ratio | ms/img | peak MB |
|---|---|---|---|---|---|---|---|---|
| mock | 1.000 | 1.000 | 1.00 | 1.000 | 0.94 | 1.00 | 8 | 46 |
| llamacpp | 0.957 | 0.983 | 0.88 | 0.996 | 0.88 | 0.98 | 7959 | 59 |
| tesseract | 0.891 | 0.908 | 0.88 | 0.940 | 0.87 | 0.93 | 614 | 100 |
