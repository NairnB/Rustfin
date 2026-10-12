"""Pure parts of the runner: tester output parsing, plan gate, signal rule, summary."""
import json
import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
import fleetrun as F  # noqa: E402

FINAL = {"result": "done", "steps": 7, "answer": "", "notes": "search was hidden",
         "screens": [{"screen": "Home", "score": 4, "why": "clear"}, {"screen": "Film page", "score": 2, "why": "tiny text"}]}
STREAM = "\n".join(json.dumps(e) for e in [
    {"type": "system", "subtype": "init"},
    {"type": "assistant", "message": {"content": [{"type": "text", "text": "I expect a search box."},
                                                  {"type": "tool_use", "name": "Bash", "input": {"command": "/x/drive s key Up"}}]}},
    {"type": "user", "message": {"content": [{"type": "tool_result", "content": "step 1: ok — screenshot: /x/001.jpg"}]}},
    {"type": "assistant", "message": {"content": [{"type": "text", "text": "Done.\n" + json.dumps(FINAL)}]}},
    {"type": "result", "result": "Done.\n" + json.dumps(FINAL)},
])


class Parsing(unittest.TestCase):
    def test_stream_to_transcript_and_final_json(self):
        text, last = F.parse_stream(STREAM)
        self.assertIn("TESTER: I expect a search box.", text)
        self.assertIn("> Bash: /x/drive s key Up", text)
        self.assertIn("< step 1: ok", text)
        self.assertEqual(F.final_json(last)["steps"], 7)

    def test_plain_text_final_line(self):
        _, last = F.parse_stream("hello\n" + json.dumps(FINAL))
        self.assertEqual(F.final_json(last)["result"], "done")

    def test_no_json(self):
        self.assertIsNone(F.final_json("I got lost"))
        self.assertIsNone(F.final_json('{"result": broken'))

    def test_scores(self):
        self.assertEqual(F.scores_of(FINAL), [("Home", 4, "clear"), ("Film page", 2, "tiny text")])
        self.assertEqual(F.scores_of({"screens": [{"screen": "x"}, {"screen": "y", "score": "high"}]}), [])
        self.assertEqual(F.scores_of(None), [])


class Plan(unittest.TestCase):
    def test_percent_takes_the_higher_of_five_hour_and_week(self):
        line = "main  max/default_claude_max_20x   5h  16% (resets 19:50 UTC) · week  93% (resets 2026-10-12) · caps 98/98 · ok"
        self.assertEqual(F.plan_percent(line), 93)
        self.assertEqual(F.plan_percent("main x 5h  98% (r) · week  10%"), 98)
        self.assertIsNone(F.plan_percent("main UNKNOWN (error)"))

    def test_full_at_97(self):
        F.PLAN_CMD = "echo 'b 5h 1% week 99%'; echo 'main x 5h 10% · week  97% (r)'"
        self.assertTrue(F.plan_full())
        F.PLAN_CMD = "echo 'main x 5h 10% · week  96% (r)'"
        self.assertFalse(F.plan_full())
        F.PLAN_CMD = "false"
        self.assertFalse(F.plan_full())  # unknown is never treated as spent


def rec(**k):
    r = {"task": "t", "mode": "desk", "kind": "everyday", "run": 1, "session": "s", "ideal": 4, "steps": 4, "ratio": 1.0,
         "check": {"pass": True, "reason": "ok"}, "scores": [{"screen": "Home", "score": 4, "why": "ok"}], "transcript": "/t.txt",
         "shots": "/shots"}
    r.update(k)
    return r


class Signal(unittest.TestCase):
    def test_rerun_triggers(self):
        self.assertFalse(F.has_signal(rec()))
        self.assertTrue(F.has_signal(rec(check={"pass": False, "reason": "x"})))
        self.assertTrue(F.has_signal(rec(steps=7, ratio=1.75)))
        self.assertFalse(F.has_signal(rec(steps=6, ratio=1.5)))
        self.assertTrue(F.has_signal(rec(scores=[{"screen": "a", "score": 2, "why": "w"}])))
        self.assertFalse(F.has_signal(rec(skipped="plan")))


