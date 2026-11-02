#!/usr/bin/env python3
"""Held-out scenes for the layout-reconstruction pass.

The first 26 scenes grew alongside the code, so several of them are the reason
a rule exists. The fourteen here are written *after* the rules, from the shapes
the rules claim to handle. They are a **targeted regression set**, not a
hold-out set: they share the renderer and were chosen with the implementation
in view. Report them separately from the Dev-26.

  * `nested-table`   — a framed table with a middle `├──┼──┤` rule row
  * `psql-table`     — an ASCII `+----+` table the engine may already frame
  * `log-columns`    — space-aligned columns with no frame at all
  * `photo-table`    — a photographed framed table (box art must stay off)
  * `deep-tree`      — five levels of `│   ├──` prefixes
  * `tree-gutter`    — line numbers next to a tree
  * `html-nested`    — indentation without any drawing glyphs
  * `rules-mixed`    — `====`, `----`, `____`, `~~~~` of different lengths
  * `diff-stat-mixed`— `+/-` bars that must not be resized
  * `yaml-config`    — two-space YAML nesting
  * `md-doc`         — headings, list, fence and a Markdown table
  * `light-editor`   — light theme, line numbers and code indentation
  * `light-tree`     — the tree pass on the light palette
  * `tiling-table`   — a 40-row table that crosses tile boundaries

Run after scenes.py / scenes2.py / scenes3.py:  python3 scenes4.py
"""
import json

import scenes
from scenes import L, degrade_photo, render

IMG = scenes.IMG
TRUTH = scenes.TRUTH

NEW = {}

# ------------------------------------------------------- framed table + mid rule
NEW["nested-table"] = dict(
    palette="dark",
    lines=[
        L(("$ ", "green"), ("kubectl get pods", "bold")),
        L(("NAME        READY   STATUS", "bold")),
        L(("┌───────────┬───────┬─────────┐", "fg")),
        L(("│ api-1     │ 1/1   │ Running │", "fg")),
        L(("│ api-2     │ 1/1   │ Running │", "fg")),
        L(("├───────────┼───────┼─────────┤", "fg")),
        L(("│ worker-1  │ 2/2   │ Running │", "fg")),
        L(("└───────────┴───────┴─────────┘", "fg")),
        L(("3 pods, 2 deployments", "dim")),
    ],
)

# --------------------------------------------------- ASCII table (already drawn)
NEW["psql-table"] = dict(
    palette="dark",
    lines=[
        L(("$ ", "green"), ('psql -c "select * from users limit 3"', "bold")),
        L((" id | email              | plan", "bold")),
        L(("----+--------------------+------", "dim")),
        L(("  1 | alice@example.com  | team", "fg")),
        L(("  2 | bob@example.com    | free", "fg")),
        L(("  3 | carol@example.com  | pro", "fg")),
        L(("(3 rows)", "dim")),
    ],
)

# ----------------------------------------------------- space-aligned columns
NEW["log-columns"] = dict(
    palette="dark",
    lines=[
        L(("2026-10-30 11:02:13 INFO  api      request path=/api/v1/users status=200 dur=13ms", "fg")),
        L(("2026-10-30 11:02:13 WARN  worker   slow query id=42 dur=842ms", "yellow")),
        L(("2026-10-30 11:02:14 ERROR api      unhandled TypeError at handler.ts:44", "red")),
        L(("2026-10-30 11:02:14 INFO  worker   retry attempt=2 backoff=250ms", "fg")),
        L(("2026-10-30 11:02:15 INFO  api      request path=/api/v1/orders status=201 dur=27ms", "fg")),
    ],
)

# ------------------------------------------------------- photographed table
NEW["photo-table"] = dict(
    palette="dark",
    photo=True,
    lines=[
        L(("$ ", "green"), ("docker ps", "bold")),
        L(("CONTAINER ID   IMAGE          STATUS", "bold")),
        L(("┌──────────────┬──────────────┬─────────┐", "fg")),
        L(("│ 8f2c1a9e4b7d │ api:1.4      │ Up 3h   │", "fg")),
        L(("│ 1d7e0c3b5a92 │ postgres:16  │ Up 3h   │", "fg")),
        L(("│ c93b6f0d2e18 │ redis:7.2    │ Up 1h   │", "fg")),
        L(("└──────────────┴──────────────┴─────────┘", "fg")),
    ],
)

