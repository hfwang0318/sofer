#!/usr/bin/env python3
"""Generate synthetic-but-realistic code/terminal screenshots with exact ground truth.

Rendering emulates a Retina macOS terminal: Menlo 13pt at 2x, per-cell backgrounds,
256-color palette. Outputs png@2x (retina), png@1x (downscaled), jpg@2x q70 (chat-app
recompression) for each scene.
"""
import json
import os
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).parent
IMG = ROOT / "images"
TRUTH = ROOT / "truth"
MENLO = "/System/Library/Fonts/Menlo.ttc"

# Dracula-ish dark palette
DARK_BG = "#1e1f29"
LIGHT_BG = "#fbfaf7"

PAL = {
    "fg":      ("#e6e6e6", None, False),
    "dim":     ("#7d7d8a", None, False),
    "green":   ("#50fa7b", None, False),
    "red":     ("#ff5555", None, False),
    "yellow":  ("#f1fa8c", None, False),
    "cyan":    ("#8be9fd", None, False),
    "purple":  ("#bd93f9", None, False),
    "orange":  ("#ffb86c", None, False),
    "blue":    ("#6272a4", None, False),
    "bold":    ("#ffffff", None, True),
    "sel":     ("#f8f8f2", "#44475a", False),
    "errbg":   ("#ff5555", "#3d1f27", False),
}
LPAL = {
    "fg":      ("#3b3b3b", None, False),
    "dim":     ("#9a9a9a", None, False),
    "green":   ("#2e7d32", None, False),
    "red":     ("#c62828", None, False),
    "yellow":  ("#8d6e00", None, False),
    "cyan":    ("#00697a", None, False),
    "purple":  ("#6a3d9a", None, False),
    "orange":  ("#b35c00", None, False),
    "blue":    ("#3b5b9a", None, False),
    "bold":    ("#111111", None, True),
    "sel":     ("#111111", "#dcdcdc", False),
    "errbg":   ("#c62828", "#ffe4e4", False),
}


def L(*segs):
    """A line is a list of (text, style) segments."""
    return list(segs)


SCENES = {}

# ---------------------------------------------------------------- 1. git diff
SCENES["git-diff"] = dict(
    palette="dark",
    lines=[
        L(("~/src/api", "cyan"), (" (main)", "dim"), (" $ ", "fg"), ("git diff --stat", "bold")),
        L((" src/auth/session.ts | 14 ++++++++------", "fg")),
        L((" 1 file changed, 8 insertions(+), 6 deletions(-)", "fg")),
        L(),
        L(("diff --git a/src/auth/session.ts b/src/auth/session.ts", "bold")),
        L(("index 3f9a1c2..b7e4d05 100644", "dim")),
        L(("--- a/src/auth/session.ts", "red")),
        L(("+++ b/src/auth/session.ts", "green")),
        L(("@@ -41,9 +41,11 @@ export async function refreshToken(refresh: string) {", "cyan")),
        L(("   const payload = await verify(refresh, {", "fg")),
        L(("     algorithms: ['RS256'],", "fg")),
        L(("     issuer: config.jwt.issuer,", "fg")),
        L(("   });", "fg")),
        L(("-  if (!payload || payload.exp < Date.now() / 1000) {", "red")),
        L(("-    throw new UnauthorizedError('token expired');", "red")),
        L(("-  }", "red")),
        L(("+  if (!payload) throw new UnauthorizedError('invalid token');", "green")),
        L(("+  if (payload.exp * 1000 <= Date.now()) {", "green")),
        L(("+    throw new UnauthorizedError(`token expired at ${payload.exp}`);", "green")),
        L(("+  }", "green")),
        L(("   return sign({ sub: payload.sub, jti: randomUUID() }, secret, {", "fg")),
        L(("     expiresIn: '15m',", "fg")),
        L(("   });", "fg")),
    ],
)

# ------------------------------------------------------- 2. python traceback
SCENES["pytest-traceback"] = dict(
    palette="dark",
    lines=[
        L(("venv $ ", "green"), ("pytest tests/test_router.py -x -q", "bold")),
        L(("....F", "fg"), ("                                                                  [100%]", "dim")),
        L(("=================================== FAILURES ===================================", "red")),
        L(("________________________ test_nested_route_priority ________________________", "red")),
        L(),
        L(("    def test_nested_route_priority() -> None:", "fg")),
        L(("        app = create_app(", "fg")),
        L(("            routes=[", "fg")),
        L(("                Route('/a/{x:int}/b', handler_a),", "fg")),
        L(("                Route('/a/static/b', handler_b),", "fg")),
        L(("            ],", "fg")),
        L(("        )", "fg")),
        L(("        resp = client.get('/a/static/b')", "fg")),
        L((">       assert resp.status_code == 200", "bold")),
        L(("E       assert 404 == 200", "red")),
        L(("E        +  where 404 = <Response [404 Not Found]>.status_code", "red")),
        L(),
        L(("tests/test_router.py:88: AssertionError", "cyan")),
        L(("-------------------------- Captured stderr call --------------------------", "dim")),
        L(("WARNING  router: no match for path=/a/static/b methods={'GET'}", "yellow")),
    ],
)

