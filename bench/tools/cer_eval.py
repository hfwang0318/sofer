#!/usr/bin/env python3
"""Independent scorer for the ocr4code benchmark (do not reuse bench/score.py).

metrics
  content_acc : 1 - CER with whitespace removed  (text fidelity, layout-agnostic)
  indent_exact: share of matched lines whose recovered leading spaces are exact
  indent_cov  : share of truth lines that could be matched at all
  rows_ratio  : reconstructed rows / truth rows
"""
import json
import sys
from collections import defaultdict

try:
    from rapidfuzz.distance import Levenshtein as RF
except Exception:  # pragma: no cover
    RF = None


def lev(a, b):
    if RF:
        return RF.distance(a, b)
    if a == b:
        return 0
    if not a:
        return len(b)
    if not b:
        return len(a)
    prev = list(range(len(b) + 1))
    for i, ca in enumerate(a, 1):
        cur = [i]
        for j, cb in enumerate(b, 1):
            cur.append(min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (ca != cb)))
        prev = cur
    return prev[-1]


def nows(s):
    return "".join(s.split())


def indent_of(s):
    return len(s) - len(s.lstrip(" "))


def fit_advance(items):
    """Cell width: median ink-width of longer lines, refined by lattice fit."""
    ratios = [(x1 - x0) / len(t) for t, x0, x1 in items if len(t) >= 10 and x1 > x0]
    if not ratios:
        return 8.0
    ratios.sort()
    prior = ratios[len(ratios) // 2]
    xs = sorted({round(x0, 1) for _, x0, _ in items})
    best, cost = prior, 1e18
    step = prior * 0.85
    while step <= prior * 1.15:
        c = sum((x / step - round(x / step)) ** 2 for x in xs) / len(xs)
        if c < cost - 1e-12:
            best, cost = step, c
        step += 0.02
    return best


def rebuild(items):
    """items: [(text, [x0,y0,x1,y1])] -> rows with reconstructed spaces."""
    items = [(t.strip(), b) for t, b in items if t and t.strip()]
    if not items:
        return []
    left = min(b[0] for _, b in items)
    adv = fit_advance([(t, b[0], b[2]) for t, b in items])
    hs = sorted(b[3] - b[1] for _, b in items)
    lh = hs[len(hs) // 2] or 1.0
    items.sort(key=lambda it: (it[1][1], it[1][0]))
    rows, cur = [], []
    for it in items:
        if not cur:
            cur = [it]
            continue
        cy = sum((b[1] + b[3]) / 2 for _, b in cur) / len(cur)
        iy = (it[1][1] + it[1][3]) / 2
        if abs(iy - cy) <= 0.55 * lh:
            cur.append(it)
        else:
            rows.append(cur)
            cur = [it]
    if cur:
        rows.append(cur)
    out = []
    for row in rows:
        row.sort(key=lambda it: it[1][0])
        s = ""
        for t, b in row:
            col = max(0, int(round((b[0] - left) / adv)))
            if len(s) < col:
                s += " " * (col - len(s))
            elif s and not s.endswith(" "):
                s += " "
            s += t
        out.append(s)
    return out


SYMS = set("(){}[]<>|;:=+-*/&^%$#@!~`\\\"'.,<>")


def sym_recall(t, p):
    """Multiset recall over code-critical symbols: catches '->' -> '>' style losses."""
    from collections import Counter
    ct = Counter(c for c in t if c in SYMS)
    cp = Counter(c for c in p if c in SYMS)
    if not ct:
        return 1.0
    return sum(min(n, cp.get(c, 0)) for c, n in ct.items()) / sum(ct.values())


BOX = set(chr(c) for c in range(0x2500, 0x2580))


def split_box(t):
    """Separate box-drawing characters from real text: they carry structure, not
    content, and a model that drops them has not "misread" the words."""
    text = "".join(c for c in t if c not in BOX)
    box = "".join(c for c in t if c in BOX)
    return text, box


def evaluate(truth_path, pred_path):
    truth = [l.rstrip() for l in open(truth_path, encoding="utf-8").read().split("\n")]
    while truth and not truth[-1]:
        truth.pop()
    d = json.load(open(pred_path))
    lines = d.get("lines") or []
    raw = d.get("text")
    if raw is None:
        raw = "\n".join(l["text"] for l in lines)
    pred = [l.rstrip() for l in raw.split("\n")]
    while pred and not pred[-1]:
        pred.pop()

    t, p = "\n".join(truth), "\n".join(pred)
    cer_nows = lev(nows(t), nows(p)) / max(1, len(nows(t)))
    t_text, t_box = split_box(nows(t))
    p_text, p_box = split_box(nows(p))
    cer_text = lev(t_text, p_text)
    text_acc = 1 - cer_text / max(1, len(t_text))
    box_recall = (sum(min(t_box.count(c), p_box.count(c)) for c in set(t_box)) / len(t_box)) if t_box else 1.0

    symr = sym_recall(t, p)
    has_box = bool(lines) and all(len(l.get("bbox", [])) == 4 for l in lines)
    rows = rebuild([(l["text"], l["bbox"]) for l in lines]) if has_box else pred

    pool = defaultdict(list)
    for r in rows:
        pool[nows(r)].append(indent_of(r))
    hit = seen = 0
    for ln in truth:
        k = nows(ln)
        if not k or k not in pool or not pool[k]:
            continue
        got = pool[k].pop(0)
        seen += 1
        hit += got == indent_of(ln)
    nonempty = sum(1 for x in truth if x.strip())
    return {
        "content_acc": round(1 - cer_nows, 4),
        "text_acc": round(text_acc, 4),
        "box_recall": round(box_recall, 4),
        "sym_recall": round(symr, 4),
        "indent_exact": round(hit / max(1, seen), 3),
        "indent_cov": round(seen / max(1, nonempty), 2),
        "rows_ratio": round(len(rows) / max(1, len(truth)), 2),
        "ms": round(d.get("ms", -1)),
        "peak_mb": d.get("peak_mb"),
        "boxes": has_box,
    }


if __name__ == "__main__":
    print(json.dumps(evaluate(sys.argv[1], sys.argv[2]), ensure_ascii=False))
