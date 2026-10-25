"""jfapi.create_user: a new user starts with no history, even if Jellyfin reused a deleted user's internal id."""
import contextlib
import io
import os
import sys
import unittest
from unittest import mock

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
import jfapi  # noqa: E402


class Fake:
    def __init__(self):
        self.posts = []

    def post(self, path, body=None):
        self.posts.append((path, body))
        return {"Id": "u9"} if path == "/Users/New" else {}

    def get(self, path):
        if "Filters=IsResumable" in path:
            return {"Items": [{"Id": "sintel"}]}
        if "Filters=IsPlayed" in path:
            return {"Items": [{"Id": "gunmen"}]}
        if path.startswith("/Users/") and "/Items" not in path:
            return {"Configuration": {"RememberAudioSelections": True, "RememberSubtitleSelections": True, "PlayDefaultAudioTrack": True},
                    "Policy": {}}
        return {"Items": []}


class CreateUser(unittest.TestCase):
    def test_inherited_history_is_reset(self):
        a = Fake()
        jfapi.create_user(a, "fleet-x", "pw")
        resets = [p for p in a.posts if p[0].startswith("/UserItems/")]
        self.assertEqual([p[0] for p in resets], ["/UserItems/gunmen/UserData?userId=u9", "/UserItems/sintel/UserData?userId=u9"])
        self.assertEqual(resets[0][1], {"Played": False, "IsFavorite": False, "PlaybackPositionTicks": 0, "PlayCount": 0})


    def test_create_user_turns_off_remember_tracks(self):
        a = Fake()
        jfapi.create_user(a, "fleet-x", "pw")
        cfg = [p for p in a.posts if p[0] == "/Users/u9/Configuration"]
        self.assertEqual(len(cfg), 1)
        self.assertFalse(cfg[0][1]["RememberAudioSelections"])
        self.assertFalse(cfg[0][1]["RememberSubtitleSelections"])
        self.assertTrue(cfg[0][1]["PlayDefaultAudioTrack"])  # nothing else in the configuration changes

    def test_only_fleet_users_touched(self):
        a = Fake()
        jfapi.create_user(a, "fleet-x", "pw")
        users = {p[0].split("/")[2] for p in a.posts if p[0].startswith("/Users/") and p[0] != "/Users/New"}
        self.assertEqual(users, {"u9"})  # only the id of the user it just created


class Owner(unittest.TestCase):
    def setUp(self):
        self._saved = os.environ.pop("RF_OWNER", None)

    def tearDown(self):
        os.environ.pop("RF_OWNER", None)
        if self._saved is not None:
            os.environ["RF_OWNER"] = self._saved

    def test_o1_name_carries_the_owner(self):
        os.environ["RF_OWNER"] = "team2"
        self.assertEqual(jfapi.fleet_name("r1-play-desk-0"), "fleet-team2-r1-play-desk-0")

    def test_o1_owner_required_and_checked(self):
        with self.assertRaises(jfapi.ApiError) as c:
            jfapi.fleet_name("s")
        self.assertIn("RF_OWNER", str(c.exception))
        for bad in ("Abc", "a-b", "a b", "", "r12"):
            os.environ["RF_OWNER"] = bad
            with self.assertRaises(jfapi.ApiError):
                jfapi.fleet_name("s")

    def test_o2_sweep_only_own_users_minus_keep(self):
        users = ["fleet-me-a", "fleet-me-b", "fleet-other-a", "fleet-r3-x-desk-0", "tester", "fleet-mex-a"]
        self.assertEqual(jfapi.sweep_names(users, "me", ["b"]), ["fleet-me-a"])

    def test_o2_bare_sweep_refused_and_deletes_nothing(self):
        for argv in (["sweep"], ["sweep", "none"]):
            a = SweepApi()
            with mock.patch.object(jfapi.Api, "admin", return_value=a), contextlib.redirect_stderr(io.StringIO()) as err:
                rc = jfapi._user_cli(argv)
            self.assertNotEqual(rc, 0)
            self.assertIn("sweep needs --owner", err.getvalue())
            self.assertEqual(a.deleted, [])

    def test_fleet_user_prefix_is_an_alias_for_rf_owner(self):
        with mock.patch.dict(os.environ, {"FLEET_USER_PREFIX": "fleet-team3-"}, clear=False):
            os.environ.pop("RF_OWNER", None)
            self.assertEqual(jfapi.owner(), "team3")
        with mock.patch.dict(os.environ, {"FLEET_USER_PREFIX": "fleet-"}, clear=False):
            os.environ.pop("RF_OWNER", None)
            with self.assertRaises(jfapi.ApiError):  # a bare prefix names no owner
                jfapi.owner()

    def test_o4_sweep_refuses_legacy_shaped_owner(self):
        calls = []
        with mock.patch.object(jfapi.Api, "admin", side_effect=lambda: calls.append("admin")), \
             mock.patch("sys.stderr", new_callable=io.StringIO):
            self.assertEqual(jfapi._user_cli(["sweep", "--owner", "r3"]), 2)
        self.assertEqual(calls, [])  # refused before any server call

    def test_o2_owner_sweep_deletes_only_owner(self):
        a = SweepApi()
        with mock.patch.object(jfapi.Api, "admin", return_value=a), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(jfapi._user_cli(["sweep", "--owner", "me", "keepme"]), 0)
        self.assertEqual(a.deleted, ["/Users/id-fleet-me-a"])  # not other, not legacy, not keepme

    def test_o4_legacy_list(self):
        a = SweepApi()
        out = io.StringIO()
        with mock.patch.object(jfapi.Api, "admin", return_value=a), contextlib.redirect_stdout(out):
            self.assertEqual(jfapi._user_cli(["list", "--legacy"]), 0)
        self.assertEqual(out.getvalue().split(), ["fleet-r3-x-desk-0", "fleet-oldstyle"])
        self.assertEqual(a.deleted, [])


class SweepApi:
    NAMES = ["tester", "fleet-me-a", "fleet-me-keepme", "fleet-other-a", "fleet-r3-x-desk-0", "fleet-oldstyle"]

    def __init__(self):
        self.deleted = []

    def get(self, path):
        return [{"Name": n, "Id": "id-" + n} for n in self.NAMES]

    def delete(self, path):
        self.deleted.append(path)

    def post(self, path, body=None):
        return {}


class Lock(unittest.TestCase):
    def test_one_creator_at_a_time(self):
        import tempfile, threading, time
        path = os.path.join(tempfile.mkdtemp(), "lock")
        live, worst = [0], [0]
        def creator():
            with jfapi.create_lock(path):
                live[0] += 1
                worst[0] = max(worst[0], live[0])
                time.sleep(0.05)
                live[0] -= 1
        ts = [threading.Thread(target=creator) for _ in range(4)]
        [t.start() for t in ts]
        [t.join() for t in ts]
        self.assertEqual(worst[0], 1)


if __name__ == "__main__":
    unittest.main()
