### per-scene content_acc

| engine | ansi-highlight | ascii-table | cargo-build | cjk-mixed | conflict | deep-tree | diff-stat-mixed | editor-rust | git-diff | git-log-graph | html-nested | huge-4k | json-logs | light-ambiguous | light-editor | light-tree | log-columns | md-doc | nerd-symbols | nested-diff | nested-table | photo-table | powerline | psql-table | pytest-traceback | real-editor-code | real-editor-term | rules-mixed | small-text | softwrap | split-panes | symbols | tiling-table | tiny-crop | tree | tree-gutter | ultra-tall | ultra-wide | wide-table | yaml-config |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| llamacpp | 0.995 | 0.966 | 0.972 | 0.994 | 1.000 | 1.000 | 0.947 | 0.934 | 0.996 | 0.996 | 1.000 | 1.000 | 1.000 | 0.985 | 0.871 | 1.000 | 1.000 | 0.926 | 0.969 | 1.000 | 0.478 | 0.940 | 0.915 | 0.961 | 0.951 | 0.996 | 0.953 | 0.798 | 0.991 | 0.997 | 0.990 | 0.996 | 0.808 | 1.000 | 1.000 | 0.914 | 1.000 | 1.000 | 1.000 | 1.000 |

### baseline (2x, 40 scenes)

| engine | content_acc | text_acc | box_recall | sym_recall | indent_exact | rows_ratio | ms/img | peak MB |
|---|---|---|---|---|---|---|---|---|
| llamacpp | 0.956 | 0.975 | 0.94 | 0.988 | 0.88 | 0.99 | 2858 | 71 |
