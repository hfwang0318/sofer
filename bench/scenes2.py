#!/usr/bin/env python3
"""Complex scenes: the ones that actually break OCR engines.

Everything here is rendered the same way as `scenes.py` (Menlo 13pt @2x, real
palette) but exercises harder content: commit graphs, compiler diagnostics,
JSON logs, selection highlights, ligature-prone operators, merge markers,
wide numeric tables, tiny text, and a two-pane split.

Run after scenes.py:  python3 scenes.py && python3 scenes2.py
"""
import json

from PIL import Image, ImageDraw, ImageFont

import scenes
from scenes import DARK_BG, LIGHT_BG, LPAL, PAL, L, hex2rgb, render

IMG = scenes.IMG
TRUTH = scenes.TRUTH
MENLO = scenes.MENLO

# an inverse-video block, i.e. a terminal cursor parked on a cell
PAL["cursor"] = ("#1e1f29", "#e6e6e6", False)

NEW = {}

# ------------------------------------------------------ commit graph
NEW["git-log-graph"] = dict(
    palette="dark",
    lines=[
        L(("~/src/api (main) $ ", "cyan"), ("git log --graph --oneline -6", "bold")),
        L(("* 8f2c1a9 (HEAD -> main) fix: guard against expired token", "yellow")),
        L(("* 4b7d9e0 feat: add refresh flow", "yellow")),
        L(("|\\", "fg")),
        L(("| * 1d7e0c3 (origin/feature) test: cover refresh edge cases", "green")),
        L(("| * c93b6f0 refactor: extract token verifier", "green")),
        L(("|/", "fg")),
        L(("* b7e4d05 chore: bump deps", "yellow")),
        L(("* 3f9a1c2 initial commit", "dim")),
    ],
)

# -------------------------------------------------- compiler diagnostics
NEW["cargo-build"] = dict(
    palette="dark",
    photo=True,
    lines=[
        L(("$ ", "green"), ("cargo build", "bold")),
        L(("   Compiling ", "fg"), ("api v0.4.1", "bold"), (" (/Users/hf/src/api)", "dim")),
        L(("warning", "yellow"), (": unused variable: ", "fg"), ("`retry`", "yellow")),
        L(("  --> ", "cyan"), ("src/auth/session.ts:42:9", "fg")),
        L(("   |", "dim")),
        L(("42 |", "dim"), ("     let retry = opts.retries;", "fg")),
        L(("   |", "dim"), ("         ", "fg"), ("^^^^^", "yellow"), (" help: prefix it with `_`", "fg")),
        L(("   |", "dim")),
        L(("error[E0308]", "red"), (": mismatched types", "fg")),
        L(("  --> ", "cyan"), ("src/core/engine.rs:88:24", "fg")),
        L(("   |", "dim")),
        L(("88 |", "dim"), ("             let handle = spawn(worker);", "fg")),
        L(("   |", "dim"), ("                          ", "fg"), ("^^^^^^", "red"), (" expected `Worker`, found `&Worker`", "fg")),
        L(("   |", "dim")),
        L(("error: ", "red"), ("could not compile `api` due to 1 previous error", "fg")),
    ],
)

# ------------------------------------------------------------ JSON logs
NEW["json-logs"] = dict(
    palette="dark",
    font_px=22,
    lines=[
        L(('{"ts":"2026-10-30T11:02:13.412Z","level":"info","msg":"request","path":"/api/v1/users?limit=20","status":200,"dur_ms":13}', "fg")),
        L(('{"ts":"2026-10-30T11:02:13.418Z","level":"warn","msg":"slow query","sql":"SELECT * FROM users WHERE id = $1","dur_ms":842}', "yellow")),
        L(('{"ts":"2026-10-30T11:02:14.001Z","level":"error","msg":"unhandled","err":"TypeError: Cannot read properties of undefined (reading \'id\')","stack":"at handler (src/routes/user.ts:44:18)"}', "red")),
        L(('{"ts":"2026-10-30T11:02:14.003Z","level":"info","msg":"retry","attempt":2,"backoff_ms":250}', "fg")),
    ],
)

# ------------------------------------- selection highlight + parked cursor
NEW["ansi-highlight"] = dict(
    palette="dark",
    lines=[
        L(("$ ", "green"), ("git status", "bold")),
        L(("On branch main", "fg")),
        L(("Your branch is up to date with 'origin/main'.", "fg")),
        L(),
        L(("Changes not staged for commit:", "fg")),
        L(('  (use "git add <file>..." to update what will be committed)', "dim")),
        L(("\tmodified:   src/auth/session.ts", "sel")),
        L(("\tmodified:   src/core/engine.rs", "red")),
        L(("$ ", "green"), ("", "cursor")),
    ],
)