# -------------------------------------------------------------- deep tree
NEW["deep-tree"] = dict(
    palette="dark",
    lines=[
        L(("$ ", "green"), ("tree -L 5 --dirsfirst src", "bold")),
        L(("src", "fg")),
        L(("├── cli", "fg")),
        L(("│   ├── commands", "fg")),
        L(("│   │   ├── build.rs", "fg")),
        L(("│   │   └── serve.rs", "fg")),
        L(("│   └── main.rs", "fg")),
        L(("├── core", "fg")),
        L(("│   ├── engine", "fg")),
        L(("│   │   ├── pool.rs", "fg")),
        L(("│   │   └── queue.rs", "fg")),
        L(("│   └── lib.rs", "fg")),
        L(("└── utils.rs", "fg")),
    ],
)

# ------------------------------------------------------- line numbers + tree
NEW["tree-gutter"] = dict(
    palette="dark",
    lines=[
        L(("  1 // rendered file layout", "dim")),
        L(("  2 src/", "fg")),
        L(("  3 ├── cli/", "fg")),
        L(("  4 │   ├── main.rs", "fg")),
        L(("  5 │   └── serve.rs", "fg")),
        L(("  6 ├── core/", "fg")),
        L(("  7 │   ├── pool.rs", "fg")),
        L(("  8 │   └── queue.rs", "fg")),
        L(("  9 └── utils.rs", "fg")),
    ],
)

# ------------------------------------------------------------ HTML nesting
NEW["html-nested"] = dict(
    palette="light",
    lines=[
        L(('<ul class="nav">', "fg")),
        L(("  <li>", "fg")),
        L(('    <a href="/home">Home</a>', "fg")),
        L(("  </li>", "fg")),
        L(("  <li>", "fg")),
        L(('    <a href="/docs">Docs</a>', "fg")),
        L(("    <ul>", "fg")),
        L(('      <li><a href="/docs/api">API</a></li>', "fg")),
        L(('      <li><a href="/docs/faq">FAQ</a></li>', "fg")),
        L(("    </ul>", "fg")),
        L(("  </li>", "fg")),
        L(("</ul>", "fg")),
    ],
)

# --------------------------------------------------- decoration lines mixed
NEW["rules-mixed"] = dict(
    palette="dark",
    lines=[
        L(("$ ", "green"), ("./run.sh", "bold")),
        L(("==================== BUILD ====================", "cyan")),
        L(("Compiling core v0.4.1", "fg")),
        L(("----------------------------------------------", "dim")),
        L(("warning: unused import `std::fs`", "yellow")),
        L(("______________________________________________", "dim")),
        L(("test result: ok. 42 passed; 0 failed", "green")),
        L(("~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~", "dim")),
        L(("diff --git a/x b/x", "bold")),
        L(("--- a/x", "red")),
        L(("+++ b/x", "green")),
    ],
)

# ------------------------------------------------------ diff stat bars mixed
NEW["diff-stat-mixed"] = dict(
    palette="dark",
    lines=[
        L(("$ ", "green"), ("git diff --stat HEAD~3", "bold")),
        L((" src/auth/session.ts      | 42 ++++++++++++++++++++++------------", "fg")),
        L((" src/core/engine.rs       | 18 ++++++++++----", "fg")),
        L((" docs/api.md              |  6 ++--", "fg")),
        L((" assets/logo.png          | Bin 18432 -> 19001 bytes", "fg")),
        L((" 4 files changed, 49 insertions(+), 27 deletions(-)", "fg")),
    ],
)

# --------------------------------------------------------------- YAML indents
NEW["yaml-config"] = dict(
    palette="dark",
    lines=[
        L(("$ ", "green"), ("cat config.yaml", "bold")),
        L(("server:", "fg")),
        L(("  host: 127.0.0.1", "fg")),
        L(("  port: 8080", "fg")),
        L(("  tls:", "fg")),
        L(("    enabled: true", "fg")),
        L(("    cert: /etc/ssl/cert.pem", "fg")),
        L(("workers:", "fg")),
        L(("  - name: api", "fg")),
        L(("    replicas: 3", "fg")),
        L(("  - name: worker", "fg")),
        L(("    replicas: 2", "fg")),
        L(("# end", "dim")),
    ],
)

# ------------------------------------------------------------ Markdown doc
NEW["md-doc"] = dict(
    palette="light",
    lines=[
        L(("# Deployment Guide", "bold")),
        L(),
        L(("## Prerequisites", "bold")),
        L(),
        L(("- Docker 24+", "fg")),
        L(("- kubectl 1.29+", "fg")),
        L(),
        L(("## Steps", "bold")),
        L(),
        L(("1. Build the image:", "fg")),
        L(("   ```bash", "dim")),
        L(("   docker build -t api:latest .", "fg")),
        L(("   ```", "dim")),
        L(("2. Apply the manifest:", "fg")),
        L(),
        L(("| Resource | Replicas |", "fg")),
        L(("|----------|----------|", "fg")),
        L(("| api      | 3        |", "fg")),
        L(("| worker   | 2        |", "fg")),
    ],
)

