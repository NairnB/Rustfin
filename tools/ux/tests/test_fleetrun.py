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



# ---------------------------------------------------------------- FIX-3: a busy host can't lose a round or fail a task
class _Patch:
    """Swap module attributes for one test and put them back."""
    def __init__(self, **kw):
        self.kw, self.saved = kw, {}

    def __enter__(self):
        for k, v in self.kw.items():
            self.saved[k] = getattr(F, k)
            setattr(F, k, v)

    def __exit__(self, *a):
        for k, v in self.saved.items():
            setattr(F, k, v)


def _task(i):
    return {"id": i, "kind": "everyday", "route_desk": ["click 1 1"], "route_tv": ["key Enter"]}


def _fake_test_one(calls, fail=None):
    def fn(api, t, m, rnd, k, stop):
        calls.append((t["id"], m, k))
        bad = fail(t["id"], m, k, calls) if fail else False
        r = rec(task=t["id"], mode=m, run=k, session=f"s-{t['id']}-{m}-{k}")
        if bad == "signal":
            r["check"] = {"pass": False, "reason": "wrong"}
        elif bad == "rig":
            r.update(rig_error=True, check={"pass": False, "reason": "rig: no app"})
        return r
    return fn


def _round(tasks, rdir, resume=False, repeat=True, fn=None, modes=("desk",)):
    import threading
    notes = []
    with _Patch(test_one=fn):
        recs = F.execute_round(None, [(t, m) for t in tasks for m in modes], 9, rdir, repeat, threading.Event(), resume, notes)
    return recs


def _lines(rdir):
    return [json.loads(l) for l in open(os.path.join(rdir, "results.jsonl"))]


