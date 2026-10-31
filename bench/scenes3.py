#!/usr/bin/env python3
"""Extreme geometry: images that are very small, very large, or extremely shaped.

These are the cases where a naive pipeline quietly breaks:
  * `tiny-crop`  — a 6.5pt crop; rows are ~14px tall, antialiasing dominates
  * `huge-4k`    — 4K canvas, 246 columns; vision-token budget explodes
  * `ultra-wide`  — 384 columns × 3 rows (aspect ≈ 40:1): one line is wider than
                    any model's native resolution
  * `ultra-tall` — 10 columns × 60 rows (aspect ≈ 1:12): a narrow column strip

Run after scenes.py / scenes2.py:  python3 scenes3.py
"""
import json

import scenes
from scenes import L, render

IMG = scenes.IMG
TRUTH = scenes.TRUTH

EXTREME = {}


def log_line(i: int, width: int = 246) -> str:
    """A realistic wide log line: every field varies, no self-repetition.

    (An earlier version padded each line by repeating its own tail, which turned
    this scene into a test of "does the model collapse intra-line repetition" —
    not "does it handle a 8.6 MP image". Real logs repeat *across* lines.)
    """
    fields = [
        f"[2026-10-30T11:{i // 60:02d}:{i % 60:02d}.{i * 7 % 1000:03d}Z]",
        f"level={'info' if i % 11 else 'warn'}",
        f"worker-{i % 8}",
        f"job={i:05d}",
        f"trace={i * 2654435761 % 0xFFFFFF:06x}",
        f"dur={i * 13 % 900 + 7}ms",
        f"queue={'api' if i % 3 else 'batch'}",
        f"node=ip-10-0-{i % 250}-{i % 17}",
        f"shard={i % 12}",
        f"replica={i % 3}",
        f"bytes={1024 + i * 97 % 65536}",
        f"retry={i % 2}",
        f"route=/api/v1/{'users' if i % 2 else 'orders'}/{i % 97}",
        f"status={200 if i % 9 else 503}",
        f"sql_ms={i * 3 % 120}",
        f"cache={'hit' if i % 4 else 'miss'}",
        f"fe={i % 5}",
        f"pod=api-{i % 40}",
        f"az={chr(97 + i % 3)}",
    ]
    return "  ".join(fields)[:width]


EXTREME["huge-4k"] = dict(
    palette="dark",
    lines=[L((log_line(i), "fg")) for i in range(56)],
)

EXTREME["ultra-wide"] = dict(
    palette="dark",
    lines=[
        L(("$ ", "green"), ("psql -c \"SELECT id, email, created_at, plan, mrr_cents, country, last_seen_at, trial_ends_at FROM accounts WHERE plan = 'team' AND mrr_cents > 5000 AND country IN ('US','DE','JP') ORDER BY mrr_cents DESC LIMIT 50;\"", "fg")),
        L((" id  | email                    | created_at          | plan | mrr_cents | country | last_seen_at        | trial_ends_at       ", "bold")),
        L((" 412 | ops@acme-corp.example    | 2025-03-11 09:14:22 | team |    249900 | US      | 2026-10-30 11:02:01 | 2027-01-08 00:00:00 ", "fg")),
    ],
)

EXTREME["ultra-tall"] = dict(
    palette="dark",
    lines=[
        L(("$ ", "green"), ("ls -1", "bold")),
    ]
    + [L((name, "fg")) for name in (
        "cli.py", "core.py", "db.py", "env.py", "fs.py", "git.py",
        "http.py", "io.py", "json.py", "log.py", "main.py", "net.py",
        "os.py", "path.py", "proc.py", "queue.py", "router.py", "schema.py",
        "shell.py", "sql.py", "state.py", "store.py", "sync.py", "task.py",
        "test.py", "time.py", "trace.py", "types.py", "ui.py", "url.py",
        "util.py", "vault.py", "view.py", "watch.py", "worker.py", "ws.py",
        "xml.py", "yaml.py", "zip.py", "zlib.py",
        "agent.py", "auth.py", "batch.py", "cache.py", "clock.py", "config.py",
        "crypto.py", "cursor.py", "daemon.py", "deps.py", "diff.py", "dns.py",
        "emit.py", "errors.py", "event.py", "filter.py", "format.py", "gc.py",
        "hash.py", "hook.py",
    )],
)

EXTREME["tiny-crop"] = dict(
    palette="dark",
    font_px=13,
    pad=6,
    lines=[
        L(("TypeError: cannot unpack", "red")),
        L(("  File \"app.py\", line 42", "fg")),
        L(("    a, b = get_pair()", "fg")),
        L(("ValueError: not enough", "red")),
        L(("  values to unpack", "fg")),
    ],
)


def main():
    manifest_path = TRUTH / "manifest.json"
    manifest = json.loads(manifest_path.read_text()) if manifest_path.exists() else {}
    for name, spec in EXTREME.items():
        base = render(name, spec)
        gt = "\n".join("".join(t for t, _ in line) for line in spec["lines"])
        (TRUTH / f"{name}.txt").write_text(gt + "\n", encoding="utf-8")
        base.save(IMG / f"{name}@2x.png")
        w, h = base.size
        base.resize((max(1, w // 2), max(1, h // 2)), scenes.Image.LANCZOS).save(IMG / f"{name}@1x.png")
        base.save(IMG / f"{name}@2x.jpg", quality=70)
        manifest[name] = {"size2x": [w, h], "lines": len(spec["lines"]), "chars": len(gt)}
        print(f"{name}: {w}x{h} ratio={w/h:.1f} {len(spec['lines'])} lines")
    manifest_path.write_text(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()