# -------------------------------------------------- light editor with gutter
NEW["light-editor"] = dict(
    palette="light",
    lines=[
        L(("  1 use std::sync::Arc;", "fg")),
        L(("  2", "fg")),
        L(("  3 #[derive(Debug)]", "purple")),
        L(("  4 pub struct Pool {", "bold")),
        L(("  5     size: usize,", "fg")),
        L(("  6     queue: Arc<Queue>,", "fg")),
        L(("  7 }", "fg")),
        L(("  8", "fg")),
        L(("  9 impl Pool {", "bold")),
        L((" 10     pub fn new(size: usize) -> Self {", "fg")),
        L((" 11         Self {", "fg")),
        L((" 12             size,", "fg")),
        L((" 13             queue: Arc::new(Queue::new()),", "fg")),
        L((" 14         }", "fg")),
        L((" 15     }", "fg")),
        L((" 16 }", "fg")),
    ],
)


# ------------------------------------------------------- light theme tree
NEW["light-tree"] = dict(
    palette="light",
    lines=[
        L(("$ ", "green"), ("tree -L 2 src", "bold")),
        L(("src", "fg")),
        L(("├── api", "fg")),
        L(("│   ├── main.rs", "fg")),
        L(("│   └── routes.rs", "fg")),
        L(("├── core", "fg")),
        L(("│   └── lib.rs", "fg")),
        L(("└── utils.rs", "fg")),
    ],
)

# ------------------------------------------- table large enough to tile
_tile_rows = [
    L((f"│ pod-{i:02d}     │ 1/1   │ Running │", "fg")) for i in range(40)
]
NEW["tiling-table"] = dict(
    palette="dark",
    lines=[L(("$ ", "green"), ("kubectl get pods -A", "bold"))]
    + [
        L(("NAME        READY   STATUS", "bold")),
        L(("┌───────────┬───────┬─────────┐", "fg")),
    ]
    + _tile_rows
    + [L(("└───────────┴───────┴─────────┘", "fg"))],
)

def main():
    manifest_path = TRUTH / "manifest.json"
    manifest = json.loads(manifest_path.read_text()) if manifest_path.exists() else {}
    for name, spec in NEW.items():
        base = render(name, spec)
        gt = "\n".join("".join(t for t, _ in line) for line in spec["lines"])
        (TRUTH / f"{name}.txt").write_text(gt + "\n", encoding="utf-8")
        base.save(IMG / f"{name}@2x.png")
        w, h = base.size
        base.resize((max(1, w // 2), max(1, h // 2)), scenes.Image.LANCZOS).save(
            IMG / f"{name}@1x.png"
        )
        base.save(IMG / f"{name}@2x.jpg", quality=70)
        if spec.get("photo"):
            degrade_photo(base, seed=len(name)).save(IMG / f"{name}@photo.jpg", quality=62)
        manifest[name] = {"size2x": [w, h], "lines": len(spec["lines"]), "chars": len(gt)}
        print(f"{name}: {w}x{h} {len(spec['lines'])} lines")
    # Scenes that predate the generators (the two real screenshots and
    # cjk-mixed) have no entry; derive it from the image and truth so the
    # manifest and SCENES.md cover the whole corpus.
    for truth in sorted(TRUTH.glob("*.txt")):
        name = truth.stem
        if name in manifest or name == "manifest":
            continue
        img = IMG / f"{name}@2x.png"
        if not img.exists():
            continue
        w, h = scenes.Image.open(img).size
        text = truth.read_text(encoding="utf-8")
        manifest[name] = {
            "size2x": [w, h],
            "lines": len(text.rstrip("\n").split("\n")),
            "chars": len(text.replace("\n", "")),
        }
    manifest_path.write_text(json.dumps(manifest, indent=2))
    (scenes.ROOT / "SCENES.md").write_text(
        "# bench scenes\n\n"
        + "\n".join(
            f"- `{n}`: {m['size2x'][0]}x{m['size2x'][1]}px@2x, {m['lines']} lines, {m['chars']} chars"
            for n, m in manifest.items()
        )
        + "\n"
    )


if __name__ == "__main__":
    main()
