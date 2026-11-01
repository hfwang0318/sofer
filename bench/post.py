#!/usr/bin/env python3
"""Prototype code-aware post-processing for raw OCR observations.

Turns flat observations (text + rect) into a monospace-faithful text block:
  * cluster observations into visual rows (gutter numbers, split tokens)
  * order cells left-to-right and rebuild inter-token spaces from pixel gaps
  * derive leading indentation from x offset / estimated char width
  * insert blank lines from vertical gaps

Vision rects are [x0, y0, x1, y1] with y increasing downwards.

This is an evaluation prototype for the sofer selection, not product code.
"""
from __future__ import annotations

import json
import statistics
import sys
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class Obs:
    text: str
    x0: float
    y0: float
    x1: float
    y1: float

    @property
    def w(self) -> float:
        return self.x1 - self.x0

    @property
    def h(self) -> float:
        return self.y1 - self.y0

    @property
    def yc(self) -> float:
        return (self.y0 + self.y1) / 2


def parse(payload: dict) -> list[Obs]:
    return [Obs(line["text"], *line["bbox"]) for line in payload["lines"]]


def cluster_rows(obs: list[Obs]) -> list[list[Obs]]:
    """Group observations whose vertical extents overlap into the same visual row."""
    rows: list[list[Obs]] = []
    for o in sorted(obs, key=lambda o: o.yc):
        for row in rows:
            ref = max(row, key=lambda r: r.h)
            overlap = min(o.y1, ref.y1) - max(o.y0, ref.y0)
            if overlap > 0.35 * min(o.h, ref.h):
                row.append(o)
                break
        else:
            rows.append([o])
    for row in rows:
        row.sort(key=lambda o: o.x0)
    rows.sort(key=lambda r: r[0].yc)
    return rows


def estimate_char_width(obs: list[Obs]) -> float:
    """Median pixel width per character across monospace-ish observations."""
    widths = [o.w / len(o.text) for o in obs if len(o.text) >= 4]
    return statistics.median(widths) if widths else 10.0


def rebuild_row(row: list[Obs], char_w: float) -> tuple[float, str]:
    """Join cells with spaces derived from pixel gaps. Returns (x0, text)."""
    row = sorted(row, key=lambda o: o.x0)
    text = row[0].text
    prev_x1 = row[0].x1
    for o in row[1:]:
        gap = o.x0 - prev_x1
        spaces = max(1, round(gap / char_w)) if gap > char_w * 0.35 else 0
        text += " " * spaces + o.text
        prev_x1 = o.x1
    return row[0].x0, text


def rebuild(obs: list[Obs]) -> str:
    if not obs:
        return ""
    char_w = estimate_char_width(obs)
    rows = cluster_rows(obs)
    row_h = statistics.median(o.h for o in obs)
    pitches = [rows[i + 1][0].yc - rows[i][0].yc for i in range(len(rows) - 1)]
    pitch = statistics.median(pitches) if pitches else row_h * 1.4
    base_x = min(r[0].x0 for r in rows)
    out: list[str] = []
    prev_yc: float | None = None
    for row in rows:
        x0, text = rebuild_row(row, char_w)
        if prev_yc is not None:
            steps = round((row[0].yc - prev_yc) / pitch) - 1
            out.extend([""] * max(0, steps))
        indent = " " * max(0, round((x0 - base_x) / char_w))
        out.append((indent + text).rstrip())
        prev_yc = row[0].yc
    return "\n".join(out)


def main() -> int:
    path = Path(sys.argv[1]) if len(sys.argv) > 1 else None
    payload = json.load(sys.stdin) if path is None else json.loads(path.read_text())
    print(rebuild(parse(payload)))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
