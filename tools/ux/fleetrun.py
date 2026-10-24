#!/usr/bin/env python3
"""fleet-run — the Haiku first-time-user fleet (BRIEF-haiku-fleet §2). Stdlib only.

  fleet-run --round N [--tasks id,id] [--modes desk,tv] [--repeat-on-signal] [--resume] [--skip-validate] [--force-invalid]

1. validate: every selected route (route_desk / route_tv) is replayed through rf-drive with a fresh fleet user and the
   server check must pass -> <out>/routes-ok.json. Tasks whose route fails are not given to testers (--force-invalid).
2. test: per task x mode: fleet user -> rf-drive start -> seed -> tester (claude -p --model haiku) -> server check ->
   rf-drive stop -> delete the user. At most 8 sessions at once. --repeat-on-signal re-runs failures / slow / low-score
   tasks with 2 more fresh testers. --resume keeps <round>/results.jsonl and runs only what is not in it.
   A busy host cannot fail a task: no tester starts while the 1-minute load is above 14, a failed `rf-drive start` is
   retried once, and a run whose app never started is a `rig_error` (left out of the success %, re-queued once).
3. <out>/round-N/results.jsonl + summary.md (rewritten after every pass).
Env: RF_DRIVE (driver), RF_FLEET_HOME (sessions dir), TESTER_CMD (fake tester: <cmd> <drive> <session> <routefile>),
RF_PLAN_CMD (optional plan gate), RF_CLAUDE_CONFIG (optional, for testers), FLEET_OUT; also read from the file
${RF_ENV:-~/.config/rf-fleet.env}.
"""
import argparse
import concurrent.futures as cf
import hashlib
import json
import os
import re
import shlex
import shutil
import signal
import statistics
import subprocess
import sys
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)  # -I / script runs do not put the script dir on sys.path
import check as checks  # noqa: E402
import osk  # noqa: E402
import jfapi  # noqa: E402
import miniyaml  # noqa: E402

REPO = os.path.dirname(os.path.dirname(HERE))
jfapi.load_env()
FLEET_HOME = jfapi.fleet_home()
RF_DRIVE = os.environ.get("RF_DRIVE", os.path.join(HERE, "rf-drive"))
PLAN_CMD = os.environ.get("RF_PLAN_CMD")
MAX_SESSIONS = 8
PLAN_STOP = 97
TESTER_TIMEOUT = 1800
SLOW = 1.5
LOW_SCORE = 2
LOAD_MAX = float(os.environ.get("RF_LOAD_MAX", 14))  # no new tester while the 1-minute load average is above this (the build slots use the same limit)
LOAD_POLL = 15
CLEAR_RECHECK_WAIT = 3  # seconds before clear_session looks again for a launcher/container that was mid-start
RF_START_RETRY_WAIT = float(os.environ.get("RF_START_RETRY_WAIT", 30))
TV_KEYS = {"Up", "Down", "Left", "Right", "Enter", "Back", "Info", "PlayPause"}
MODES = ("desk", "tv")
DESK_HELP = """\
- click X Y        click at pixel X,Y of the screenshot (same coordinates as in the picture you see)
- move X Y         move the mouse pointer without clicking
- scroll DX DY     scroll the page under the pointer (DY negative = scroll down, e.g. scroll 0 -400; DX negative = scroll a sideways row to the right, e.g. scroll -400 0; move the pointer over the part to scroll first)
- key NAME [N]     press a key: Up Down Left Right Enter Back Esc (N = repeat count)
- type TEXT        type text into the field that is selected
- wait MS          wait for that many milliseconds (at most 10000, i.e. 10 seconds, per command)"""
TV_HELP = """\
You only have a TV remote: 
- key NAME [N]     press a remote button: Up Down Left Right Enter Back Info PlayPause (N = repeat count)
- wait MS          wait for that many milliseconds (at most 10000, i.e. 10 seconds, per command)"""
BUDGET_MIN = 40  # commands a tester gets before giving up; more when the shortest route itself is long


# ---------------------------------------------------------------- tasks
def load_tasks(path=os.path.join(HERE, "tasks.yaml")):
    with open(path) as f:
        tasks = miniyaml.loads(f.read())
    seen = set()
    for t in tasks:
        for f in ("id", "goal", "kind", "needs_admin", "check", "route_desk", "route_tv"):
            if f not in t:
                raise ValueError(f"task {t.get('id')!r}: missing {f}")
        if t["id"] in seen:
            raise ValueError(f"duplicate task id {t['id']}")
        seen.add(t["id"])
        if t["kind"] not in ("everyday", "occasional", "admin"):
            raise ValueError(f"task {t['id']}: bad kind {t['kind']!r}")
        if t["check"].get("name") not in checks.CHECKS:
            raise ValueError(f"task {t['id']}: unknown check {t['check'].get('name')!r}")
        for r in (t["route_desk"], t["route_tv"]):
            if not r:
                raise ValueError(f"task {t['id']}: empty route")
    return tasks


