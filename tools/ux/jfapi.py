"""Tiny Jellyfin API client for the fleet harness (stdlib only). Used by check.py, fleet-user.sh and fleetrun.py."""
import contextlib
import fcntl
import json
import os
import re
import secrets
import subprocess
import sys
import urllib.error
import urllib.request

# jf-real's shared admin (tools/testserver/real/up.sh). Only used to create/delete throwaway fleet users and to
# read server state for checks; tester/viewer themselves are never modified.
ADMIN_USER = os.environ.get("JF_REAL_ADMIN", "tester")
ADMIN_PASS = os.environ.get("JF_REAL_ADMIN_PASS", "test-pass-1")


def _auth(device_id):
    return f'MediaBrowser Client="fleet", Device="fleet", DeviceId="{device_id}", Version="1"'


class ApiError(Exception):
    pass


def _alive(url):
    try:
        urllib.request.urlopen(url + "/System/Info/Public", timeout=4).read()
        return True
    except Exception:
        return False


def base_url():
    """JF_REAL_URL, else http://jf-real:8096 if the name resolves (inside the build container), else the container's IP."""
    env = os.environ.get("JF_REAL_URL")
    if env:
        return env.rstrip("/")
    cands = ["http://jf-real:8096"]
    try:
        out = subprocess.run(
            ["docker", "inspect", "jf-real", "--format", "{{range .NetworkSettings.Networks}}{{.IPAddress}} {{end}}"],
            capture_output=True, text=True, timeout=10).stdout.split()
        cands += [f"http://{ip}:8096" for ip in out]
    except Exception:
        pass
    for c in cands:
        if _alive(c):
            return c
    raise ApiError("jf-real is not reachable (tried " + ", ".join(cands) + ")")


class Api:
    def __init__(self, base, token=None, user_id=None):
        self.base, self.token, self.user_id = base.rstrip("/"), token, user_id
        # One device id per client object: the server keeps one token per user+device, so two logins of the same
        # user sharing an id would sign each other out (seen as 401s mid-run).
        self.device_id = "fleet-" + secrets.token_hex(6)
        self.creds = None

    @classmethod
    def login(cls, base, name, password):
        api = cls(base)
        api._sign_in(name, password)
        return api

    def _sign_in(self, name, password):
        self.token = None
        r = self.request("POST", "/Users/AuthenticateByName", {"Username": name, "Pw": password})
        self.token, self.user_id, self.creds = r["AccessToken"], r["User"]["Id"], (name, password)

    @classmethod
    def admin(cls, base=None):
        return cls.login(base or base_url(), ADMIN_USER, ADMIN_PASS)

    def request(self, method, path, body=None, retry=True):
        hdr = {"Content-Type": "application/json",
               "Authorization": _auth(self.device_id) + (f', Token="{self.token}"' if self.token else "")}
        data = json.dumps(body).encode() if body is not None else None
        req = urllib.request.Request(self.base + path, data=data, headers=hdr, method=method)
        try:
            raw = urllib.request.urlopen(req, timeout=30).read()
        except urllib.error.HTTPError as e:
            if e.code == 401 and self.creds and retry:  # token revoked: sign in again once
                self._sign_in(*self.creds)
                return self.request(method, path, body, retry=False)
            raise ApiError(f"{method} {path}: HTTP {e.code}") from None
        except urllib.error.URLError as e:
            raise ApiError(f"{method} {path}: {e.reason}") from None
        return json.loads(raw) if raw.strip() else None

    def get(self, path):
        return self.request("GET", path)

    def post(self, path, body=None):
        return self.request("POST", path, body)

    def delete(self, path):
        return self.request("DELETE", path)


def find_user(api, name):
    for u in api.get("/Users") or []:
        if u.get("Name") == name:
            return u
    return None


def load_env():
    """Optional private settings: KEY=VALUE lines of ${RF_ENV:-~/.config/rf-fleet.env}; variables already set win."""
    path = os.environ.get("RF_ENV") or os.path.join(os.path.expanduser("~"), ".config", "rf-fleet.env")
    try:
        lines = open(path).read().splitlines()
    except OSError:
        return
    for line in lines:
        k, eq, v = line.strip().partition("=")
        if eq and k.startswith("RF_"):
            os.environ.setdefault(k, v.strip().strip("\"'"))


def owner():
    """Who owns the fleet users this process makes: RF_OWNER, lower-case letters and digits only. No default, so two
    jobs sharing a server can never sweep each other's users. (`r<digits>` is the old session-name shape: not an owner.)"""
    o = os.environ.get("RF_OWNER", "")
    if not o:  # the other lead's name for the same thing: FLEET_USER_PREFIX=fleet-<owner>-
        m = re.fullmatch(r"fleet-([a-z0-9]+)-", os.environ.get("FLEET_USER_PREFIX", ""))
        o = m.group(1) if m else ""
    if not re.fullmatch(r"[a-z0-9]+", o) or re.fullmatch(r"r[0-9]+", o):
        raise ApiError("RF_OWNER is required (letters a-z and digits only, not r<number>): it names whose fleet users these are")
    return o


def fleet_name(session):
    return f"fleet-{owner()}-{session}"


def sweep_names(names, own, keep=()):
    """The users an owner's sweep deletes: `fleet-<own>-*` except the kept sessions. Nobody else's."""
    kept = {f"fleet-{own}-{k}" for k in keep}
    return [n for n in names if n.startswith(f"fleet-{own}-") and n not in kept]


def legacy_names(names):
    """fleet-* users with no owner: `fleet-<session>` or the old `fleet-r<N>-...` session names."""
    return [n for n in names if n.startswith("fleet-") and not re.match(r"fleet-(?!r[0-9]+-)[a-z0-9]+-", n)]


