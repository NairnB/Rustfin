#!/usr/bin/env python3
"""Server-side success checks for the Haiku fleet (BRIEF-haiku-fleet §2). Stdlib only: run with `python3 -I`.

  check.py <name> --session S --user fleet-S [--args JSON] [--since EPOCH] [--device-id ID] [--session-dir DIR]
prints one JSON line {"pass": bool, "reason": str}; exit 0 pass, 1 fail, 2 could not check.

Truth comes from jf-real's API for that session's user (read with the shared admin login), never from the tester's claim. Each check function takes
(api, uid, args, ctx) — `api` any object with get/post/delete, so the unit tests feed recorded answers.
Checks that create things (admin: new user; playlists) delete them again after reading.
"""
import argparse
import datetime
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # -I drops the script dir from sys.path
import jfapi  # noqa: E402

TICKS = 10_000_000  # Jellyfin ticks per second


class Ctx:
    def __init__(self, since=0, device_id=None, session_dir=None, answer=""):
        self.since, self.device_id, self.session_dir, self.answer = since, device_id, session_dir, answer


def ok(reason="ok"):
    return {"pass": True, "reason": reason}


def fail(reason):
    return {"pass": False, "reason": reason}


def _q(s):
    from urllib.parse import quote
    return quote(str(s), safe="")


def find_item(api, uid, title=None, series=None, season=None, episode=None, nth=None):
    """An item by exact (case-insensitive) title, or an episode of a series by season+episode or position (nth, 1-based)."""
    if series:
        s = find_item(api, uid, title=series)
        if not s:
            return None
        eps = (api.get(f"/Shows/{s['Id']}/Episodes?userId={uid}") or {}).get("Items", [])
        for i, e in enumerate(eps, 1):
            if nth is not None and i == nth:
                return e
            if season is not None and episode is not None and e.get("ParentIndexNumber") == season and e.get("IndexNumber") == episode:
                return e
        return None
    r = api.get(f"/Users/{uid}/Items?Recursive=true&SearchTerm={_q(title)}&IncludeItemTypes=Movie,Series,Episode,BoxSet") or {}
    items = r.get("Items", [])
    for i in items:
        if i.get("Name", "").lower() == str(title).lower():
            return i
    return items[0] if items and len(items) == 1 else None


def _streams(api, uid, item_id):
    it = api.get(f"/Users/{uid}/Items/{item_id}") or {}
    srcs = it.get("MediaSources") or [{}]
    return srcs[0].get("MediaStreams", [])


def _wanted_stream(streams, kind, index, lang=None, channels=None):
    s = next((x for x in streams if x.get("Type") == kind and x.get("Index") == index), None)
    if s is None:
        return None
    return s


def now_playing(api, uid, args, ctx):
    """Sessions: the user is playing <title | series+season+episode | series+nth> [from >= min_position_s]
    [audio_lang | audio_channels] [sub_lang | sub: none]."""
    mine = [s for s in (api.get("/Sessions") or []) if s.get("UserId") == uid and s.get("NowPlayingItem")]
    if not mine:
        return fail("nothing is playing for this user")
    s = mine[0]
    np, ps = s["NowPlayingItem"], s.get("PlayState") or {}
    if args.get("series"):
        want = find_item(api, uid, series=args["series"], season=args.get("season"), episode=args.get("episode"), nth=args.get("nth"))
        if not want:
            return fail(f"cannot work out the wanted episode of {args['series']}")
        if np.get("Id") != want["Id"]:
            return fail(f"playing {np.get('SeriesName', '')} S{np.get('ParentIndexNumber')}E{np.get('IndexNumber')} {np.get('Name')!r}, wanted {want.get('Name')!r}")
    elif args.get("title") and np.get("Name", "").lower() != args["title"].lower():
        return fail(f"playing {np.get('Name')!r}, wanted {args['title']!r}")
    pos = (ps.get("PositionTicks") or 0) / TICKS
    if pos < args.get("min_position_s", 0):
        return fail(f"position {pos:.0f}s is before {args['min_position_s']}s (did not resume)")
    if any(k in args for k in ("audio_lang", "audio_channels", "sub_lang", "sub")):
        streams = _streams(api, uid, np["Id"])
        ai, si = ps.get("AudioStreamIndex"), ps.get("SubtitleStreamIndex")
        if "audio_channels" in args or "audio_lang" in args:
            if ai is None:  # the server's default audio when the client never chose one
                a = next((x for x in streams if x.get("Type") == "Audio" and x.get("IsDefault")), None)
            else:
                a = _wanted_stream(streams, "Audio", ai)
            if a is None:
                return fail("no audio stream selected")
            if "audio_channels" in args and (a.get("Channels") or 0) < args["audio_channels"]:
                return fail(f"audio has {a.get('Channels')} channels, wanted {args['audio_channels']}")
            if "audio_lang" in args and a.get("Language") != args["audio_lang"]:
                return fail(f"audio language {a.get('Language')}, wanted {args['audio_lang']}")
        if args.get("sub") == "none" and si not in (None, -1):
            return fail("subtitles are on, wanted off")
        if "sub_lang" in args:
            if si in (None, -1):
                return fail("no subtitle stream selected")
            sub = _wanted_stream(streams, "Subtitle", si)
            if sub is None or sub.get("Language") != args["sub_lang"]:
                return fail(f"subtitle stream {si} is {sub and sub.get('Language')}, wanted {args['sub_lang']}")
    return ok(f"playing {np.get('Name')!r} at {pos:.0f}s")