def expand(obj, uniq):
    """Replace {uniq} in every string of a task (goal, check args, routes)."""
    if isinstance(obj, str):
        return obj.replace("{uniq}", uniq)
    if isinstance(obj, list):
        return [expand(x, uniq) for x in obj]
    if isinstance(obj, dict):
        out = {k: expand(v, uniq) for k, v in obj.items()}
        if "route_tv" in out:  # `osk <text>` lines: the TV's on-screen keyboard presses, made after {uniq} is known
            out["route_tv"] = osk.expand_route(out["route_tv"])
        return out
    return obj


def ideal_steps(route):
    """Commands in a route; `key NAME N` counts N presses (a tester sends one press per step)."""
    n = 0
    for c in route:
        m = re.fullmatch(r"key \S+ (\d+)", c.strip())
        n += int(m.group(1)) if m else 1
    return n


def budget(task, mode):
    """Commands the tester may send before giving up: max(40, 2 x the commands in the task's (validated) route)."""
    return max(BUDGET_MIN, 2 * len(task.get("route_" + mode) or []))


def tv_allowed(cmd):
    """The TV remote only has these buttons (rf-drive enforces the same list)."""
    p = cmd.split()
    return bool(p) and (p[0] == "wait" or (p[0] == "key" and len(p) >= 2 and p[1] in TV_KEYS))


# ---------------------------------------------------------------- tester output
def parse_stream(text):
    """claude stream-json (or plain text) -> (readable transcript, last assistant text)."""
    out, last, plain = [], "", []
    for line in text.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            ev = json.loads(line)
        except ValueError:
            out.append(line)
            plain.append(line)
            continue
        if not isinstance(ev, dict) or "type" not in ev:  # plain text that happens to be JSON (the tester's last line)
            out.append(line)
            plain.append(line)
            continue
        if ev.get("type") == "assistant":
            for b in (ev.get("message") or {}).get("content", []):
                if b.get("type") == "text":
                    out.append("TESTER: " + b["text"])
                    last = b["text"]
                elif b.get("type") == "tool_use":
                    inp = b.get("input") or {}
                    out.append(f"  > {b.get('name')}: {inp.get('command') or inp.get('file_path') or inp}")
        elif ev.get("type") == "user":
            c = (ev.get("message") or {}).get("content")
            if isinstance(c, list):
                for b in c:
                    if b.get("type") == "tool_result":
                        r = b.get("content")
                        r = " ".join(x.get("text", "[image]") for x in r) if isinstance(r, list) else str(r)
                        out.append("  < " + r.strip()[:300])
        elif ev.get("type") == "result" and ev.get("result"):
            last = ev["result"]
    return "\n".join(out), last or "\n".join(plain)


def final_json(text):
    """The last JSON object in the tester's final text that has a "result" key."""
    for line in reversed(text.splitlines()):
        line = line.strip()
        i = line.find('{"result"')
        if i >= 0:
            try:
                return json.loads(line[i:])
            except ValueError:
                continue
    return None


def scores_of(tj):
    out = []
    for s in (tj or {}).get("screens") or []:
        try:
            out.append((str(s.get("screen", "?")), int(s["score"]), str(s.get("why", ""))))
        except (KeyError, ValueError, TypeError):
            pass
    return out


# ---------------------------------------------------------------- plan gate
def plan_percent(line):
    """Highest of the five-hour and week percentages on a `main ...` plan-usage line, or None."""
    vals = [float(x) for x in re.findall(r"(?:5h|week)\s+(\d+(?:\.\d+)?)%", line)]
    return max(vals) if vals else None


def plan_full():
    if not PLAN_CMD:
        return False
    try:
        out = subprocess.run(PLAN_CMD, shell=True, capture_output=True, text=True, timeout=60).stdout
    except Exception:
        return False  # unknown is never treated as spent
    for line in out.splitlines():
        if line.startswith("main"):
            p = plan_percent(line)
            return p is not None and p >= PLAN_STOP
    return False


# ---------------------------------------------------------------- load guard
def _load1():
    return os.getloadavg()[0]


def _sleep(s):
    time.sleep(s)


def wait_for_load():
    n = 0
    while (load := _load1()) > LOAD_MAX:
        if n % 20 == 0:
            print(f"waiting: load {load:.1f} > {LOAD_MAX}", flush=True)
        n += 1
        _sleep(LOAD_POLL)


