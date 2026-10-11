"""check.py against recorded API answers (shapes captured from jf-real 10.10.7)."""
import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
import check  # noqa: E402

T = check.TICKS
TOS = {"Id": "tos", "Name": "Tears of Steel", "Type": "Movie"}
STREAMS = [
    {"Index": 0, "Type": "Subtitle", "Language": "ger"}, {"Index": 1, "Type": "Subtitle", "Language": "eng"},
    {"Index": 8, "Type": "Video", "Language": "eng"},
    {"Index": 9, "Type": "Audio", "Language": "eng", "Channels": 2, "IsDefault": True},
    {"Index": 10, "Type": "Audio", "Language": "eng", "Channels": 6},
]
EPS = {"Items": [{"Id": "e19", "Name": "The Gunmen", "ParentIndexNumber": 1, "IndexNumber": 19},
                 {"Id": "e20", "Name": "The Fear Merchants", "ParentIndexNumber": 1, "IndexNumber": 20},
                 {"Id": "m2", "Name": "The Mission", "ParentIndexNumber": 2, "IndexNumber": 2}]}


class FakeApi:
    """GET answers by path prefix (longest first); records deletes/posts."""
    def __init__(self, answers):
        self.answers, self.deleted, self.posted = answers, [], []

    def get(self, path):
        for k in sorted(self.answers, key=len, reverse=True):
            if path.startswith(k):
                return self.answers[k]
        raise AssertionError("unexpected GET " + path)

    def delete(self, path):
        self.deleted.append(path)

    def post(self, path, body=None):
        self.posted.append(path)


def sessions(item=TOS, pos=10, audio=None, sub=None, uid="u1"):
    ps = {"PositionTicks": int(pos * T)}
    if audio is not None:
        ps["AudioStreamIndex"] = audio
    if sub is not None:
        ps["SubtitleStreamIndex"] = sub
    return [{"UserId": uid, "UserName": "fleet-x", "DeviceId": "dev1", "NowPlayingItem": item, "PlayState": ps}]


def api(sess, **extra):
    a = {"/Sessions": sess,
         "/Users/u1/Items/tos": {"MediaSources": [{"MediaStreams": STREAMS}], "UserData": {}},
         "/Users/u1/Items?": {"Items": [TOS]}}
    a.update(extra)
    return FakeApi(a)


class NowPlaying(unittest.TestCase):
    def run_check(self, a, **args):
        return check.run_check(a, "u1", "now_playing", args, check.Ctx())

    def test_title_match_and_miss(self):
        self.assertTrue(self.run_check(api(sessions()), title="Tears of Steel")["pass"])
        r = self.run_check(api(sessions()), title="Sintel")
        self.assertFalse(r["pass"])
        self.assertIn("Sintel", r["reason"])

    def test_nothing_playing(self):
        self.assertFalse(self.run_check(api([]), title="Sintel")["pass"])
        self.assertFalse(self.run_check(api(sessions(uid="someone-else")), title="Tears of Steel")["pass"])

    def test_resume_position(self):
        self.assertTrue(self.run_check(api(sessions(pos=31)), title="Tears of Steel", min_position_s=28)["pass"])
        r = self.run_check(api(sessions(pos=0.5)), title="Tears of Steel", min_position_s=28)
        self.assertFalse(r["pass"])
        self.assertIn("resume", r["reason"])

    def test_subtitle_language(self):
        self.assertTrue(self.run_check(api(sessions(sub=1)), title="Tears of Steel", sub_lang="eng")["pass"])
        self.assertFalse(self.run_check(api(sessions(sub=0)), title="Tears of Steel", sub_lang="eng")["pass"])
        r = self.run_check(api(sessions()), title="Tears of Steel", sub_lang="eng")
        self.assertIn("no subtitle", r["reason"])
        self.assertFalse(self.run_check(api(sessions(sub=-1)), title="Tears of Steel", sub_lang="eng")["pass"])

    def test_surround_audio(self):
        self.assertTrue(self.run_check(api(sessions(audio=10)), title="Tears of Steel", audio_channels=6)["pass"])
        self.assertFalse(self.run_check(api(sessions(audio=9)), title="Tears of Steel", audio_channels=6)["pass"])
        # no choice reported = the server's default track (stereo)
        self.assertFalse(self.run_check(api(sessions()), title="Tears of Steel", audio_channels=6)["pass"])

    def test_episode_by_position_and_number(self):
        bon = {"Id": "bon", "Name": "Show A", "Type": "Series"}
        ep = {"Id": "e20", "Name": "The Fear Merchants", "SeriesName": "Show A", "ParentIndexNumber": 1, "IndexNumber": 20}
        a = api(sessions(item=ep), **{"/Users/u1/Items?": {"Items": [bon]}, "/Shows/bon/Episodes": EPS})
        self.assertTrue(self.run_check(a, series="Show A", nth=2)["pass"])
        self.assertFalse(self.run_check(a, series="Show A", nth=1)["pass"])
        self.assertFalse(self.run_check(a, series="Show A", season=2, episode=2)["pass"])
        m2 = dict(ep, Id="m2", ParentIndexNumber=2, IndexNumber=2)
        a = api(sessions(item=m2), **{"/Users/u1/Items?": {"Items": [bon]}, "/Shows/bon/Episodes": EPS})
        self.assertTrue(self.run_check(a, series="Show A", season=2, episode=2)["pass"])


