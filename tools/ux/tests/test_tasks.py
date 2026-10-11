"""The yaml loader and tasks.yaml itself."""
import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
import check  # noqa: E402
import fleetrun  # noqa: E402
import miniyaml  # noqa: E402

SAMPLE = '''# comment
- id: a
  goal: "Watch it: now, # not a comment"
  needs_admin: false
  n: 3
  check: {name: playlist, args: {name: Weekend, contains: ["Big Buck Bunny", Sintel]}}
  route_desk:
    - key Up
    - type hello world
- id: b
  goal: plain words here
  needs_admin: true
  route_desk:
    - wait 500
'''


class MiniYaml(unittest.TestCase):
    def test_sample(self):
        a, b = miniyaml.loads(SAMPLE)
        self.assertEqual(a["goal"], "Watch it: now, # not a comment")
        self.assertIs(a["needs_admin"], False)
        self.assertEqual(a["n"], 3)
        self.assertEqual(a["check"]["args"]["contains"], ["Big Buck Bunny", "Sintel"])
        self.assertEqual(a["route_desk"], ["key Up", "type hello world"])
        self.assertEqual(b["goal"], "plain words here")
        self.assertIs(b["needs_admin"], True)

    def test_errors_name_the_line(self):
        with self.assertRaises(miniyaml.YamlError) as e:
            miniyaml.loads("- id: a\n  oops\n")
        self.assertIn("line 2", str(e.exception))
        with self.assertRaises(miniyaml.YamlError):
            miniyaml.loads("id: a\n")


class TasksFile(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tasks = fleetrun.load_tasks()

    def test_about_thirty_in_every_kind(self):
        self.assertGreaterEqual(len(self.tasks), 28)
        self.assertEqual({t["kind"] for t in self.tasks}, {"everyday", "occasional", "admin"})
        self.assertEqual({t["id"] for t in self.tasks if t["needs_admin"]}, {t["id"] for t in self.tasks if t["kind"] == "admin"})

    def test_every_check_is_known_and_has_its_args(self):
        for t in self.tasks:
            for c in (t["check"], t.get("also")):
                if c:
                    self.assertIn(c["name"], check.CHECKS, t["id"])

    def test_goals_do_not_name_screens_or_the_app(self):
        for t in self.tasks:
            low = t["goal"].lower()
            for w in ("jellyfin", "rustfin", "button", "menu", "tab ", "click", "settings screen"):
                self.assertNotIn(w, low, t["id"])

    def test_tv_routes_use_only_remote_buttons(self):
        for t in self.tasks:
            # text goes in with the on-screen keyboard (`osk <text>` lines): remote presses only, never `type`
            for c in fleetrun.expand(t, "abc123")["route_tv"]:  # osk lines become key presses
                self.assertTrue(fleetrun.tv_allowed(c), f"{t['id']}: {c}")

    def test_seed_specs(self):
        for t in self.tasks:
            if t.get("seed"):
                s = t["seed"]
                self.assertTrue(s.get("title") or s.get("series"), t["id"])
                self.assertTrue(s.get("seconds") or s.get("played"), t["id"])

    def test_expand_replaces_uniq_everywhere(self):
        t = next(t for t in self.tasks if t["id"] == "admin-add-user")
        e = fleetrun.expand(t, "ab12cd")
        self.assertIn("guest-ab12cd", e["goal"])
        self.assertEqual(e["check"]["args"]["name"], "guest-ab12cd")
        self.assertIn("type guest-ab12cd", e["route_desk"])

    def test_ideal_counts_repeated_presses(self):
        self.assertEqual(fleetrun.ideal_steps(["key Up", "key Right 3", "type x", "wait 500"]), 6)


if __name__ == "__main__":
    unittest.main()