class Summary(unittest.TestCase):
    def test_summary_numbers_and_failures(self):
        rs = [rec(ratio_cmd=0.75, commands=3), rec(task="u", check={"pass": False, "reason": "wrong film"}, ratio=2.5, steps=10, commands=8, ratio_cmd=2.0,
                                  scores=[{"screen": "Home", "score": 1, "why": "dark"}]),
              rec(mode="tv", ratio=2.0, steps=8, commands=6, ratio_cmd=1.5)]
        md = F.summarize(rs, 1, ["note one"])
        self.assertIn("note one", md)
        self.assertIn("| desk | 2 | 1 | 50 | 50 | 1.0 | 0.75 |", md)
        self.assertIn("| tv | 1 | 1 | 100 | 100 | 2.0 | 1.5 |", md)
        self.assertIn("10 presses vs ideal 4 (x2.5; 8 commands, x2.0 the rounds 1-2 way)", md)
        self.assertIn("| home | 3 | 3.0 | 1 |", md)
        self.assertIn("**u** / desk (run 1): check failed: wrong film", md)
        self.assertIn("[transcript](/t.txt)", md)

    def test_skipped_runs_are_called_out(self):
        md = F.summarize([rec(), rec(skipped="plan at or above 97%: no new testers")], 2)
        self.assertIn("1 runs not started", md)


class Prompt(unittest.TestCase):
    def test_prompt_has_goal_and_no_app_name(self):
        t = {"goal": "Watch the film Sintel."}
        for mode in ("desk", "tv"):
            p = F.tester_prompt(t, mode, "r1-x-desk-1")
            self.assertIn("Watch the film Sintel.", p)
            self.assertIn("r1-x-desk-1", p)
            for w in ("jellyfin", "rustfin", "repo"):
                self.assertNotIn(w, p.lower())
        self.assertNotIn("click", F.tester_prompt(t, "tv", "s"))

    def test_prompt_states_the_budget_and_the_wait_cap(self):
        short = {"goal": "g", "route_tv": ["key Enter"] * 5}
        long = {"goal": "g", "route_tv": ["key Down"] * 57, "route_desk": ["click 1 1"]}
        self.assertEqual(F.budget(short, "tv"), 40)
        self.assertEqual(F.budget(long, "tv"), 114)
        self.assertEqual(F.budget(long, "desk"), 40)
        p = F.tester_prompt(long, "tv", "s")
        self.assertIn("You have 114 commands", p)
        self.assertNotIn("after 40 commands", p)
        for mode in ("desk", "tv"):
            self.assertIn("10 seconds", F.tester_prompt(short, mode, "s"))

    def test_tester_is_locked_down(self):
        a = F.tester_argv("s1")
        self.assertIn("--safe-mode", a)
        self.assertEqual(a[a.index("--permission-mode") + 1], "dontAsk")
        self.assertIn("Bash(" + os.path.join(F.FLEET_HOME, "drive") + ":*)", a)
        self.assertIn("Edit Write WebFetch WebSearch Glob Grep Task", a)
        self.assertNotIn("jellyfin", " ".join(a))