class Robust(unittest.TestCase):
    def setUp(self):
        import tempfile
        self.rdir = tempfile.mkdtemp()

    def _seed_results(self, recs):
        with open(os.path.join(self.rdir, "results.jsonl"), "w") as f:
            for r in recs:
                f.write(json.dumps(r) + "\n")

    def test_resume_skips_recorded_runs(self):
        self._seed_results([rec(task="a", mode="desk", run=1), rec(task="b", mode="desk", run=1)])
        calls = []
        _round([_task("a"), _task("b"), _task("c")], self.rdir, resume=True, repeat=False, fn=_fake_test_one(calls))
        self.assertEqual(calls, [("c", "desk", 1)])
        self.assertEqual(len(_lines(self.rdir)), 3)  # kept the old two, added one

    def test_without_resume_the_file_starts_empty(self):
        self._seed_results([rec(task="a", mode="desk", run=1)])
        calls = []
        _round([_task("a")], self.rdir, repeat=False, fn=_fake_test_one(calls))
        self.assertEqual(calls, [("a", "desk", 1)])
        self.assertEqual(len(_lines(self.rdir)), 1)

    def test_resume_runs_repeat_pass_for_old_signals(self):
        self._seed_results([rec(task="a", mode="desk", run=1, check={"pass": False, "reason": "x"}), rec(task="b", mode="desk", run=1)])
        calls = []
        recs = _round([_task("a"), _task("b")], self.rdir, resume=True, repeat=True, fn=_fake_test_one(calls))
        self.assertEqual(sorted(calls), [("a", "desk", 2), ("a", "desk", 3)])
        self.assertEqual(len(recs), 4)

    def test_resume_does_not_redo_finished_repeats(self):
        self._seed_results([rec(task="a", mode="desk", run=1, check={"pass": False, "reason": "x"}), rec(task="a", mode="desk", run=2)])
        calls = []
        _round([_task("a")], self.rdir, resume=True, repeat=True, fn=_fake_test_one(calls))
        self.assertEqual(calls, [("a", "desk", 3)])

    def test_summary_written_after_each_pass(self):
        seen = []

        def fn(api, t, m, rnd, k, stop):
            if k == 2:  # the repeat pass starts: the first pass's summary must already be on disk
                seen.append(os.path.exists(os.path.join(self.rdir, "summary.md")))
            return _fake_test_one([], lambda *a: "signal" if a[2] == 1 else False)(api, t, m, rnd, k, stop)

        def crash(api, t, m, rnd, k, stop):
            if k == 3:
                raise KeyboardInterrupt
            return fn(api, t, m, rnd, k, stop)
        with self.assertRaises(KeyboardInterrupt):
            _round([_task("a")], self.rdir, fn=crash)
        self.assertEqual(seen, [True])
        self.assertIn("Fleet round 9 summary", open(os.path.join(self.rdir, "summary.md")).read())

    def test_rig_error_not_counted_and_requeued_once(self):
        # first try of a/desk/1 hits a rig error, the re-queue works; b/desk/1 always hits one
        def fail(tid, m, k, calls):
            if tid == "b" or calls.count((tid, m, k)) == 1:
                return "rig"
        calls = []
        recs = _round([_task("a"), _task("b")], self.rdir, repeat=True, fn=_fake_test_one(calls, fail))
        self.assertEqual(calls.count(("a", "desk", 1)), 2)
        self.assertEqual(calls.count(("b", "desk", 1)), 2)  # once more, not forever
        self.assertEqual([r for r in recs if r["task"] == "a" and not r.get("rig_error")][0]["run"], 1)
        self.assertEqual(len([r for r in recs if r["task"] == "b"]), 1)  # the re-run replaced the first record
        self.assertFalse(any(("b", "desk", k) in calls for k in (2, 3)))  # no repeat pass for a rig error
        md = F.summarize(recs, 9)
        self.assertIn("| desk | 1 | 1 | 100 |", md)  # b left out of runs and success %
        self.assertIn("1 runs hit a rig error", md)

    def test_summary_without_rig_errors_has_no_rig_line(self):
        self.assertNotIn("rig error", F.summarize([rec()], 1))

    def test_start_failure_deletes_user(self):
        deleted = []

        class Acc:
            name, pw = "fleet-s", "p"
            def __init__(self, session, admin): pass
            def delete(self): deleted.append(1)
        bad = type("R", (), {"returncode": 1, "stdout": "", "stderr": "overloaded"})()
        with _Patch(Account=Acc, seed=lambda *a: None, sh=lambda args, **kw: bad, RF_START_RETRY_WAIT=0):
            with self.assertRaises(RuntimeError):
                F.start_session(None, {"needs_admin": False}, "desk", "s-del")
        self.assertEqual(deleted, [1])
        self.assertNotIn("s-del", F.ACTIVE)

    def test_seed_failure_also_deletes_user(self):
        deleted = []

        class Acc:
            name, pw = "fleet-s", "p"
            def __init__(self, session, admin): pass
            def delete(self): deleted.append(1)

        def boom(*a):
            raise RuntimeError("seed: cannot find")
        with _Patch(Account=Acc, seed=boom):
            with self.assertRaises(RuntimeError):
                F.start_session(None, {"needs_admin": False, "seed": {"title": "x"}}, "desk", "s-seed")
        self.assertEqual(deleted, [1])

    def test_rf_drive_start_retried_once(self):
        starts, waits = [], []

        class Acc:
            name, pw = "fleet-s", "p"
            def __init__(self, session, admin): pass
            def delete(self): pass

        def fake_sh(args, **kw):
            starts.append(args[1])
            return type("R", (), {"returncode": 1 if len(starts) == 1 else 0, "stdout": "", "stderr": "busy"})()
        saved_find, saved_dev = F.jfapi.find_user, F.device_id_of
        F.jfapi.find_user = lambda api, n: {"Id": "u"}
        try:
            with _Patch(Account=Acc, seed=lambda *a: None, sh=fake_sh, out_lines=lambda s: 0, device_id_of=lambda a, u: "d",
                        RF_START_RETRY_WAIT=30, _sleep=waits.append):
                F.start_session(None, {"needs_admin": False}, "desk", "s-retry")
            self.assertEqual(starts, ["start", "start"])
            self.assertEqual(waits, [30])
            starts.clear(); waits.clear()
            fake_always = lambda args, **kw: starts.append(1) or type("R", (), {"returncode": 1, "stdout": "", "stderr": "busy"})()
            with _Patch(Account=Acc, seed=lambda *a: None, sh=fake_always, _sleep=waits.append):
                with self.assertRaises(F.RigError):
                    F.start_session(None, {"needs_admin": False}, "desk", "s-retry2")
            self.assertEqual(len(starts), 2)  # exactly one retry
        finally:
            F.jfapi.find_user, F.device_id_of = saved_find, saved_dev
            F.ACTIVE.pop("s-retry", None)

    def test_start_failure_in_test_one_is_a_rig_error_not_a_task_failure(self):
        import threading

        def boom(*a, **k):
            raise F.RigError("rf-drive start failed: busy")
        with _Patch(start_session=boom, end_session=lambda s, a: None, plan_full=lambda: False, wait_for_load=lambda: None):
            r = F.test_one(None, {"id": "x", "goal": "g", "kind": "everyday", "needs_admin": False, "route_desk": ["click 1 1"], "route_tv": ["key Enter"]},
                           "desk", 9, 1, threading.Event())
        self.assertTrue(r["rig_error"])
        self.assertFalse(r["check"]["pass"])
        self.assertTrue(r["check"]["reason"].startswith("rig: "))
        self.assertFalse(F.has_signal(r))

    def _acc(self):
        class Acc:
            name, pw = "fleet-s", "p"
            def __init__(self, session, admin): pass
            def delete(self): pass
        return Acc

    def test_start_timeout_is_retried_then_a_rig_error(self):
        import subprocess
        starts, waits = [], []

        def hang(args, **kw):
            starts.append(1)
            raise subprocess.TimeoutExpired(args, 600)
        with _Patch(Account=self._acc(), seed=lambda *a: None, sh=hang, _sleep=waits.append):
            with self.assertRaises(F.RigError):
                F.start_session(None, {"needs_admin": False}, "desk", "s-to")
        self.assertEqual(len(starts), 2)
        self.assertEqual(waits, [30])

    def test_app_fault_on_start_is_not_a_rig_error_and_not_retried(self):
        starts = []

        def stuck(args, **kw):
            starts.append(1)
            return type("R", (), {"returncode": 1, "stdout": "", "stderr": "sign-in stuck before home: x"})()
        with _Patch(Account=self._acc(), seed=lambda *a: None, sh=stuck, _sleep=lambda s: None):
            with self.assertRaises(F.AppStartError) as cm:
                F.start_session(None, {"needs_admin": False}, "desk", "s-app")
        self.assertNotIsInstance(cm.exception, F.RigError)
        self.assertEqual(len(starts), 1)

    def test_prelude_failed_because_app_never_started_is_a_rig_error_and_retried(self):
        starts = []

        def dead(args, **kw):
            starts.append(1)
            return type("R", (), {"returncode": 1, "stdout": "", "stderr": "rf-drive: app did not take the command (is the session running?)\nrf-drive: sign-in prelude failed (see x/app.log)"})()
        with _Patch(Account=self._acc(), seed=lambda *a: None, sh=dead, _sleep=lambda s: None):
            with self.assertRaises(F.RigError):
                F.start_session(None, {"needs_admin": False}, "desk", "s-dead")
        self.assertEqual(len(starts), 2)

    def test_no_answer_within_is_an_app_fault(self):
        r = type("R", (), {"returncode": 1, "stdout": "", "stderr": "rf-drive: no answer within 90s\nrf-drive: sign-in prelude failed"})()
        self.assertTrue(F.app_fault(r))

    def test_clear_session_keeps_the_failed_folder_and_kills_the_launcher(self):
        import tempfile
        home = tempfile.mkdtemp()
        os.makedirs(os.path.join(home, "s-keep", "shots"))
        open(os.path.join(home, "s-keep", "app.log"), "w").write("boom")
        calls = []

        def rec(args, **kw):
            calls.append(list(args))
            return type("R", (), {"returncode": 0, "stdout": "", "stderr": ""})()
        saved = F.subprocess.run
        F.subprocess.run = rec
        try:
            with _Patch(FLEET_HOME=home, sh=rec, _sleep=lambda s: None):
                F.clear_session("s-keep")
                os.makedirs(os.path.join(home, "s-keep"))
                F.clear_session("s-keep")
        finally:
            F.subprocess.run = saved
        self.assertFalse(os.path.exists(os.path.join(home, "s-keep")))
        self.assertEqual(open(os.path.join(home, "s-keep.failed1", "app.log")).read(), "boom")
        self.assertTrue(os.path.isdir(os.path.join(home, "s-keep.failed2")))
        self.assertTrue(any(c[0] == "pkill" and c[-1].endswith("s-keep/launch.sh") for c in calls))
        self.assertEqual(len([c for c in calls if c[:3] == ["docker", "rm", "-f"]]), 4)  # twice per clear: the launcher may be mid-start

    def test_last_start_timeout_clears_the_orphan_launcher(self):
        import subprocess
        import tempfile
        home = tempfile.mkdtemp()
        calls = []

        def hang(args, **kw):
            os.makedirs(os.path.join(home, "s-last"), exist_ok=True)  # rf-drive start made its folder + launcher, then timed out
            raise subprocess.TimeoutExpired(args, 600)

        def rec(args, **kw):
            calls.append(list(args))
            return type("R", (), {"returncode": 0, "stdout": "", "stderr": ""})()
        saved = F.subprocess.run
        F.subprocess.run = rec
        try:
            with _Patch(FLEET_HOME=home, Account=self._acc(), seed=lambda *a: None, sh=hang, _sleep=lambda s: None):
                with self.assertRaises(F.RigError):
                    F.start_session(None, {"needs_admin": False}, "desk", "s-last")
        finally:
            F.subprocess.run = saved
        self.assertFalse(os.path.exists(os.path.join(home, "s-last")))
        self.assertTrue(os.path.isdir(os.path.join(home, "s-last.failed2")))  # one per attempt: the last one was cleared too
        self.assertEqual(len([c for c in calls if c[0] == "pkill"]), 4)  # retry clear + final clear, two passes each

    def test_rig_error_record_points_at_the_renamed_folder_and_summary_names_it(self):
        import tempfile
        import threading
        home = tempfile.mkdtemp()
        os.makedirs(os.path.join(home, "r9-x-desk-1.failed1"))
        os.makedirs(os.path.join(home, "r9-x-desk-1.failed2"))

        def boom(*a, **k):
            raise F.RigError("rf-drive start failed: busy")
        with _Patch(FLEET_HOME=home, start_session=boom, end_session=lambda s, a: None, plan_full=lambda: False, wait_for_load=lambda: None):
            r = F.test_one(None, {"id": "x", "goal": "g", "kind": "everyday", "needs_admin": False, "route_desk": ["click 1 1"], "route_tv": ["key Enter"]},
                           "desk", 9, 1, threading.Event())
        self.assertEqual(r["shots"], os.path.join(home, "r9-x-desk-1.failed2", "shots"))
        self.assertIn("session r9-x-desk-1", F.summarize([r], 9))
        self.assertIn("app crash", F.summarize([r], 9))

    def test_retry_clears_the_dead_session_folder_first(self):
        import tempfile
        home = tempfile.mkdtemp()
        seen = []

        def fake_sh(args, **kw):
            if args[1] == "start":
                seen.append(os.path.exists(os.path.join(home, "s-clr")))
                os.makedirs(os.path.join(home, "s-clr"), exist_ok=True)  # the real driver makes it before the prelude
                return type("R", (), {"returncode": 1 if len(seen) == 1 else 0, "stdout": "", "stderr": "busy"})()
            return type("R", (), {"returncode": 0, "stdout": "", "stderr": ""})()
        saved_find = F.jfapi.find_user
        F.jfapi.find_user = lambda api, n: {"Id": "u"}
        try:
            with _Patch(Account=self._acc(), seed=lambda *a: None, sh=fake_sh, FLEET_HOME=home, out_lines=lambda s: 0,
                        device_id_of=lambda a, u: "d", _sleep=lambda s: None):
                F.start_session(None, {"needs_admin": False}, "desk", "s-clr")
        finally:
            F.jfapi.find_user = saved_find
            F.ACTIVE.pop("s-clr", None)
        self.assertEqual(seen, [False, False])  # the folder was gone before each start

    def test_truncated_last_line_does_not_stop_resume(self):
        import tempfile
        p = os.path.join(tempfile.mkdtemp(), "results.jsonl")
        with open(p, "w") as f:
            f.write(json.dumps({"task": "a", "mode": "desk", "run": 1}) + "\n" + '{"task": "b", "mo')
        self.assertEqual(list(F.load_results(p)), [("a", "desk", 1)])

    def test_load_guard_waits(self):
        loads = iter([20.0, 15.1, 9.0])
        waits = []
        with _Patch(_load1=lambda: next(loads), _sleep=waits.append):
            F.wait_for_load()
        self.assertEqual(waits, [15, 15])
        waits.clear()
        with _Patch(_load1=lambda: 3.0, _sleep=waits.append):
            F.wait_for_load()
        self.assertEqual(waits, [])

    def test_tasks_have_no_stale_known_gap(self):
        t = next(t for t in F.load_tasks() if t["id"] == "audio-51")
        self.assertNotIn("known_gap", t)