def fleet_home():
    return os.environ.get("RF_FLEET_HOME") or os.path.join(os.path.expanduser("~"), "rf-fleet")


def locks_dir():
    return os.environ.get("RF_LOCKS") or os.path.join(fleet_home(), "locks")


@contextlib.contextmanager
def create_lock(path=None):
    """One user is created at a time, across every runner on this machine: users made at the same moment can get the
    same internal id in Jellyfin and then share played marks, resume positions and favourites (seen: seeds lost, another
    tester's Sintel showing up)."""
    path = path or os.environ.get("FLEET_USER_LOCK") or os.path.join(locks_dir(), "fleet-user")
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        fcntl.flock(f, fcntl.LOCK_EX)
        try:
            yield
        finally:
            fcntl.flock(f, fcntl.LOCK_UN)


def clean_user_data(api, uid):
    """Jellyfin reuses the internal id of a deleted user, so a new user can inherit its played marks, resume
    positions and favourites (seen: a new user starting with another tester's Sintel in Continue watching). Reset them."""
    for flt in ("IsPlayed", "IsResumable", "IsFavorite"):
        r = api.get(f"/Users/{uid}/Items?Recursive=true&Filters={flt}&IncludeItemTypes=Movie,Episode,Series,Season,BoxSet") or {}
        for it in r.get("Items", []):
            api.post(f"/UserItems/{it['Id']}/UserData?userId={uid}",
                     {"Played": False, "IsFavorite": False, "PlaybackPositionTicks": 0, "PlayCount": 0})


def create_user(api, name, password, admin=False):
    u = api.post("/Users/New", {"Name": name, "Password": password})
    clean_user_data(api, u["Id"])
    # the server remembers track choices per item and user id, and reuses ids: start on the true default tracks
    cfg = api.get(f"/Users/{u['Id']}")["Configuration"]
    cfg["RememberAudioSelections"] = False
    cfg["RememberSubtitleSelections"] = False
    api.post(f"/Users/{u['Id']}/Configuration", cfg)
    if admin:
        pol = api.get(f"/Users/{u['Id']}")["Policy"]
        pol["IsAdministrator"] = True
        api.post(f"/Users/{u['Id']}/Policy", pol)
    return u


def delete_user(api, name):
    u = find_user(api, name)
    if u:
        api.delete(f"/Users/{u['Id']}")
    return bool(u)


def presign(data_dir, server_addr, name, password):
    """Write the app's settings.json with a signed-in session, so the app starts on Home without any typed text.
    (The TV build's on-screen keyboard stands down for the whole session once any text event has arrived — and the
    sign-in prelude of rf-drive types — so a TV tester would otherwise never get the keyboard.)"""
    base = base_url()
    api = Api(base)
    api.device_id = secrets.token_hex(16)  # the app's own device id: 32 hex, like Settings::device_id
    api._sign_in(name, password)
    info = api.get("/System/Info/Public") or {}
    session = {"user_id": api.user_id, "user_name": name, "token": api.token}
    sid = info.get("Id", "")
    settings = {"device_id": api.device_id, "last_server": sid,
                "servers": [{"id": sid, "name": info.get("ServerName", ""), "address": server_addr.rstrip("/"), "session": session}],
                "saved_users": [{"server_id": sid, "session": session}]}
    os.makedirs(data_dir, exist_ok=True)
    with open(os.path.join(data_dir, "settings.json"), "w") as f:
        json.dump(settings, f)


def _user_cli(argv):
    if argv and argv[0] == "presign" and len(argv) == 5:
        presign(*argv[1:])
        return 0
    usage = "usage: fleet-user.sh create|delete <session> [--admin] | sweep --owner <o> [keep-session...] | list --legacy"
    if argv and argv[0] == "sweep":
        if len(argv) < 3 or argv[1] != "--owner":
            print("fleet-user: sweep needs --owner <o> (it deletes only that owner's fleet-<o>-* users)", file=sys.stderr)
            return 2
        if not re.fullmatch(r"[a-z0-9]+", argv[2]) or re.fullmatch(r"r[0-9]+", argv[2]):
            print("fleet-user: --owner must be letters a-z and digits only, not r<number> (that is the old session-name shape)", file=sys.stderr)
            return 2
    elif argv and argv[0] == "list":
        if argv[1:] != ["--legacy"]:
            print(usage, file=sys.stderr)
            return 2
    elif len(argv) < 2 or argv[0] not in ("create", "delete"):
        print(usage, file=sys.stderr)
        return 2
    cmd = argv[0]
    api = Api.admin()
    if cmd == "create":
        name, pw = fleet_name(argv[1]), secrets.token_urlsafe(12)
        with create_lock():
            delete_user(api, name)  # a leftover from a crashed run
            create_user(api, name, pw, admin="--admin" in argv)
        print(json.dumps({"user": name, "pass": pw}))
    elif cmd == "delete":
        delete_user(api, fleet_name(argv[1]))
    elif cmd == "list":  # fleet users with no owner, for a human to decide; deletes nothing
        for n in legacy_names([u["Name"] for u in api.get("/Users") or []]):
            print(n)
    else:  # sweep: remove this owner's fleet users except the sessions named
        doomed = set(sweep_names([u["Name"] for u in api.get("/Users") or []], argv[2], argv[3:]))
        for u in api.get("/Users") or []:
            if u["Name"] in doomed:
                api.delete(f"/Users/{u['Id']}")
                print("deleted", u["Name"])
    try:
        api.post("/Sessions/Logout")  # leave no stale admin session behind
    except ApiError:
        pass
    return 0


if __name__ == "__main__":
    try:
        sys.exit(_user_cli(sys.argv[1:]))
    except ApiError as e:
        print(f"fleet-user: {e}", file=sys.stderr)
        sys.exit(1)