# ---------------------------------------------------------------- rf-drive / accounts
class RigError(RuntimeError):
    """The harness could not get the app up (a busy host): not a failure of the task."""


class AppStartError(RuntimeError):
    """The app came up but never reached its first screen: that is an app fault, scored as a failed task."""


def sh(args, **kw):
    return subprocess.run(args, capture_output=True, text=True, **kw)


def drive_cmd(session, cmd):
    """One command through rf-drive (type keeps its text as a single argument)."""
    verb, _, rest = cmd.partition(" ")
    args = [RF_DRIVE, session, verb] + ([rest] if verb == "type" else shlex.split(rest))
    return sh(args, timeout=60)


def make_wrapper():
    """A neutrally named driver path for the tester (the repo path would name the project)."""
    os.makedirs(FLEET_HOME, exist_ok=True)
    p = os.path.join(FLEET_HOME, "drive")
    with open(p, "w") as f:
        f.write(f'#!/usr/bin/env bash\nexec {shlex.quote(RF_DRIVE)} "$@"\n')
    os.chmod(p, 0o755)
    return p


def out_lines(session):
    try:
        with open(os.path.join(FLEET_HOME, session, "out")) as f:
            return sum(1 for _ in f)
    except OSError:
        return 0


def tester_commands(session, base):
    """The commands the tester sent (rf-drive logs one line per step in `cmds`, in step with `out`), or None without a log."""
    try:
        with open(os.path.join(FLEET_HOME, session, "cmds")) as f:
            return [l.rstrip("\n") for l in f][base:]
    except OSError:
        return None


class Account:
    def __init__(self, session, admin):
        self.session, self.name, self.pw = session, f"fleet-{session}", None
        cmd = [os.path.join(HERE, "fleet-user.sh"), "create", session] + (["--admin"] if admin else [])
        r = sh(cmd, timeout=120)
        if r.returncode:
            raise RuntimeError("fleet-user create failed: " + (r.stderr or r.stdout).strip()[:300])
        self.pw = json.loads(r.stdout.strip().splitlines()[-1])["pass"]

    def delete(self):
        sh([os.path.join(HERE, "fleet-user.sh"), "delete", self.session], timeout=120)


ACTIVE = {}  # session -> Account, for cleanup on interrupt
ACTIVE_LOCK = threading.Lock()


def cleanup_active():
    with ACTIVE_LOCK:
        items = list(ACTIVE.items())
    for s, acc in items:
        sh([RF_DRIVE, "stop", s], timeout=60)
        acc.delete()


def resume_window(api):
    """The server's resume rules: (MinResumePct, MaxResumePct, MinResumeDurationSeconds), Jellyfin's defaults if unread."""
    try:
        c = api.get("/System/Configuration") or {}
    except jfapi.ApiError:
        c = {}
    return c.get("MinResumePct", 5), c.get("MaxResumePct", 90), c.get("MinResumeDurationSeconds", 300)


def check_resumable(item, seconds, window):
    """A seeded resume position the server would keep as one: inside its min/max percent, on media long enough to resume."""
    lo, hi, min_dur = window
    run = (item.get("RunTimeTicks") or 0) / checks.TICKS
    if not run:
        return
    if run < min_dur:
        raise RuntimeError(f"seed: {item.get('Name')!r} runs {run:.0f}s, under the server's {min_dur}s minimum to resume")
    pct = 100 * seconds / run
    if not lo <= pct <= hi:
        raise RuntimeError(f"seed: {seconds}s is {pct:.0f}% of {item.get('Name')!r} ({run:.0f}s), outside the server's {lo}-{hi}% resume window")


def seed(api, acc, spec):
    """Give the throwaway user history before the tester starts: a resume position (seconds) or `played`; a list of
    specs seeds each one."""
    if isinstance(spec, list):
        for s in spec:
            seed(api, acc, s)
        return
    me = jfapi.Api.login(api.base, acc.name, acc.pw)
    item = checks.find_item(me, me.user_id, title=spec.get("title"), series=spec.get("series"), nth=spec.get("nth"))
    if not item:
        raise RuntimeError(f"seed: cannot find {spec}")
    if spec.get("played"):
        for path in (f"/UserPlayedItems/{item['Id']}?userId={me.user_id}", f"/Users/{me.user_id}/PlayedItems/{item['Id']}"):
            try:
                me.post(path)
                break
            except jfapi.ApiError:
                continue
        else:
            raise RuntimeError("seed: cannot mark played")
    if spec.get("seconds"):
        check_resumable(item, spec["seconds"], resume_window(api))
        body = {"PlaybackPositionTicks": int(spec["seconds"] * checks.TICKS)}
        for path in (f"/UserItems/{item['Id']}/UserData?userId={me.user_id}", f"/Users/{me.user_id}/Items/{item['Id']}/UserData"):
            try:
                me.post(path, body)
                break
            except jfapi.ApiError:
                continue
        else:
            raise RuntimeError("seed: cannot store the resume position")