class Others(unittest.TestCase):
    def test_user_data(self):
        a = api([], **{"/Users/u1/Items/tos": {"UserData": {"IsFavorite": True, "Played": False}}})
        self.assertTrue(check.run_check(a, "u1", "user_data", {"title": "Tears of Steel", "favourite": True}, check.Ctx())["pass"])
        self.assertFalse(check.run_check(a, "u1", "user_data", {"title": "Tears of Steel", "played": True}, check.Ctx())["pass"])

    def test_playlist_checked_then_deleted(self):
        a = api(sessions(), **{"/Users/u1/Items?Recursive=true&IncludeItemTypes=Playlist": {"Items": [{"Id": "pl", "Name": "Weekend"}]},
                               "/Playlists/pl/Items": {"Items": [{"Name": "Sintel"}, {"Name": "Tears of Steel"}]}})
        r = check.run_check(a, "u1", "playlist", {"name": "weekend", "contains": ["Sintel"], "playing": True}, check.Ctx())
        self.assertTrue(r["pass"], r)
        self.assertEqual(a.deleted, ["/Items/pl"])
        r = check.run_check(a, "u1", "playlist", {"name": "Weekend", "contains": ["Spring"]}, check.Ctx())
        self.assertFalse(r["pass"])
        r = check.run_check(a, "u1", "playlist", {"name": "Other"}, check.Ctx())
        self.assertFalse(r["pass"])

    def test_playlist_ignores_other_users_public_lists(self):
        # public playlists are visible to everyone: one made before the session (another user / fleet) is neither judged nor deleted
        a = api(sessions(), **{"/Users/u1/Items?Recursive=true&IncludeItemTypes=Playlist": {"Items": [
            {"Id": "old", "Name": "Weekend", "DateCreated": "2026-10-11T10:00:00.0000000Z"},
            {"Id": "new", "Name": "Weekend", "DateCreated": "2026-10-11T12:00:30.5000000Z"}]},
            "/Playlists/old/Items": {"Items": [{"Name": "Sintel"}]}, "/Playlists/new/Items": {"Items": [{"Name": "Spring"}]}})
        since = check._epoch("2026-10-11T12:00:00Z")
        r = check.run_check(a, "u1", "playlist", {"name": "Weekend", "contains": ["Spring"]}, check.Ctx(since=since))
        self.assertTrue(r["pass"], r)
        self.assertEqual(a.deleted, ["/Items/new"])
        a.deleted.clear()
        r = check.run_check(a, "u1", "playlist", {"name": "Weekend", "contains": ["Sintel"]}, check.Ctx(since=since))
        self.assertFalse(r["pass"])  # the old list holds Sintel but was not made in this session
        self.assertEqual(a.deleted, ["/Items/new"])

    def test_user_exists_deletes_the_created_user(self):
        a = FakeApi({"/Users": [{"Id": "x1", "Name": "guest-ab12"}, {"Id": "x2", "Name": "tester"}]})
        self.assertTrue(check.run_check(a, "u1", "user_exists", {"name": "guest-ab12"}, check.Ctx())["pass"])
        self.assertEqual(a.deleted, ["/Users/x1"])
        self.assertFalse(check.run_check(a, "u1", "user_exists", {"name": "nobody"}, check.Ctx())["pass"])

    def test_library_scan(self):
        since = check._epoch("2026-10-11T12:00:00Z")
        idle = lambda start: [{"Key": "RefreshLibrary", "State": "Idle", "LastExecutionResult": {"StartTimeUtc": start}}]  # noqa: E731
        act = {"/System/ActivityLog/Entries": {"Items": []}}
        self.assertTrue(check.run_check(FakeApi({"/ScheduledTasks": idle("2026-10-11T12:00:05.1234567Z"), **act}), "u1", "library_scan", {}, check.Ctx(since))["pass"])
        self.assertFalse(check.run_check(FakeApi({"/ScheduledTasks": idle("2026-10-11T11:00:00.0000000Z"), **act}), "u1", "library_scan", {}, check.Ctx(since))["pass"])
        run = [{"Key": "RefreshLibrary", "State": "Running"}]
        self.assertTrue(check.run_check(FakeApi({"/ScheduledTasks": run, **act}), "u1", "library_scan", {}, check.Ctx(since))["pass"])
        log = {"/System/ActivityLog/Entries": {"Items": [{"Name": "Library scan started", "Type": "x"}]}}
        self.assertTrue(check.run_check(FakeApi({"/ScheduledTasks": idle(""), **log}), "u1", "library_scan", {}, check.Ctx(since))["pass"])

    def test_user_config(self):
        a = FakeApi({"/Users/u1": {"Configuration": {"SubtitleMode": "Always", "SubtitleLanguagePreference": "eng"}}})
        self.assertTrue(check.run_check(a, "u1", "user_config", {"SubtitleMode": "Always"}, check.Ctx())["pass"])
        self.assertFalse(check.run_check(a, "u1", "user_config", {"SubtitleLanguagePreference": "spa"}, check.Ctx())["pass"])

    def test_session_user_needs_the_same_install(self):
        a = FakeApi({"/Sessions": [{"UserName": "viewer", "DeviceId": "dev1"}, {"UserName": "viewer", "DeviceId": "other"}]})
        self.assertTrue(check.run_check(a, "u1", "session_user", {"name": "viewer"}, check.Ctx(device_id="dev1"))["pass"])
        a = FakeApi({"/Sessions": [{"UserName": "viewer", "DeviceId": "other"}]})
        self.assertFalse(check.run_check(a, "u1", "session_user", {"name": "viewer"}, check.Ctx(device_id="dev1"))["pass"])

    def test_app_setting_paths(self):
        import json
        import tempfile
        with tempfile.TemporaryDirectory() as d:
            os.makedirs(os.path.join(d, "data"))
            with open(os.path.join(d, "data", "settings.json"), "w") as f:
                json.dump({"subtitles": {"style": {"size": "ExtraLarge"}},
                           "library_view": {"lib1": {"sort": "PremiereDate", "filters": {"unplayed": True}}}}, f)
            ctx = check.Ctx(session_dir=d)
            r = lambda p, v: check.run_check(None, "u1", "app_setting", {"path": p, "equals": v}, ctx)["pass"]  # noqa: E731
            self.assertTrue(r("subtitles.style.size", "ExtraLarge"))
            self.assertFalse(r("subtitles.style.size", "Small"))
            self.assertTrue(r("library_view.*.sort", "PremiereDate"))
            self.assertTrue(r("library_view.*.filters.unplayed", True))
            self.assertFalse(r("library_view.*.filters.played", True))
        self.assertFalse(check.run_check(None, "u1", "app_setting", {"path": "a", "equals": 1}, check.Ctx(session_dir="/nonexistent"))["pass"])

    def test_answer_contains(self):
        self.assertTrue(check.run_check(None, "u1", "answer_contains", {"all": ["2006"]}, check.Ctx(answer="It came out in 2006."))["pass"])
        self.assertFalse(check.run_check(None, "u1", "answer_contains", {"all": ["2006"]}, check.Ctx(answer="I don't know"))["pass"])

    def test_unknown_check_and_api_error(self):
        self.assertFalse(check.run_check(None, "u1", "nope", {}, check.Ctx())["pass"])

        class Boom:
            def get(self, p):
                raise check.jfapi.ApiError("GET x: HTTP 500")
        r = check.run_check(Boom(), "u1", "now_playing", {"title": "x"}, check.Ctx())
        self.assertTrue(r.get("error"))


if __name__ == "__main__":
    unittest.main()
