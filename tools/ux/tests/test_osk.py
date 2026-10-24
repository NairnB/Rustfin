"""osk.py: route generator for the TV on-screen keyboard (replays the keyboard's own navigation rules)."""
import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
import osk  # noqa: E402


def replay(keys, symbols=False):
    """A second, literal model of KeyboardState::press (row/col/x, nearest column) typing what the keys select."""
    row, col, x, typed = 0, 1, 2.0, ""
    for k in keys:
        rows = osk._rows(symbols)
        if k == "Left":
            col = max(col - 1, 0); x = osk._centre(rows[row], col)
        elif k == "Right":
            col = min(col + 1, len(rows[row]) - 1); x = osk._centre(rows[row], col)
        elif k == "Up" and row > 0:
            row -= 1; col = osk._nearest(rows[row], x)
        elif k == "Down" and row + 1 < len(rows):
            row += 1; col = osk._nearest(rows[row], x)
        elif k == "Enter":
            cap = rows[row][col][0]
            if cap == "layer":
                symbols = not symbols
            elif cap == "space":
                typed += " "
            elif cap == "delete":
                typed = typed[:-1]
            elif cap == "clear":
                typed = ""
            elif cap not in ("shift", "done"):
                typed += cap
    return typed


class Osk(unittest.TestCase):
    def test_typing_spells_the_text(self):
        for text in ("weekend", "spa", "viewer", "view-pass-1", "guest-3fa9c1", "hello-guest1", "a.b_c@d"):
            ks = osk.presses(text)
            self.assertEqual(ks[-1], "Back")
            self.assertEqual(replay(ks[:-1]), text, text)

    def test_clear_first(self):
        self.assertEqual(replay(osk.presses("ab", clear=True)[:-1]), "ab")

    def test_short_words_are_short(self):
        self.assertEqual(osk.presses("a"), ["Enter", "Back"])  # the keyboard opens on "a"
        self.assertLess(len(osk.presses("spa")), 30)

    def test_no_run_of_more_than_four_arrows_in_one_line(self):
        for line in osk.commands("hello-guest1"):
            parts = line.split()
            self.assertTrue(len(parts) < 3 or int(parts[2]) <= 4, line)

    def test_a_capital_is_refused(self):
        with self.assertRaises(ValueError):
            osk.presses("Weekend")

    def test_route_lines_expand(self):
        r = osk.expand_route(["key Enter", "osk a", "key Down"])
        self.assertEqual(r, ["key Enter", "key Enter", "key Back", "key Down"])


if __name__ == "__main__":
    unittest.main()