def device_id_of(api, uid):
    for s in api.get("/Sessions") or []:
        if s.get("UserId") == uid:
            return s.get("DeviceId")
    return None


def run_checks(api, acc, task, session, since, device_id, answer):
    u = jfapi.find_user(api, acc.name)
    if not u:
        return {"pass": False, "reason": f"user {acc.name} vanished"}
    ctx = checks.Ctx(since, device_id, os.path.join(FLEET_HOME, session), answer)
    for c in (task["check"], task.get("also")):
        if not c:
            continue
        r = checks.run_check(api, u["Id"], c["name"], c.get("args"), ctx)
        if not r["pass"]:
            return r
    return {"pass": True, "reason": "all checks passed"}


# ---------------------------------------------------------------- one session
def session_name(rnd, task_id, mode, k):
    return f"r{rnd}-{task_id}-{mode}-{k}"[:48]


def start_once(cmd):
    """One `rf-drive start`; None if it timed out (the driver waits for a free slot, so that is a busy host)."""
    try:
        return sh(cmd, timeout=600)
    except subprocess.TimeoutExpired:
        return None


APP_FAULT = ("sign-in stuck before", "no answer within")  # rf-drive's own words when the app is up but off course
NO_APP = "app did not take the command"  # rf-drive: nothing reads the app's pipe, so the app never started (a rig fault, even with "sign-in prelude failed" after it)


def app_fault(r):
    out = (r.stderr or "") + (r.stdout or "")
    return NO_APP not in out and any(m in out for m in APP_FAULT)


def start_detail(session, r):
    """The driver's message plus the tail of the app's log: an app fault must stay visible, not pass as rig noise."""
    tail = ""
    try:
        with open(os.path.join(FLEET_HOME, session, "app.log")) as f:
            tail = " | app.log: " + " / ".join(f.read().strip().splitlines()[-5:])
    except OSError:
        pass
    return (r.stderr or r.stdout).strip()[:300] + tail[:600]


def clear_session(session):
    """Stop a dead session and move its folder aside (kept for diagnosis) so rf-drive accepts the name again."""
    d = os.path.join(FLEET_HOME, session)
    if os.path.exists(d):
        try:
            sh([RF_DRIVE, "stop", session], timeout=60)
        except subprocess.TimeoutExpired:
            pass
        try:  # a timed-out start leaves its launch.sh waiting for a slot: it must not start an orphan container later
            for again in (False, True):  # twice: a launcher that had just exec'd its runner has no container to remove yet
                if again:
                    _sleep(CLEAR_RECHECK_WAIT)
                subprocess.run(["pkill", "-f", os.path.join(d, "launch.sh")], capture_output=True, timeout=30)
                subprocess.run(["docker", "rm", "-f", f"rfd-{session}"], capture_output=True, timeout=60)
        except (subprocess.TimeoutExpired, OSError):
            pass
        n = 1
        while os.path.exists(f"{d}.failed{n}"):
            n += 1
        try:
            os.rename(d, f"{d}.failed{n}")
        except OSError:
            shutil.rmtree(d, ignore_errors=True)


def failed_dir(session):
    """The newest renamed folder of a dead attempt (clear_session keeps it), or None."""
    d = os.path.join(FLEET_HOME, session)
    n = 0
    while os.path.exists(f"{d}.failed{n + 1}"):
        n += 1
    return f"{d}.failed{n}" if n else None


def start_session(api, task, mode, session):
    """Account + app + seed. Returns (account, device_id, baseline out lines). Caller stops + deletes in finally, but if
    anything fails in here the caller has no account yet, so the user made here is deleted before the error goes on."""
    acc = Account(session, task["needs_admin"])
    with ACTIVE_LOCK:
        ACTIVE[session] = acc
    try:
        if task.get("seed"):  # before the app starts: its Home is loaded once, so history made afterwards would not show
            seed(api, acc, task["seed"])
        cmd = [RF_DRIVE, "start", "--mode", mode, "--session", session, "--user", acc.name, "--pass", acc.pw]
        clear_session(session)  # rf-drive refuses a session whose folder exists (a resumed, re-queued or retried run)
        r = start_once(cmd)
        if r is None or r.returncode:  # a busy host: once more after a pause before calling it a rig error
            if r is not None and app_fault(r):
                raise AppStartError("the app did not reach its first screen: " + start_detail(session, r))
            _sleep(RF_START_RETRY_WAIT)
            clear_session(session)
            r = start_once(cmd)
        if r is None:
            raise RigError("rf-drive start timed out")
        if r.returncode:
            if app_fault(r):
                raise AppStartError("the app did not reach its first screen: " + start_detail(session, r))
            raise RigError("rf-drive start failed: " + (r.stderr or r.stdout).strip()[:300])
        base = out_lines(session)
        uid = jfapi.find_user(api, acc.name)["Id"]
        return acc, device_id_of(api, uid), base
    except BaseException as e:
        try:
            if isinstance(e, RigError):
                clear_session(session)  # the last start may have timed out: kill its launcher so no container appears later
            acc.delete()
        finally:
            with ACTIVE_LOCK:
                ACTIVE.pop(session, None)
        raise


