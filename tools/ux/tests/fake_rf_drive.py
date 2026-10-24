#!/usr/bin/env python3
"""A stand-in for rf-drive for dry runs (no app): same CLI as the RFD spec, and on the closing `wait 3000` of a route
it makes the server see what a successful run of that session's task would have done (plays the film, sets the
favourite / played flag), as the session's own fleet user. Session names look like r1-<task-id>-<mode>-<k>."""
import json
import os
import re
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
import check  # noqa: E402
import jfapi  # noqa: E402
import miniyaml  # noqa: E402

jfapi.load_env()
HOME = jfapi.fleet_home()
TASKS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "tasks.yaml")


def arg(argv, flag):
    return argv[argv.index(flag) + 1] if flag in argv else None


def task_of(session):
    m = re.match(r"r\d+-(?:v-)?(.+)-(?:desk|tv)-\d+$", session)
    return next((t for t in miniyaml.loads(open(TASKS).read()) if t["id"] == m.group(1)), None) if m else None


def effect(session, user, pw):
    t = task_of(session)
    if not t:
        return
    me = jfapi.Api.login(jfapi.base_url(), user, pw)
    c = t["check"]
    a = c.get("args") or {}
    if c["name"] == "now_playing":
        it = check.find_item(me, me.user_id, title=a["title"])
        me.post("/Sessions/Playing", {"ItemId": it["Id"], "PositionTicks": 0, "CanSeek": True})
        me.post("/Sessions/Playing/Progress", {"ItemId": it["Id"], "PositionTicks": (a.get("min_position_s", 3) + 2) * check.TICKS})
    elif c["name"] == "user_data":
        it = check.find_item(me, me.user_id, title=a["title"])
        if a.get("favourite"):
            me.post(f"/Users/{me.user_id}/FavoriteItems/{it['Id']}")
        if a.get("played"):
            me.post(f"/UserPlayedItems/{it['Id']}?userId={me.user_id}")


def forced_start_failure(session):
    """FAKE_START_FAIL=<session substring>:<n> makes the first n `start`s of a matching session fail (a busy host)."""
    spec = os.environ.get("FAKE_START_FAIL")
    if not spec:
        return False
    sub, _, n = spec.rpartition(":")
    if sub not in session:
        return False
    mark = os.path.join(HOME, f".fail-{session}")
    done = int(open(mark).read()) if os.path.exists(mark) else 0
    if done >= int(n):
        return False
    open(mark, "w").write(str(done + 1))
    return True


def main(argv):
    if argv[0] == "start":
        s = arg(argv, "--session")
        d = os.path.join(HOME, s)
        if os.path.exists(d):  # like the real driver: a used session name is refused
            print(f"session {s} exists (stop it and pick a new name)", file=sys.stderr)
            return 1
        if forced_start_failure(s):  # the real driver leaves the folder behind when its prelude fails
            os.makedirs(os.path.join(d, "shots"), exist_ok=True)
            # the real driver's wording when the app never started (send's die, then pre's)
            print("rf-drive: app did not take the command (is the session running?)\nrf-drive: sign-in prelude failed (see app.log)", file=sys.stderr)
            return 1
        os.makedirs(os.path.join(d, "shots"), exist_ok=True)
        os.makedirs(os.path.join(d, "data"), exist_ok=True)
        open(os.path.join(d, "out"), "a").close()
        open(os.path.join(d, "cmds"), "a").close()
        json.dump({"user": arg(argv, "--user"), "pass": arg(argv, "--pass")}, open(os.path.join(d, ".fake-account"), "w"))
        jfapi.Api.login(jfapi.base_url(), arg(argv, "--user"), arg(argv, "--pass"))  # signing in makes the server session
        print(os.path.join(d, "shots", "000.jpg"))
        return 0
    if argv[0] == "stop":
        return 0
    s, cmd = argv[0], " ".join(argv[1:])
    d = os.path.join(HOME, s)
    n = sum(1 for _ in open(os.path.join(d, "out"))) + 1
    if cmd == "wait 3000":
        acc = json.load(open(os.path.join(d, ".fake-account")))
        effect(s, acc["user"], acc["pass"])
    with open(os.path.join(d, "out"), "a") as f:
        f.write(f"{n:03d} ok fake 1\n")
    with open(os.path.join(d, "cmds"), "a") as f:
        f.write(cmd + "\n")
    print(f"step {n}: ok — screenshot: {os.path.join(d, 'shots', '%03d.jpg' % n)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