# ------------------------------------------- ligature / operator heavy code
NEW["symbols"] = dict(
    palette="dark",
    lines=[
        L(("if (a => b && c !== d) { x = a != b; y = a === b; }", "fg")),
        L(("let arrow = (x) => x ** 2;   // => != === <= >=", "fg")),
        L(("const path = `~/src/${name}/index.ts`;", "fg")),
        L(("matrix[i][j] *= 1.5e-3;  // 0x1F | 0b1010 ^ 0o17", "fg")),
        L(("type T = { a?: string } & Record<string, number>;", "fg")),
        L(("const re = /^[a-z0-9_-]+$/gi;  // \\d{2,4} \\s+", "fg")),
        L(("x <<= 2; y >>= 1; z >>>= 3; w = ~x & (y | z) ^ 0xFF;", "fg")),
    ],
)

# ----------------------------------------------------- merge conflict
NEW["conflict"] = dict(
    palette="dark",
    lines=[
        L(("~/src/api $ ", "cyan"), ("git diff --check", "bold")),
        L(("src/core/timeout.ts:12: leftover conflict marker", "red")),
        L(),
        L(("<<<<<<< HEAD", "purple")),
        L(("const timeout = 30_000;", "fg")),
        L(("=======", "purple")),
        L(("const timeout = 15_000;", "fg")),
        L((">>>>>>> feature/fast-timeout", "purple")),
        L(),
        L(("3 files changed, 12 insertions(+), 4 deletions(-)", "fg")),
    ],
)

# ------------------------------------------------------ wide numeric table
NEW["wide-table"] = dict(
    palette="light",
    lines=[
        L(("NAME              QPS    P95(ms)   ERR%    MEM     REGION", "bold")),
        L(("api-gateway     12480      38.2   0.02    512M    us-east-1", "fg")),
        L(("auth-service     9310      41.7   0.00    384M    us-east-1", "fg")),
        L(("search-index     2205     210.4   1.31    2.1G    eu-west-1", "fg")),
        L(("billing-worker    410      12.9   0.00    128M    ap-east-1", "fg")),
        L(("total           24405         -      -     3.1G           -", "dim")),
    ],
)

# ------------------------------------------------------------- tiny text
NEW["small-text"] = dict(
    palette="dark",
    font_px=17,
    pad=10,
    lines=[
        L(("$ ", "green"), ("make test", "bold")),
        L(("FAIL  src/core/engine.test.ts", "red")),
        L(("  ✗ parses nested routes (12 ms)", "fg")),
        L(("  ✓ parses flat routes (3 ms)", "green")),
        L(("Tests: 1 failed, 1 passed, 2 total", "fg")),
    ],
)


# ---------------------------------------------- modern CLI iconography (gh/delta)
NEW["nerd-symbols"] = dict(
    palette="dark",
    lines=[
        L(("~/src/api ", "cyan"), ("main", "purple"), (" ?2 ", "yellow"), ("↑1", "green")),
        L(("✓ ", "green"), ("build", "fg"), ("  412ms", "dim")),
        L(("✗ ", "red"), ("lint", "fg"), ("  3 problems (1 error, 2 warnings)", "dim")),
        L(("⚠ ", "yellow"), ("deprecated: use `verifyToken` instead", "fg")),
        L(("● ", "green"), ("api-gateway", "fg"), ("  →  ", "dim"), ("127.0.0.1:8080", "fg")),
        L(("◆ ", "purple"), ("3 files changed", "fg"), ("  ↑ 42  ↓ 7", "green")),
        L(("⌘ ", "dim"), ("press ", "fg"), ("⌘⇧P", "bold"), (" to run a command", "fg")),
    ],
)

# --------------------------------------------------------- diff inside a diff
NEW["nested-diff"] = dict(
    palette="dark",
    lines=[
        L(("~/src/api $ ", "cyan"), ("git diff -- patches/fix.patch", "bold")),
        L(("@@ -1,7 +1,7 @@", "cyan")),
        L((" --- a/src/core/engine.rs", "fg")),
        L((" +++ b/src/core/engine.rs", "fg")),
        L(("-@@ -88,7 +88,7 @@ impl Engine {", "red")),
        L(("+@@ -88,9 +88,9 @@ impl Engine {", "green")),
        L((" -            let handle = spawn(worker);", "fg")),
        L((" -            handle.join();", "fg")),
        L(("+             let handle = spawn(worker)?;", "fg")),
        L(("+             handle.join()?;", "fg")),
        L(("-- ", "red"), ("a/src/core/engine.rs", "red")),
        L(("++ ", "green"), ("b/src/core/engine.rs", "green")),
    ],
)


# ------------------------------------- Powerline / Nerd Font private-use glyphs
# Menlo has no PUA glyphs, so these render as tofu — which is exactly what the
# user sees without a patched font, and what OCR has to survive. Ground truth
# deliberately omits the glyphs: a PUA codepoint has no textual identity, so the
# question is only "does the model hallucinate text into the gap".
PUA_A, PUA_B, PUA_C = "\ue0b0", "\ue0b1", "\ue0a0"
NEW["powerline"] = dict(
    palette="dark",
    lines=[
        L((f" {PUA_A} ~/src/api ", "fg"), (PUA_B, "fg"), (" main ", "purple"), (PUA_B, "fg"), (" 2", "yellow")),
        L((f" {PUA_A} ", "fg"), ("git status", "bold")),
        L(("On branch main", "fg")),
        L((f" {PUA_A} ", "fg"), ("npm run test", "bold")),
        L((f" {(PUA_C)} ", "green"), ("PASS  src/auth.test.ts", "green")),
    ],
)

