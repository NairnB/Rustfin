"""The TV's on-screen keyboard as a route generator (core/src/ui/keyboard.rs, UXP5). Stdlib only.

A task route line `osk <text>` (or `osk-clear <text>`: press clear first) becomes the shortest remote presses that
type <text> on a freshly opened keyboard and close it with Back. Lower case, digits (on the letter layer) and the keyboard's symbols only
(a capital needs shift: not worth the presses; the checks compare names without case).
"""
import collections

LETTERS = ["1234567890", "abcdefghij", "klmnopqrst", "uvwxyz.-_@"]  # FIX-4: the digit row is on both layers
SYMBOLS = ["1234567890", ":/.-_@?&=+", "!#$%^*()~,", "[]{}<>|;'\""]
SIDE = [("shift", 1.5), ("layer", 1.5), ("clear", 1.5)]  # the left column of the rows under the digits (shift / 123 near "a")
BOTTOM = [("space", 4.5), ("delete", 3.5), ("done", 3.5)]
MAX_HELD = 4  # five or more repeated Left/Right inside 0.2 s make the real keyboard skip keys


def _rows(symbols):
    src = SYMBOLS if symbols else LETTERS
    digits = [[(c, 1.15) for c in src[0]]]  # the digit row has no side key: its 10 keys span the full width
    return digits + [[side] + [(c, 1.0) for c in r] for side, r in zip(SIDE, src[1:])] + [list(BOTTOM)]


def _centre(row, col):
    return sum(w for _, w in row[:col]) + row[col][1] / 2


def _nearest(row, x):
    return min(range(len(row)), key=lambda c: (abs(_centre(row, c) - x), c))


def _path(rows, start, target):
    """Fewest arrow presses from focus `start` = (row, col, x) to the key at `target` = (row, col)."""
    seen, queue = {start}, collections.deque([(start, [])])
    while queue:
        (r, c, x), keys = queue.popleft()
        if (r, c) == target:
            return keys, (r, c, x)
        moves = [("Left", (r, max(c - 1, 0), None)), ("Right", (r, min(c + 1, len(rows[r]) - 1), None))]
        if r > 0:
            moves.append(("Up", (r - 1, _nearest(rows[r - 1], x), x)))
        if r + 1 < len(rows):
            moves.append(("Down", (r + 1, _nearest(rows[r + 1], x), x)))
        for name, (nr, nc, nx) in moves:
            st = (nr, nc, _centre(rows[nr], nc) if nx is None else nx)
            if st not in seen:
                seen.add(st)
                queue.append((st, keys + [name]))
    raise ValueError("unreachable key")


def _find(rows, ch):
    for r, row in enumerate(rows):
        for c, (cap, _) in enumerate(row):
            if cap == ch:
                return r, c
    return None


def presses(text, clear=False):
    """The key names (Left/Right/Up/Down/Enter/Back) that type `text`, from a fresh keyboard, then close it."""
    symbols, focus, out = False, (1, 1, 2.0), []  # opens on "a" (row 0 is the digits; column 0 the side keys)

    def go(target, rows):
        nonlocal focus
        keys, focus = _path(rows, focus, target)
        out.extend(keys)
        out.append("Enter")

    if clear:
        go((3, 0), _rows(symbols))
    for ch in text:
        rows = _rows(symbols)
        spot = _find(rows, ch)
        if spot is None:  # on the other layer: press 123 / abc, then look again
            go((2, 0), rows)
            symbols = not symbols
            rows = _rows(symbols)
            spot = _find(rows, ch)
            if spot is None:
                raise ValueError(f"the keyboard has no key {ch!r}")
        go(spot, rows)
    out.append("Back")
    return out


def commands(text, clear=False):
    """`presses` as route lines: repeats of one arrow share a `key X n` line (n <= 4: see MAX_HELD)."""
    lines, run = [], []

    def flush():
        while run:
            n = min(len(run), MAX_HELD)
            lines.append(f"key {run[0]} {n}" if n > 1 else f"key {run[0]}")
            del run[:n]

    for k in presses(text, clear):
        if run and k != run[0]:
            flush()
        if k in ("Left", "Right"):
            run.append(k)
        else:
            flush()
            lines.append(f"key {k}")
    flush()
    return lines


def expand_route(route):
    """Replace every `osk <text>` / `osk-clear <text>` line of a route by its presses."""
    out = []
    for line in route:
        verb, _, rest = line.partition(" ")
        if verb == "osk":
            out.extend(commands(rest))
        elif verb == "osk-clear":
            out.extend(commands(rest, clear=True))
        else:
            out.append(line)
    return out