def end_session(session, acc):
    try:
        sh([RF_DRIVE, "stop", session], timeout=60)
    except subprocess.TimeoutExpired:
        # A stuck stop must not kill the whole round (it did in round 1): remove the session's container.
        try:
            subprocess.run(["docker", "rm", "-f", f"rfd-{session}"], capture_output=True, timeout=60)
        except subprocess.TimeoutExpired:
            # A busy host can stall docker too (round 3): leave the container for the sweep, keep the round going.
            print(f"warning: rfd-{session} not removed (docker timed out); sweep it after the round", flush=True)
    finally:
        if acc:
            acc.delete()
        with ACTIVE_LOCK:
            ACTIVE.pop(session, None)


_EXCL = {}


def _exclusive(fn):
    """Tasks with `exclusive: <name>` never run at the same time (playlists are public: every user sees every playlist)."""
    def run(api, task0, *a, **kw):
        name = task0.get("exclusive")
        if not name:
            return fn(api, task0, *a, **kw)
        with _EXCL.setdefault(name, threading.Lock()):
            return fn(api, task0, *a, **kw)
    return run


@_exclusive
def validate_one(api, task0, mode, rnd):
    session = session_name(rnd, "v-" + task0["id"], mode, 0)
    task = expand(task0, hashlib.sha1(session.encode()).hexdigest()[:6])
    route = task["route_" + mode]
    acc = None
    try:
        if mode == "tv":
            bad = [c for c in route if not tv_allowed(c)]
            if bad:
                return {"ok": False, "reason": f"route uses commands a TV remote lacks: {bad[:3]}"}
        t0 = time.time()
        acc, dev, _ = start_session(api, task, mode, session)
        for c in route + ["wait 3000"]:
            r = drive_cmd(session, c)
            if r.returncode or " error" in (r.stdout + r.stderr).lower():
                return {"ok": False, "reason": f"{c!r} failed: {(r.stdout + r.stderr).strip()[:200]}"}
        res = run_checks(api, acc, task, session, t0, dev, "2006" if task["check"]["name"] == "answer_contains" else "")
        return {"ok": res["pass"], "reason": res["reason"], "steps": ideal_steps(route), "commands": len(route)}
    except Exception as e:  # a broken session is a failed route, not a crashed run
        return {"ok": False, "reason": f"{type(e).__name__}: {e}"}
    finally:
        end_session(session, acc)


def tester_argv(session):
    cfg = os.path.join(FLEET_HOME, "drive")
    return ["claude", "-p", "--model", "haiku", "--safe-mode", "--permission-mode", "dontAsk", "--no-session-persistence",
            "--output-format", "stream-json", "--verbose",
            "--allowedTools", f"Bash({cfg}:*)", f"Read(/{FLEET_HOME}/{session}/shots/**)",
            "--disallowedTools", "Edit Write WebFetch WebSearch Glob Grep Task"]


def tester_prompt(task, mode, session):
    with open(os.path.join(HERE, "tester-prompt.md")) as f:
        tpl = f.read()
    drive = os.path.join(FLEET_HOME, "drive")
    shots = os.path.join(FLEET_HOME, session, "shots")
    # The exact first screenshot: testers may not list folders (only `drive` and Read are allowed).
    jpgs = sorted(f for f in os.listdir(shots) if f.endswith(".jpg")) if os.path.isdir(shots) else []
    first_shot = os.path.join(shots, jpgs[-1]) if jpgs else shots
    return tpl.format(goal=task["goal"], session=session, drive=drive, shots=shots, first_shot=first_shot, commands=DESK_HELP if mode == "desk" else TV_HELP,
                      budget=budget(task, mode),
                      example="click 700 300" if mode == "desk" else "key Down")