# ------------------------------------------------------------- 3. tree
SCENES["tree"] = dict(
    palette="dark",
    lines=[
        L(("~/proj $ ", "cyan"), ("tree -L 3 --dirsfirst src", "bold")),
        L(("src", "blue")),
        L(("├── cli", "blue")),
        L(("│   ├── __init__.py", "fg")),
        L(("│   ├── main.py", "fg")),
        L(("│   └── options.py", "fg")),
        L(("├── core", "blue")),
        L(("│   ├── engine.py", "fg")),
        L(("│   ├── registry.py", "fg")),
        L(("│   └── workers", "blue")),
        L(("│       ├── pool.py", "fg")),
        L(("│       └── queue.py", "fg")),
        L(("└── utils.py", "fg")),
        L(),
        L(("3 directories, 8 files", "dim")),
    ],
)

# ------------------------------------------------------------- 4. table
SCENES["ascii-table"] = dict(
    palette="dark",
    lines=[
        L(("$ ", "green"), ("docker ps --format table", "bold")),
        L(("CONTAINER ID   IMAGE                 STATUS          PORTS                    NAMES", "bold")),
        L(("┌──────────────┬───────────────────────┬─────────────────┬──────────────────────────┐", "dim")),
        L(("│ 8f2c1a9e4b7d │ ghcr.io/acme/api:1.4 │ Up 3 hours      │ 0.0.0.0:8080->8080/tcp   │", "fg")),
        L(("│ 1d7e0c3b5a92 │ postgres:16-alpine    │ Up 3 hours      │ 5432/tcp                 │", "fg")),
        L(("│ c93b6f0d2e18 │ redis:7.2             │ Restarting (1)  │ 6379/tcp                 │", "red")),
        L(("└──────────────┴───────────────────────┴─────────────────┴──────────────────────────┘", "dim")),
        L(),
        L(("REPOSITORY            TAG       SIZE", "bold")),
        L(("acme/api              1.4       184MB", "fg")),
        L(("acme/worker           1.4       191MB", "fg")),
    ],
)

# ------------------------------------------------------- 5. code editor (rust)
SCENES["editor-rust"] = dict(
    palette="dark",
    lines=[
        L(("  1 use std::collections::HashMap;", "dim")),
        L(("  2 use crate::error::{Error, Result};", "dim")),
        L(("  3 ", "dim")),
        L(("  4 pub struct Cache<K, V> {", "purple")),
        L(("  5     map: HashMap<K, V>,", "fg")),
        L(("  6     capacity: usize,", "fg")),
        L(("  7 }", "purple")),
        L(("  8 ", "dim")),
        L(("  9 impl<K: Eq + std::hash::Hash, V: Clone> Cache<K, V> {", "purple")),
        L((" 10     pub fn get_or_insert(&mut self, key: K, f: impl FnOnce() -> V) -> Result<&V> {", "cyan")),
        L((" 11         if !self.map.contains_key(&key) {", "fg")),
        L((" 12             if self.map.len() >= self.capacity {", "fg")),
        L((" 13                 return Err(Error::Capacity { limit: self.capacity });", "orange")),
        L((" 14             }", "fg")),
        L((" 15             let value = f();", "fg")),
        L((" 16             self.map.insert(key.clone(), value);", "fg")),
        L((" 17         }", "fg")),
        L((" 18         Ok(self.map.get(&key).unwrap_or_else(|| unreachable!()))", "fg")),
        L((" 19     }", "fg")),
        L((" 20 }", "purple")),
    ],
)

# ------------------------------------------ 6. light theme ls + ambiguous glyphs
SCENES["light-ambiguous"] = dict(
    palette="light",
    lines=[
        L(("~ $ ", "dim"), ("ls -la build/ | head -6", "bold")),
        L(("total 1049", "fg")),
        L(("drwxr-xr-x  12 hf  staff   384 Oct 21 09:14 .", "fg")),
        L(("-rw-r--r--   1 hf  staff  1240 Oct 21 09:14 Il1O0.conf", "fg")),
        L(("-rw-r--r--   1 hf  staff  8192 Oct 21 09:15 O0Il.txt", "fg")),
        L(("-rw-r--r--   1 hf  staff    11 Oct 21 09:15 rn vs m", "fg")),
        L(("~ $ ", "dim"), ("echo \"a1l I0O rnm != => :: 0x1F\"", "bold")),
        L(("a1l I0O rnm != => :: 0x1F", "fg")),
        L(("~ $ ", "dim"), ("grep -n \"it's\" *.md | cut -d: -f1", "bold")),
        L(("README.md", "cyan")),
    ],
)