def user_data(api, uid, args, ctx):
    """UserData of <title>: favourite / played / min_resume_s."""
    it = find_item(api, uid, title=args["title"])
    if not it:
        return fail(f"no item called {args['title']!r}")
    ud = (api.get(f"/Users/{uid}/Items/{it['Id']}") or {}).get("UserData") or {}
    if "favourite" in args and bool(ud.get("IsFavorite")) != args["favourite"]:
        return fail(f"favourite is {bool(ud.get('IsFavorite'))}, wanted {args['favourite']}")
    if "played" in args and bool(ud.get("Played")) != args["played"]:
        return fail(f"played is {bool(ud.get('Played'))}, wanted {args['played']}")
    if (ud.get("PlaybackPositionTicks") or 0) / TICKS < args.get("min_resume_s", 0):
        return fail("no resume position saved")
    return ok("user data as wanted")


def playlist(api, uid, args, ctx):
    """The user made a playlist (name) holding all of `contains`; with playing: true the user is now playing one of
    them. The app's playlists are public, so every other user (and other fleets) sees them: only playlists created since
    the session began are judged, and only those are deleted after reading."""
    r = api.get(f"/Users/{uid}/Items?Recursive=true&IncludeItemTypes=Playlist&Fields=DateCreated") or {}
    lists = [p for p in r.get("Items", []) if _created_since(p, ctx.since)]
    try:
        pl = next((p for p in lists if "name" not in args or p.get("Name", "").lower() == args["name"].lower()), None)
        if pl is None:
            return fail("no playlist" + (f" called {args['name']!r}" if "name" in args else "") + " made in this session")
        names = [i.get("Name") for i in (api.get(f"/Playlists/{pl['Id']}/Items?userId={uid}") or {}).get("Items", [])]
        missing = [c for c in args.get("contains", []) if c not in names]
        if missing:
            return fail(f"playlist {pl['Name']!r} holds {names}, missing {missing}")
        if args.get("playing"):
            mine = [s for s in (api.get("/Sessions") or []) if s.get("UserId") == uid and s.get("NowPlayingItem")]
            if not mine or mine[0]["NowPlayingItem"].get("Name") not in names:
                return fail("nothing from the playlist is playing")
        return ok(f"playlist {pl['Name']!r} holds {names}")
    finally:
        for p in lists:
            try:
                api.delete(f"/Items/{p['Id']}")
            except Exception:
                pass


def _created_since(item, since):
    """True when the item has no creation date to judge by, or it was created at or after `since` (epoch s, 5 s slack)."""
    made = item.get("DateCreated")
    if not since or not made:
        return True
    return _epoch(made) >= since - 5


def user_exists(api, uid, args, ctx):
    """Admin: a user called `name` exists (it is deleted after reading)."""
    u = jfapi.find_user(api, args["name"])
    if not u:
        return fail(f"no user called {args['name']!r}")
    api.delete(f"/Users/{u['Id']}")
    return ok(f"user {args['name']!r} was created (and removed again)")