def run_tester(task, mode, session):
    """Returns (transcript path, final json or None, last text, error or None)."""
    sdir = os.path.join(FLEET_HOME, session)
    cwd = os.path.join(sdir, "cwd")
    os.makedirs(cwd, exist_ok=True)
    raw = os.path.join(sdir, "transcript.jsonl")
    fake = os.environ.get("TESTER_CMD")
    err = None
    try:
        if fake:
            rf = os.path.join(sdir, "route.txt")
            with open(rf, "w") as f:
                f.write("\n".join(task["route_" + mode]) + "\n")
            p = subprocess.run(shlex.split(fake) + [os.path.join(FLEET_HOME, "drive"), session, rf], capture_output=True,
                               text=True, cwd=cwd, timeout=TESTER_TIMEOUT)
        else:
            env = dict(os.environ)
            if os.environ.get("RF_CLAUDE_CONFIG"):
                env["CLAUDE_CONFIG_DIR"] = os.environ["RF_CLAUDE_CONFIG"]
            p = subprocess.run(tester_argv(session), input=tester_prompt(task, mode, session), capture_output=True, text=True,
                               cwd=cwd, env=env, timeout=TESTER_TIMEOUT)
        text = p.stdout
        if p.returncode:
            err = f"tester exited {p.returncode}: {p.stderr.strip()[:200]}"
    except subprocess.TimeoutExpired as e:
        text = (e.stdout or b"").decode() if isinstance(e.stdout, bytes) else (e.stdout or "")
        err = "tester timed out"
    with open(raw, "w") as f:
        f.write(text)
    readable, last = parse_stream(text)
    tpath = os.path.join(sdir, "transcript.txt")
    with open(tpath, "w") as f:
        f.write(readable + "\n")
    return tpath, final_json(last) or final_json(text), last, err


@_exclusive
def test_one(api, task0, mode, rnd, k, stop_event):
    session = session_name(rnd, task0["id"], mode, k)
    uniq = hashlib.sha1(session.encode()).hexdigest()[:6]
    task = expand(task0, uniq)
    ideal = ideal_steps(task["route_" + mode])
    rec = {"task": task["id"], "mode": mode, "kind": task["kind"], "run": k, "session": session, "ideal": ideal,
           "ideal_commands": len(task["route_" + mode]), "budget": budget(task, mode),
           "shots": os.path.join(FLEET_HOME, session, "shots")}
    if stop_event.is_set() or plan_full():
        stop_event.set()
        return dict(rec, skipped="plan at or above %d%%: no new testers" % PLAN_STOP)
    acc = None
    try:
        wait_for_load()
        t0 = time.time()
        acc, dev, base = start_session(api, task, mode, session)
        tpath, tj, last, err = run_tester(task, mode, session)
        commands = max(out_lines(session) - base, 0)
        sent = tester_commands(session, base)
        steps = ideal_steps(sent) if sent is not None else commands  # presses, counted like the ideal (`key X n` = n)
        res = run_checks(api, acc, task, session, t0, dev, (tj or {}).get("answer", "") or last)
        sc = scores_of(tj)
        rec.update(claim=(tj or {}).get("result", "no-json"), claimed_steps=(tj or {}).get("steps"), notes=(tj or {}).get("notes", ""),
                   check=res, steps=steps, ratio=round(steps / ideal, 2) if ideal else None,
                   # rounds 1-2 compared tester commands with ideal presses: kept so round 3 compares both ways
                   commands=commands, ratio_cmd=round(commands / ideal, 2) if ideal else None,
                   scores=[{"screen": a, "score": b, "why": c} for a, b, c in sc], transcript=tpath)
        if err:
            rec["error"] = err
    except RigError as e:
        fd = failed_dir(session)
        rec.update(rig_error=True, **({"shots": os.path.join(fd, "shots")} if fd else {}), check={"pass": False, "reason": f"rig: {e}"}, error=f"{type(e).__name__}: {e}")
    except Exception as e:
        rec.update(check={"pass": False, "reason": "harness error"}, error=f"{type(e).__name__}: {e}")
    finally:
        end_session(session, acc)
    return rec


def has_signal(r):
    """Re-run triggers: failed the check, took > 1.5x the ideal steps, or any screen scored <= 2."""
    if r.get("skipped") or r.get("rig_error"):
        return False
    return (not r.get("check", {}).get("pass")) or (r.get("ratio") or 0) > SLOW or any(s["score"] <= LOW_SCORE for s in r.get("scores", []))