SCENES = scenes.SCENES
SCENES.update(NEW)


def render_two_pane(spec):
    """tmux-style split: two panes, a vertical rule, one line of ground truth per
    visual row (that is what a reader (or an OCR) actually sees)."""
    left, right = spec["left"], spec["right"]
    font_px = 26
    font = ImageFont.truetype(MENLO, font_px)
    adv = font.getlength("M")
    lh = int(font_px * 1.45)
    pad = 16
    lc = max(len(t) for t, _ in left)
    rc = max(len(t) for t, _ in right)
    W = int((lc + rc + 3) * adv + pad * 2)
    H = int((max(len(left), len(right)) + 1) * lh + pad * 2)
    img = Image.new("RGB", (W, H), hex2rgb(DARK_BG))
    d = ImageDraw.Draw(img)
    split_x = pad + (lc + 1) * adv
    d.rectangle([split_x, pad // 2, split_x + 2, H - pad // 2], fill=hex2rgb("#44475a"))

    for pane, (x0, lines) in enumerate(((pad, left), (split_x + adv, right))):
        for r, (text, style) in enumerate(lines):
            fg, bgc, _ = PAL[style]
            y = pad + r * lh
            if bgc:
                d.rectangle([x0, y, x0 + len(text) * adv, y + lh], fill=hex2rgb(bgc))
            d.text((x0, y), text, font=font, fill=hex2rgb(fg), anchor="la")

    # Ground truth is *column-major*: a split screen has no canonical reading
    # order, and for a copy-to-clipboard tool the useful unit is one pane. Models
    # (and humans) read left pane first; scoring against row-interleaved text
    # would punish the behaviour we actually want.
    truth = [t for t, _ in left] + [t for t, _ in right]
    return img, "\n".join(truth)


TWO_PANE = {
    "split-panes": {
        "left": [
            ("$ npm run dev", "cyan"),
            ("", "fg"),
            ("  VITE v6.2.1  ready in 412 ms", "green"),
            ("", "fg"),
            ("  ➜  Local:   http://localhost:5173/", "fg"),
            ("  ➜  Network: use --host to expose", "dim"),
        ],
        "right": [
            ("$ npm run test:watch", "cyan"),
            ("", "fg"),
            (" PASS  src/auth/session.test.ts", "green"),
            (" FAIL  src/core/engine.test.ts", "red"),
            ("  ● parses nested routes", "red"),
            ("    expected 200, got 404", "fg"),
        ],
    }
}


def main():
    manifest_path = TRUTH / "manifest.json"
    manifest = json.loads(manifest_path.read_text()) if manifest_path.exists() else {}

    for name, spec in NEW.items():
        base = render(name, spec)
        gt = "\n".join("".join(t for t, _ in line) for line in spec["lines"])
        (TRUTH / f"{name}.txt").write_text(gt + "\n", encoding="utf-8")
        base.save(IMG / f"{name}@2x.png")
        w, h = base.size
        base.resize((w // 2, h // 2), Image.LANCZOS).save(IMG / f"{name}@1x.png")
        base.save(IMG / f"{name}@2x.jpg", quality=70)
        if spec.get("photo"):
            photo = scenes.degrade_photo(base, seed=len(name))
            photo.save(IMG / f"{name}@photo.jpg", quality=62)
        manifest[name] = {"size2x": [w, h], "lines": len(spec["lines"]), "chars": len(gt)}
        print(f"{name}: {w}x{h} {len(spec['lines'])} lines")

    # photo-of-a-screen variants for scenes defined in scenes.py
    for name in ("git-diff", "git-log-graph", "pytest-traceback"):
        spec = scenes.SCENES[name]
        base = render(name, spec)
        photo = scenes.degrade_photo(base, seed=len(name))
        photo.save(IMG / f"{name}@photo.jpg", quality=62)
        print(f"{name}: photo variant")

    for name, spec in TWO_PANE.items():
        img, gt = render_two_pane(spec)
        (TRUTH / f"{name}.txt").write_text(gt + "\n", encoding="utf-8")
        img.save(IMG / f"{name}@2x.png")
        w, h = img.size
        img.resize((w // 2, h // 2), Image.LANCZOS).save(IMG / f"{name}@1x.png")
        img.save(IMG / f"{name}@2x.jpg", quality=70)
        manifest[name] = {"size2x": [w, h], "lines": len(gt.split("\n")), "chars": len(gt)}
        print(f"{name}: {w}x{h} (two panes)")

    manifest_path.write_text(json.dumps(manifest, indent=2))
    lines = [f"- `{n}`: {m['size2x'][0]}x{m['size2x'][1]}px@2x, {m['lines']} lines, {m['chars']} chars"
             for n, m in manifest.items()]
    (scenes.ROOT / "SCENES.md").write_text("# bench scenes\n\n" + "\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