def library_scan(api, uid, args, ctx):
    """Admin: a library scan ran or is running since the session began (ScheduledTasks, then the activity log)."""
    for t in api.get("/ScheduledTasks") or []:
        if t.get("Key") != "RefreshLibrary":
            continue
        if t.get("State") in ("Running", "Cancelling"):
            return ok("a library scan is running")
        st = ((t.get("LastExecutionResult") or {}).get("StartTimeUtc")) or ""
        if st and _epoch(st) >= ctx.since:
            return ok(f"library scan started {st}")
    since_iso = datetime.datetime.fromtimestamp(ctx.since, datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    log = (api.get(f"/System/ActivityLog/Entries?minDate={_q(since_iso)}&limit=100") or {}).get("Items", [])
    for e in log:
        text = (e.get("Name", "") + " " + e.get("Type", "")).lower()
        if "scan" in text or "refresh" in text or "librar" in text:
            return ok("activity log: " + e.get("Name", ""))
    return fail("no library scan started since the session began")


def user_config(api, uid, args, ctx):
    """The user's server-side configuration holds the given values (e.g. SubtitleMode, SubtitleLanguagePreference)."""
    cfg = (api.get(f"/Users/{uid}") or {}).get("Configuration") or {}
    for k, v in args.items():
        if cfg.get(k) != v:
            return fail(f"{k} is {cfg.get(k)!r}, wanted {v!r}")
    return ok("user configuration as wanted")


def session_user(api, uid, args, ctx):
    """Another user (`name`) is signed in from the same app install (same DeviceId as this session started with)."""
    for s in api.get("/Sessions") or []:
        if s.get("UserName") != args["name"]:
            continue
        if ctx.device_id and s.get("DeviceId") != ctx.device_id:
            continue
        return ok(f"{args['name']} is signed in from this app")
    return fail(f"{args['name']!r} is not signed in from this app")


def _walk(obj, parts):
    """All values at a dotted path; '*' matches any key of a dict."""
    if not parts:
        return [obj]
    if not isinstance(obj, dict):
        return []
    head, rest = parts[0], parts[1:]
    keys = list(obj) if head == "*" else ([head] if head in obj else [])
    return [v for k in keys for v in _walk(obj[k], rest)]


def app_setting(api, uid, args, ctx):
    """The app's own settings.json (in the session's data dir) has `path` (dotted, '*' = any key) equal to `equals`."""
    p = os.path.join(ctx.session_dir or "", "data", "settings.json")
    try:
        with open(p) as f:
            data = json.load(f)
    except (OSError, ValueError) as e:
        return fail(f"cannot read the app settings: {e}")
    vals = _walk(data, args["path"].split("."))
    if args["equals"] in vals:
        return ok(f"{args['path']} = {args['equals']!r}")
    return fail(f"{args['path']} is {vals!r}, wanted {args['equals']!r}")


def answer_contains(api, uid, args, ctx):
    """The tester's own written answer names every item of `all` (case-insensitive) — for questions with no server state."""
    low = (ctx.answer or "").lower()
    missing = [w for w in args["all"] if w.lower() not in low]
    return fail(f"answer lacks {missing}") if missing else ok("answer names " + ", ".join(args["all"]))


CHECKS = {f.__name__: f for f in (now_playing, user_data, playlist, user_exists, library_scan, user_config,
                                  session_user, app_setting, answer_contains)}


def _epoch(iso):
    """ISO time from the server (7 fractional digits, trailing Z) to epoch seconds."""
    t = iso.replace("Z", "+00:00")
    if "." in t:
        head, tail = t.split(".", 1)
        n = 0
        while n < len(tail) and tail[n].isdigit():
            n += 1
        t = head + "." + tail[:n][:6].ljust(6, "0") + tail[n:]
    return datetime.datetime.fromisoformat(t).timestamp()


def run_check(api, uid, name, args, ctx):
    if name not in CHECKS:
        return fail(f"unknown check {name!r}")
    try:
        return CHECKS[name](api, uid, args or {}, ctx)
    except jfapi.ApiError as e:
        return {"pass": False, "reason": f"could not check: {e}", "error": True}


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("name")
    ap.add_argument("--session", required=True)
    ap.add_argument("--user", required=True)
    ap.add_argument("--args", default="{}")
    ap.add_argument("--since", type=float, default=0)
    ap.add_argument("--device-id")
    ap.add_argument("--session-dir")
    ap.add_argument("--answer", default="")
    a = ap.parse_args(argv)
    try:
        admin = jfapi.Api.admin()
        u = jfapi.find_user(admin, a.user)
        if not u:
            r = {"pass": False, "reason": f"user {a.user} not found", "error": True}
        else:
            r = run_check(admin, u["Id"], a.name, json.loads(a.args), Ctx(a.since, a.device_id, a.session_dir, a.answer))
    except jfapi.ApiError as e:
        r = {"pass": False, "reason": f"could not check: {e}", "error": True}
    print(json.dumps(r))
    return 0 if r["pass"] else (2 if r.get("error") else 1)


if __name__ == "__main__":
    sys.exit(main())