class StartOrder(unittest.TestCase):
    def test_history_is_seeded_before_the_app_starts(self):
        calls = []
        class Acc:
            name, pw = "fleet-s", "p"
            def __init__(self, session, admin): pass
        saved = (F.Account, F.seed, F.sh, F.out_lines, F.jfapi.find_user, F.device_id_of)
        try:
            F.Account = Acc
            F.seed = lambda api, acc, spec: calls.append("seed")
            F.sh = lambda args, **kw: calls.append(args[1]) or type("R", (), {"returncode": 0, "stdout": "", "stderr": ""})()
            F.out_lines = lambda s: 0
            F.jfapi.find_user = lambda api, name: {"Id": "u"}
            F.device_id_of = lambda api, uid: "d"
            F.start_session(None, {"needs_admin": False, "seed": {"title": "Sintel", "seconds": 30}}, "desk", "s")
        finally:
            F.Account, F.seed, F.sh, F.out_lines, F.jfapi.find_user, F.device_id_of = saved
            F.ACTIVE.pop("s", None)
        self.assertEqual(calls, ["seed", "start"])

    def test_help_tells_the_truth_about_scroll(self):
        self.assertIn("negative", F.DESK_HELP)

    def test_exclusive_tasks_do_not_overlap(self):
        import threading, time
        live, worst = [0], [0]
        def body(api, task, *a):
            live[0] += 1
            worst[0] = max(worst[0], live[0])
            time.sleep(0.05)
            live[0] -= 1
        f = F._exclusive(body)
        ts = [threading.Thread(target=f, args=(None, {"exclusive": "playlists"})) for _ in range(4)]
        [t.start() for t in ts]
        [t.join() for t in ts]
        self.assertEqual(worst[0], 1)
        worst[0] = 0
        ts = [threading.Thread(target=f, args=(None, {})) for _ in range(4)]
        [t.start() for t in ts]
        [t.join() for t in ts]
        self.assertGreater(worst[0], 1)


if __name__ == "__main__":
    unittest.main()


class Counting(unittest.TestCase):
    def test_presses_come_from_the_command_log_after_the_baseline(self):
        import tempfile
        saved = F.FLEET_HOME
        try:
            F.FLEET_HOME = tempfile.mkdtemp()
            os.makedirs(os.path.join(F.FLEET_HOME, "s"))
            with open(os.path.join(F.FLEET_HOME, "s", "cmds"), "w") as f:
                f.write("wait 100\nkey Down 3\nkey Enter\ntype hello there\n")
            sent = F.tester_commands("s", 1)
            self.assertEqual(sent, ["key Down 3", "key Enter", "type hello there"])
            self.assertEqual(F.ideal_steps(sent), 5)  # counted like the ideal: `key X n` = n
            self.assertIsNone(F.tester_commands("nope", 0))
        finally:
            F.FLEET_HOME = saved


class Seeding(unittest.TestCase):
    def test_resume_seed_must_sit_inside_the_servers_window(self):
        sintel = {"Name": "Sintel", "RunTimeTicks": 630 * F.checks.TICKS}
        F.check_resumable(sintel, 250, (5, 90, 300))
        with self.assertRaises(RuntimeError):
            F.check_resumable(sintel, 1200, (5, 90, 300))  # past the end: 20 min does not fit a 10 min film
        with self.assertRaises(RuntimeError):
            F.check_resumable(sintel, 20, (5, 90, 300))  # under 5 %: the server would not offer to resume
        with self.assertRaises(RuntimeError):
            F.check_resumable({"Name": "short", "RunTimeTicks": 200 * F.checks.TICKS}, 80, (5, 90, 300))

    def test_a_list_seeds_each_spec(self):
        got = []
        saved = F.jfapi.Api.login, F.checks.find_item
        class Me:
            user_id = "u"
            def post(self, path, body=None): got.append(path)
        try:
            F.jfapi.Api.login = classmethod(lambda cls, b, n, p: Me())
            F.checks.find_item = lambda api, uid, title=None, series=None, nth=None: {"Id": title}
            acc = type("A", (), {"name": "fleet-s", "pw": "p"})()
            F.seed(type("Api", (), {"base": "b"})(), acc, [{"title": "A", "played": True}, {"title": "B", "played": True}])
        finally:
            F.jfapi.Api.login, F.checks.find_item = saved
        self.assertEqual(got, ["/UserPlayedItems/A?userId=u", "/UserPlayedItems/B?userId=u"])

