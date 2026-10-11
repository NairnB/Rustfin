"""jfapi.create_user: a new user starts with no history, even if Jellyfin reused a deleted user's internal id."""
import os
import sys
import unittest

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
        return {"Items": []}


class CreateUser(unittest.TestCase):
    def test_inherited_history_is_reset(self):
        a = Fake()
        jfapi.create_user(a, "fleet-x", "pw")
        resets = [p for p in a.posts if p[0].startswith("/UserItems/")]
        self.assertEqual([p[0] for p in resets], ["/UserItems/gunmen/UserData?userId=u9", "/UserItems/sintel/UserData?userId=u9"])
        self.assertEqual(resets[0][1], {"Played": False, "IsFavorite": False, "PlaybackPositionTicks": 0, "PlayCount": 0})


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
