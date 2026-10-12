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
                for s in t["seed"] if isinstance(t["seed"], list) else [t["seed"]]:
                    self.assertTrue(s.get("title") or s.get("series"), t["id"])
                    self.assertTrue(s.get("seconds") or s.get("played"), t["id"])

    def test_resume_seeds_read_as_part_way_and_the_check_still_proves_resuming(self):
        for tid in ("resume-show", "resume-film"):
            t = next(t for t in self.tasks if t["id"] == tid)
            sec, want = t["seed"]["seconds"], t["check"]["args"]["min_position_s"]
            self.assertGreaterEqual(sec, 240, tid)           # minutes in, not a 0:30 nudge
            self.assertTrue(sec - 2 <= want < sec, tid)       # the same 2 s slack as before: resumed, not restarted

    def test_filter_unwatched_has_watched_films_to_hide(self):
        t = next(t for t in self.tasks if t["id"] == "filter-unwatched")
        self.assertEqual(sum(1 for s in t["seed"] if s.get("played")), 2)
        self.assertEqual(t["check"]["args"], {"path": "library_view.*.filters.unplayed", "equals": True})

    def test_episode_goals_name_the_episode_not_the_first(self):
        # the server's season starts part way in (its first episode is not E1): the goal names the episode instead
        for tid in ("resume-show", "next-episode"):
            g = next(t for t in self.tasks if t["id"] == tid)["goal"]
            self.assertRegex(g, r"(watching|finished) [A-Z][\w' ]+, an episode of the TV show", tid)
            self.assertNotIn("first episode", g, tid)

    def test_playing_goals_say_start_watching(self):
        for t in self.tasks:
            if t["check"]["name"] == "now_playing":
                self.assertNotRegex(t["goal"], r"(?i)(^|[.,] )(find and )?watch ", t["id"])

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