# ---------------------------------------------------------------- summary
def summarize(records, rnd, notes=()):
    L = [f"# Fleet round {rnd} summary", ""]
    rig = [r for r in records if r.get("rig_error")]
    ran = [r for r in records if not r.get("skipped") and not r.get("rig_error")]
    for n in notes:
        L.append(f"**{n}**")
        L.append("")
    skipped = [r for r in records if r.get("skipped")]
    if skipped:
        L += [f"**{len(skipped)} runs not started: {skipped[0]['skipped']}**", ""]
    if rig:
        L += [f"**{len(rig)} runs hit a rig error (the driver or the app never came up, e.g. a busy host or an app crash before its command pipe opened; retried and re-queued): left out of the numbers below. Look at: "
              + ", ".join(f"{r['task']}/{r['mode']} (session {r['session']})" for r in rig) + "**", ""]
    L += ["## Success", "", "Steps are presses on both sides (`key X n` = n). The last column is the rounds 1-2 measure: tester commands / ideal presses.", "",
          "| mode | runs | passed | success % | everyday success % | median steps/ideal (passed runs) | median commands/ideal (rounds 1-2 way) |",
          "|---|---|---|---|---|---|---|"]
    for m in MODES:
        rs = [r for r in ran if r["mode"] == m]
        if not rs:
            continue
        ok = [r for r in rs if r["check"]["pass"]]
        ev = [r for r in rs if r["kind"] == "everyday"]
        evok = [r for r in ev if r["check"]["pass"]]
        ratios = [r["ratio"] for r in ok if r.get("ratio") is not None]
        cratios = [r["ratio_cmd"] for r in ok if r.get("ratio_cmd") is not None]
        L.append(f"| {m} | {len(rs)} | {len(ok)} | {100 * len(ok) // len(rs)} | "
                 f"{(100 * len(evok) // len(ev)) if ev else '-'} | {statistics.median(ratios) if ratios else '-'} | "
                 f"{statistics.median(cratios) if cratios else '-'} |")
    by = {}
    for r in ran:
        for s in r.get("scores", []):
            by.setdefault(s["screen"].strip().lower(), []).append(s["score"])
    L += ["", "## Score per screen (1 bad - 5 great)", "", "| screen | seen | mean | lowest |", "|---|---|---|---|"]
    for name, v in sorted(by.items(), key=lambda kv: statistics.mean(kv[1])):
        L.append(f"| {name} | {len(v)} | {statistics.mean(v):.1f} | {min(v)} |")
    if not by:
        L.append("| - | 0 | - | - |")
    L += ["", "## Failures and slow runs", ""]
    bad = [r for r in ran if has_signal(r)]
    for r in bad:
        why = []
        if not r["check"]["pass"]:
            why.append("check failed: " + r["check"]["reason"])
        if (r.get("ratio") or 0) > SLOW:
            why.append(f"{r['steps']} presses vs ideal {r['ideal']} (x{r['ratio']}"
                       + (f"; {r['commands']} commands, x{r['ratio_cmd']} the rounds 1-2 way)" if r.get("ratio_cmd") is not None else ")"))
        low = [s for s in r.get("scores", []) if s["score"] <= LOW_SCORE]
        if low:
            why.append("low scores: " + "; ".join(f"{s['screen']} {s['score']} ({s['why']})" for s in low))
        if r.get("error"):
            why.append("harness: " + r["error"])
        L.append(f"- **{r['task']}** / {r['mode']} (run {r['run']}): " + " · ".join(why) +
                 f" — [transcript]({r.get('transcript', '')}) · shots: `{r['shots']}`")
    if not bad:
        L.append("None.")
    return "\n".join(L) + "\n"


# ---------------------------------------------------------------- main
def _jload(path):
    with open(path) as f:
        return json.load(f)


def jobs_from(tasks, modes):
    return [(t, m) for t in tasks for m in modes]


def load_results(path):
    """results.jsonl as {(task, mode, run): record}; a later line for the same run (a re-queue) replaces the earlier one."""
    out = {}
    if os.path.exists(path):
        with open(path) as f:
            for line in f:
                if line.strip():
                    try:
                        r = json.loads(line)
                    except ValueError:  # a line cut short when the process died
                        continue
                    out[(r["task"], r["mode"], r["run"])] = r
    return out


