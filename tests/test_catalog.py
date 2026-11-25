import copy
import shutil
import json
import sys
import tempfile
import unittest
import subprocess
from pathlib import Path
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[1]
FFMPEG = (
    str(ROOT / "runtime/ffmpeg.exe")
    if (ROOT / "runtime/ffmpeg.exe").is_file()
    else shutil.which("ffmpeg")
)
sys.path.insert(0, str(ROOT / "bridge"))
import catalog


class ArtistTests(unittest.TestCase):
    def test_untitled_video_carousel_keeps_artist_releases(self):
        from ytmusicapi import YTMusic

        artist_id = "UC_test_artist"

        def release_shelf(category, release_id, title):
            return {
                "musicCarouselShelfRenderer": {
                    "header": {
                        "musicCarouselShelfBasicHeaderRenderer": {
                            "title": {"runs": [{"text": category}]},
                        },
                    },
                    "contents": [
                        {
                            "musicTwoRowItemRenderer": {
                                "title": {
                                    "runs": [
                                        {
                                            "text": title,
                                            "navigationEndpoint": {
                                                "browseEndpoint": {"browseId": release_id},
                                            },
                                        },
                                    ],
                                },
                            },
                        },
                    ],
                },
            }

        shelves = [
            {
                "musicCarouselShelfRenderer": {
                    "contents": [
                        {
                            "musicTwoRowItemRenderer": {
                                "title": {"runs": [{"text": "Watch the new music video"}]},
                            },
                        },
                    ],
                },
            },
            release_shelf("Albums", "MPRE_album", "Test Album"),
            release_shelf("Singles & EPs", "MPRE_single", "Test Single"),
        ]
        response = {
            "header": {
                "musicImmersiveHeaderRenderer": {
                    "title": {"runs": [{"text": "Test Artist"}]},
                    "subscriptionButton": {
                        "subscribeButtonRenderer": {
                            "channelId": artist_id,
                            "subscribed": False,
                        },
                    },
                },
            },
            "contents": {
                "singleColumnBrowseResultsRenderer": {
                    "tabs": [
                        {
                            "tabRenderer": {
                                "content": {"sectionListRenderer": {"contents": shelves}},
                            },
                        },
                    ],
                },
            },
        }
        with patch.object(YTMusic, "_send_request", return_value=response):
            result = catalog.main({"op": "artist", "id": artist_id})

        self.assertEqual(result["title"], "Test Artist")
        self.assertEqual(
            [(row["id"], row["type"]) for row in result["releases"]],
            [("MPRE_album", "Album"), ("MPRE_single", "Single")],
        )

    def test_artist_error_returns_a_readable_message(self):
        with patch.object(catalog, "client", side_effect=KeyError("raw provider data")):
            with self.assertRaisesRegex(ValueError, r"^No se pudo cargar la ficha del artista") as error:
                catalog.main({"op": "artist", "id": "UC_test"})
        self.assertNotIn("raw provider data", str(error.exception))


class LyricsTests(unittest.TestCase):
    def test_search_and_check_the_selected_recording(self):
        api = Mock()
        song = {
            "videoId": "abcdefghijk",
            "title": "Canción",
            "artists": [{"name": "Artista"}],
            "album": {"name": "Álbum"},
        }
        api.search.return_value = [song, song, {"title": "Sin identificador"}]
        api.get_watch_playlist.return_value = {"tracks": [song], "lyrics": "lyric-id"}
        api.get_lyrics.return_value = {"lyrics": "Primera línea\nSegunda línea"}
        with patch.object(catalog, "client", return_value=api):
            results = catalog.main({"op": "search_lyrics", "q": "Canción Artista"})
            self.assertEqual([result["id"] for result in results], ["abcdefghijk"])
            self.assertEqual(results[0]["album"], "Álbum")
            api.search.assert_called_once_with("Canción Artista", filter="songs", limit=20)
            result = catalog.main({"op": "lyrics", "id": results[0]["id"]})
            self.assertEqual(result["lyrics_status"], "Disponible")
            self.assertEqual(result["lyrics"], "Primera línea\nSegunda línea")
            api.get_watch_playlist.assert_called_once_with(videoId="abcdefghijk", limit=1)
            api.get_lyrics.assert_called_once_with("lyric-id")

    def test_missing_lyrics_and_a_failed_lookup_have_distinct_statuses(self):
        api = Mock()
        api.get_lyrics.side_effect = RuntimeError("offline")
        self.assertEqual(
            catalog.track_lyrics(api, {})["lyrics_status"], "No disponible en la fuente"
        )
        self.assertEqual(
            catalog.track_lyrics(api, {"lyrics": "lyric-id"})["lyrics_status"],
            "No se pudo consultar la letra",
        )


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
