import copy
import shutil
import json
import sys
import tempfile
import unittest
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FFMPEG = (
    str(ROOT / "runtime/ffmpeg.exe")
    if (ROOT / "runtime/ffmpeg.exe").is_file()
    else shutil.which("ffmpeg")
)
sys.path.insert(0, str(ROOT / "bridge"))
import catalog


class AlbumTests(unittest.TestCase):
    def fixture(self, prefix="beatles"):
        album = json.loads(
            (ROOT / f"tests/fixtures/{prefix}-album.json").read_text(encoding="utf8")
        )
        playlist = json.loads(
            (ROOT / f"tests/fixtures/{prefix}-playlist.json").read_text(encoding="utf8")
        )
        return album, playlist

    def test_real_beatles_uses_audio_playlist_not_album_video(self):
        a, p = self.fixture()
        result = catalog.build_album("MPREb_crTwIzZFB2i", a, p)
        self.assertEqual(result["tracks"][1]["video_id"], "L0s9DdFUTxg")
        self.assertNotEqual(result["tracks"][1]["video_id"], a["tracks"][1]["videoId"])
        self.assertNotIn("Y_V6y1ZCg_8", [t["video_id"] for t in result["tracks"]])
        self.assertTrue(all(not t["unavailable"] for t in result["tracks"]))

    def test_real_soda_keeps_album_and_all_ten_tracks(self):
        a, p = self.fixture("soda")
        result = catalog.build_album("MPREb_xkBO3uL2Y0v", a, p)
        self.assertEqual(result["title"], "Nada Personal")
        self.assertEqual(len(result["tracks"]), 10)
        self.assertTrue(
            all(t["source_type"] == "MUSIC_VIDEO_TYPE_ATV" for t in result["tracks"])
        )

    def test_recording_from_another_album_is_allowed_when_in_selected_audio_list(self):
        a, p = self.fixture()
        p["tracks"][0]["album"] = {"name": "Earlier publication", "id": "MPRE_other"}
        result = catalog.build_album("MPRE_test", a, p)
        self.assertFalse(result["tracks"][0]["unavailable"])
        self.assertEqual(result["title"], a["title"])
        self.assertEqual(result["tracks"][0]["source_album"], "Earlier publication")

    def test_changed_order_does_not_choose_similar_song(self):
        a, p = self.fixture()
        p["tracks"][0], p["tracks"][1] = p["tracks"][1], p["tracks"][0]
        result = catalog.build_album("MPRE_test", a, p)
        self.assertTrue(result["tracks"][0]["unavailable"])
        self.assertTrue(result["tracks"][1]["unavailable"])

    def test_wrong_mix_is_blocked_even_with_identical_duration(self):
        a, p = self.fixture()
        p["tracks"][1]["title"] = (
            "Norwegian Wood (This Bird Has Flown) (Remastered 2009)"
        )
        result = catalog.build_album("MPRE_test", a, p)
        self.assertTrue(result["tracks"][1]["unavailable"])

    def test_video_in_audio_playlist_is_blocked(self):
        a, p = self.fixture()
        p["tracks"][0]["videoType"] = "MUSIC_VIDEO_TYPE_OMV"
        self.assertTrue(
            catalog.build_album("MPRE_test", a, p)["tracks"][0]["unavailable"]
        )

    def test_thriller_deluxe_uses_the_two_studio_recordings(self):
        fixture = json.loads(
            (ROOT / "tests/fixtures/thriller.json").read_text(encoding="utf8")
        )
        album = catalog.build_album(
            fixture["id"], fixture["album"], fixture["playlist"]
        )
        self.assertEqual(
            [
                (t["video_id"], t["duration"], t["unavailable"])
                for t in album["tracks"][3:5]
            ],
            [("bR17f-FpTZA", 358, False), ("OiuN52RM3ao", 259, False)],
        )
        fixture["playlist"]["tracks"][3].update(duration="13:42", duration_seconds=822)
        album = catalog.build_album(
            fixture["id"], fixture["album"], fixture["playlist"]
        )
        self.assertTrue(album["tracks"][3]["unavailable"])

    def test_incomplete_playlist_does_not_change_track_positions(self):
        a, p = self.fixture()
        p["tracks"].pop()
        with self.assertRaises(ValueError):
            catalog.build_album("MPRE_test", a, p)

    def test_significant_parentheses_and_mix_are_preserved(self):
        self.assertNotEqual(catalog.key("Song (Part One)"), catalog.key("Song"))
        self.assertNotEqual(
            catalog.key("Song (2026 Mix)"), catalog.key("Song (2009 Remaster)")
        )

    def test_live_title_filter_does_not_reject_live_and_let_die(self):
        self.assertFalse(catalog.live("Live and Let Die"))
        self.assertTrue(catalog.live("Song (Live at Wembley)"))

    def test_reused_audio_is_retagged_for_destination_and_lyrics_preserved(self):
        from mutagen import File

        self.assertIsNotNone(
            FFMPEG, "FFmpeg debe estar instalado para las pruebas de audio."
        )
        with tempfile.TemporaryDirectory() as temp:
            original = Path(temp) / "first.mp3"
            subprocess.run(
                [
                    FFMPEG,
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:duration=1",
                    "-c:a",
                    "libmp3lame",
                    str(original),
                ],
                check=True,
                creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
            )
            a, p = self.fixture()
            album = catalog.build_album("MPRE_test", a, p)
            t = album["tracks"][0]
            request = dict(
                op="tag",
                path=str(original),
                album=album,
                track=t,
                lyrics="Primera línea\nSegunda línea",
            )
            catalog.tag(request)
            before = original.read_bytes()
            new = Path(temp) / "new.mp3"
            new.write_bytes(before)
            album = copy.deepcopy(album)
            album["title"] = "New compilation"
            t = copy.deepcopy(t)
            t["number"] = 7
            catalog.tag(dict(request, path=str(new), album=album, track=t, lyrics=""))
            self.assertEqual(original.read_bytes(), before)
            tags = File(new)
            self.assertEqual(str(tags["TALB"]), "New compilation")
            self.assertEqual(str(tags["TRCK"]), "7/14")
            self.assertEqual(catalog.embedded(tags), "Primera línea\nSegunda línea")
            self.assertEqual(catalog.inspect_file(new)["source_id"], t["video_id"])

            def pcm(path):
                return subprocess.check_output(
                    [FFMPEG, "-v", "error", "-i", str(path), "-f", "s16le", "-"],
                    creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
                )

            self.assertEqual(pcm(original), pcm(new))

            from mutagen.id3 import TRCK

            tags.tags.add(TRCK(encoding=3, text="desconocido/14"))
            tags.save()
            self.assertEqual(catalog.inspect_file(new)["number"], 0)


if __name__ == "__main__":
    unittest.main()