def execute_round(api, todo, rnd, rdir, repeat, stop_event, resume, notes):
    """First pass, then (if asked) the repeat pass for every run-1 with a signal. Each pass re-queues its rig errors once and
    rewrites summary.md, so a crash keeps the summary of every finished pass. With resume, runs already in results.jsonl
    (bar rig errors and not-started ones) are kept and skipped."""
    res_path = os.path.join(rdir, "results.jsonl")
    done = {}
    if resume:
        done = {key: r for key, r in load_results(res_path).items() if not r.get("rig_error") and not r.get("skipped")}
    with open(res_path, "w") as f:
        for r in done.values():
            f.write(json.dumps(r) + "\n")
    final = dict(done)  # (task, mode, run) -> record
    lock = threading.Lock()

    def run(t, m, k):
        r = test_one(api, t, m, rnd, k, stop_event)
        with lock:
            final[(t["id"], m, k)] = r
            with open(res_path, "a") as f:
                f.write(json.dumps(r) + "\n")
        return r

    def write_summary():
        n = list(notes)
        if stop_event.is_set():
            n.append(f"Plan reached {PLAN_STOP} %: stopped launching testers; finished the ones running.")
        with open(os.path.join(rdir, "summary.md"), "w") as f:
            f.write(summarize(list(final.values()), rnd, n))

    def do_pass(jobs):
        if jobs:
            with cf.ThreadPoolExecutor(MAX_SESSIONS) as ex:
                list(ex.map(lambda j: run(*j), jobs))
            again = [j for j in jobs if final[(j[0]["id"], j[1], j[2])].get("rig_error")]
            if again and not stop_event.is_set():
                with cf.ThreadPoolExecutor(MAX_SESSIONS) as ex:
                    list(ex.map(lambda j: run(*j), again))
        write_summary()

    do_pass([(t, m, 1) for t, m in todo if (t["id"], m, 1) not in done])
    if repeat and not stop_event.is_set():
        again = [(t, m, k) for t, m in todo if has_signal(final.get((t["id"], m, 1), {"skipped": True})) for k in (2, 3)
                 if (t["id"], m, k) not in done]
        do_pass(again)
    return list(final.values())


def main(argv=None):
    ap = argparse.ArgumentParser(prog="fleet-run")
    ap.add_argument("--round", type=int, required=True)
    ap.add_argument("--tasks", help="comma-separated task ids (default all)")
    ap.add_argument("--modes", default="desk,tv")
    ap.add_argument("--repeat-on-signal", action="store_true")
    ap.add_argument("--resume", action="store_true", help="keep this round's results.jsonl and run only what is not in it")
    ap.add_argument("--skip-validate", action="store_true", help="reuse routes-ok.json instead of replaying the routes")
    ap.add_argument("--force-invalid", action="store_true", help="give testers tasks whose route did not validate")
    ap.add_argument("--tasks-file", default=os.path.join(HERE, "tasks.yaml"))
    ap.add_argument("--out", default=os.environ.get("FLEET_OUT", os.path.join(REPO, "ux", "fleet")))
    a = ap.parse_args(argv)

    tasks = load_tasks(a.tasks_file)
    if a.tasks:
        want = a.tasks.split(",")
        missing = [w for w in want if w not in {t["id"] for t in tasks}]
        if missing:
            sys.exit(f"unknown task ids: {missing}")
        tasks = [t for t in tasks if t["id"] in want]
    modes = [m for m in a.modes.split(",") if m]
    if any(m not in MODES for m in modes):
        sys.exit(f"modes must be from {MODES}")
    os.makedirs(a.out, exist_ok=True)
    rdir = os.path.join(a.out, f"round-{a.round}")
    os.makedirs(rdir, exist_ok=True)
    make_wrapper()
    api = jfapi.Api.admin()
    stop_event = threading.Event()

    def bail(*_):
        cleanup_active()
        os._exit(130)
    signal.signal(signal.SIGINT, bail)
    signal.signal(signal.SIGTERM, bail)

    ok_path = os.path.join(a.out, "routes-ok.json")
    known = {}
    if (a.skip_validate or a.resume) and os.path.exists(ok_path):  # a resumed round keeps its routes (validation sessions still exist)
        known = _jload(ok_path)
    else:
        known = _jload(ok_path) if os.path.exists(ok_path) else {}
        with cf.ThreadPoolExecutor(MAX_SESSIONS) as ex:
            futs = {ex.submit(validate_one, api, t, m, a.round): (t["id"], m) for t, m in jobs_from(tasks, modes)}
            for f in cf.as_completed(futs):
                known[f"{futs[f][0]}:{futs[f][1]}"] = f.result()
        with open(ok_path, "w") as f:
            json.dump(known, f, indent=1, sort_keys=True)
    bad_routes = [k for t, m in jobs_from(tasks, modes) for k in [f"{t['id']}:{m}"] if not known.get(k, {}).get("ok")]
    notes = []
    if bad_routes:
        notes.append(f"{len(bad_routes)} routes did not validate ({'run anyway' if a.force_invalid else 'skipped'}): " + ", ".join(bad_routes))
        print("routes not ok:", ", ".join(bad_routes), file=sys.stderr)

    todo = [(t, m) for t, m in jobs_from(tasks, modes) if a.force_invalid or f"{t['id']}:{m}" not in bad_routes]
    records = execute_round(api, todo, a.round, rdir, a.repeat_on_signal, stop_event, a.resume, notes)
    print(f"round {a.round}: {len(records)} runs -> {rdir}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
