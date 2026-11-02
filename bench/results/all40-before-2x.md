### per-scene content_acc

| engine | ansi-highlight | ascii-table | cargo-build | cjk-mixed | conflict | deep-tree | diff-stat-mixed | editor-rust | git-diff | git-log-graph | html-nested | huge-4k | json-logs | light-ambiguous | light-editor | light-tree | log-columns | md-doc | nerd-symbols | nested-diff | nested-table | photo-table | powerline | psql-table | pytest-traceback | real-editor-code | real-editor-term | rules-mixed | small-text | softwrap | split-panes | symbols | tiling-table | tiny-crop | tree | tree-gutter | ultra-tall | ultra-wide | wide-table | yaml-config |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| llamacpp | 0.995 | 0.581 | 0.972 | 0.994 | 1.000 | 0.685 | 0.947 | 0.934 | 0.996 | 0.996 | 1.000 | 1.000 | 1.000 | 0.985 | 0.871 | 0.708 | 1.000 | 0.926 | 0.969 | 1.000 | 0.478 | 0.530 | 0.915 | 0.961 | 0.911 | 0.996 | 0.894 | 0.700 | 0.991 | 0.997 | 0.990 | 0.996 | 0.752 | 1.000 | 0.767 | 0.676 | 1.000 | 1.000 | 1.000 | 1.000 |

### baseline (2x, 40 scenes)

| engine | content_acc | text_acc | box_recall | sym_recall | indent_exact | rows_ratio | ms/img | peak MB |
|---|---|---|---|---|---|---|---|---|
| llamacpp | 0.903 | 0.968 | 0.78 | 0.988 | 0.86 | 0.97 | 3118 | 71 |