# ------------------------------------------------ 7. soft-wrapped narrow output
# Long log lines wrapped by the terminal at 52 columns; continuation lines start at
# column 0 (that is how iTerm/Terminal render them, and what OCR sees).
def _wrap52(parts):
    out, cur = [], ""
    for text, style in parts:
        for tok in [text[i:i + 52] for i in range(0, len(text), 52)]:
            if len(cur) + len(tok) > 52:
                out.append(L((cur, style)))
                cur = tok
            else:
                cur += tok
    if cur:
        out.append(L((cur, "fg")))
    return out


SCENES["softwrap"] = dict(
    palette="dark",
    lines=[
        L(("$ ", "green"), ("kubectl get events --sort-by=.lastTimestamp", "bold")),
        L(("LAST SEEN   TYPE      REASON    OBJECT             MESSAGE", "bold")),
    ] + _wrap52([("3m          Warning   BackOff   pod/api-6f8d9c4b-x2vn   Back-off restarting failed container api in pod api-6f8d9c4b-x2vn_ns(8f2c1a9e-4b7d-4c1e-9a2b-77d0c3b5a92e)", "fg")])
      + _wrap52([("1m          Normal    Pulled    pod/worker-5c7f4d8b-q9kz   Successfully pulled image \"ghcr.io/acme/worker:1.4\" in 2.184s (2.184s including waiting). Image size: 191234567 bytes.", "fg")]),
)


def hex2rgb(h):
    h = h.lstrip("#")
    return tuple(int(h[i:i + 2], 16) for i in (0, 2, 4))


def render(name, spec, scale=2, font_px=26, pad=18):
    font_px = spec.get("font_px", font_px)
    pad = spec.get("pad", pad)
    pal = PAL if spec["palette"] == "dark" else LPAL
    bg = hex2rgb(DARK_BG if spec["palette"] == "dark" else LIGHT_BG)
    font = ImageFont.truetype(MENLO, font_px)
    adv = font.getlength("M")
    lh = int(font_px * 1.45)
    cols = spec.get("width_cols") or max(
        sum(len(t) for t, _ in line) for line in spec["lines"] if line
    ) + 2
    rows = len(spec["lines"]) + 1
    W = int(cols * adv + pad * 2)
    H = int(rows * lh + pad * 2)
    img = Image.new("RGB", (W, H), bg)
    d = ImageDraw.Draw(img)
    for r, line in enumerate(spec["lines"]):
        y = pad + r * lh
        col = 0
        for text, style in line:
            fg, bgc, bold = pal[style]
            if bgc:
                d.rectangle([pad + col * adv, y, pad + (col + len(text)) * adv, y + lh], fill=hex2rgb(bgc))
            f = ImageFont.truetype(MENLO, font_px) if not bold else ImageFont.truetype(MENLO, font_px)
            x = pad + col * adv
            d.text((x, y), text, font=f, fill=hex2rgb(fg), anchor="la")
            col += len(text)
    return img


def degrade_photo(img, seed=0):
    """Simulate a photo of a screen: downscale, blur, sensor noise, heavy JPEG."""
    import numpy as np
    from PIL import ImageFilter

    w, h = img.size
    im = img.resize((max(1, int(w * 0.82)), max(1, int(h * 0.82))), Image.LANCZOS)
    im = im.filter(ImageFilter.GaussianBlur(0.6))
    a = np.asarray(im).astype(np.int16)
    rng = np.random.default_rng(seed)
    a = a + rng.normal(0, 5.0, a.shape)
    return Image.fromarray(np.clip(a, 0, 255).astype(np.uint8))


def main():
    IMG.mkdir(parents=True, exist_ok=True)
    TRUTH.mkdir(parents=True, exist_ok=True)
    manifest = {}
    for name, spec in SCENES.items():
        base = render(name, spec, scale=2)
        gt = "\n".join("".join(t for t, _ in line) for line in spec["lines"])
        (TRUTH / f"{name}.txt").write_text(gt + "\n", encoding="utf-8")
        base.save(IMG / f"{name}@2x.png")
        w, h = base.size
        small = base.resize((w // 2, h // 2), Image.LANCZOS)
        small.save(IMG / f"{name}@1x.png")
        base.save(IMG / f"{name}@2x.jpg", quality=70)
        if spec.get("photo"):
            photo = degrade_photo(base, seed=len(name))
            photo.save(IMG / f"{name}@photo.jpg", quality=62)
            manifest[name]["photo"] = list(photo.size)
        manifest[name] = {"size2x": [w, h], "lines": len(spec["lines"]), "chars": len(gt)}
        print(f"{name}: {w}x{h} {len(spec['lines'])} lines")
    (TRUTH / "manifest.json").write_text(json.dumps(manifest, indent=2))
    # sidecar for humans
    (ROOT / "SCENES.md").write_text(
        "# bench scenes\n\n"
        + "\n".join(f"- `{n}`: {m['size2x'][0]}x{m['size2x'][1]}px@2x, {m['lines']} lines, {m['chars']} chars"
                    for n, m in manifest.items()) + "\n"
    )


if __name__ == "__main__":
    main()
