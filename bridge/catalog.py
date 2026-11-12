"""Thin provider/tag adapter. Album decisions, jobs and file publication live in Rust.

No search-for-a-replacement function exists here. The only playable IDs are those
in the selected release's audioPlaylistId, at the corresponding track position.
"""

import io
import json
import re
import sys
import unicodedata
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from urllib.parse import urlparse, parse_qs


def key(text):
    return " ".join(
        re.findall(r"\w+", unicodedata.normalize("NFKC", str(text or "")).lower())
    )


def names(rows):
    return ", ".join(x["name"] for x in rows or [] if x.get("name"))


def seconds(text):
    try:
        value = sum(
            float(part) * 60**i for i, part in enumerate(reversed(str(text).split(":")))
        )
        return value if 0 < value < float("inf") else 0
    except (ValueError, TypeError):
        return 0


def image(row):
    return max(row.get("thumbnails") or [{}], key=lambda x: x.get("width", 0)).get(
        "url", ""
    )


def live(title):
    return bool(
        re.search(
            r"(^live$|^live (?:at|in|from|on)\b|\ben vivo\b|\ben directo\b|[\[(]live\b| - live\b)",
            title,
            re.I,
        )
    )


def compact(row):
    return dict(
        id=row.get("browseId", ""),
        title=row.get("title", ""),
        artist=names(row.get("artists")),
        year=str(row.get("year") or ""),
        image=image(row),
        type=row.get("type", "Album"),
    )


def build_album(browse_id, album, playlist):
    original = album.get("tracks", [])
    audio = playlist.get("tracks", [])
    if not original or len(original) != len(audio):
        raise ValueError(
            "La lista de audio no coincide con el número de pistas del álbum. No se buscarán sustitutos."
        )
    if live(album.get("title", "")):
        raise ValueError(
            "Esta publicación está identificada como en vivo. Elige su álbum de estudio."
        )
    tracks = []
    for i, (expected, source) in enumerate(zip(original, audio), 1):
        artist = names(expected.get("artists")) or names(album.get("artists"))
        source_artist = names(source.get("artists"))
        album_duration = seconds(expected.get("duration_seconds")) or seconds(
            expected.get("duration")
        )
        duration = seconds(source.get("duration_seconds")) or seconds(
            source.get("duration")
        )
        reason = ""
        if key(expected.get("title")) != key(source.get("title")) or key(artist) != key(
            source_artist
        ):
            reason = "La posición de la lista de audio no coincide con el título y artista del álbum."
        elif source.get("videoType") != "MUSIC_VIDEO_TYPE_ATV":
            reason = "Esta pista no tiene audio oficial confirmado en la lista del álbum. No se usará el videoclip."
        elif not source.get("isAvailable", True) or not re.fullmatch(
            r"[A-Za-z0-9_-]{11}", source.get("videoId") or ""
        ):
            reason = "El audio de esta pista no está disponible en el catálogo."
        elif live(source.get("title", "")):
            reason = "Esta pista está identificada como grabación en vivo."
        elif album_duration and duration and abs(album_duration - duration) > 3:
            reason = (
                "La duración de la lista de audio difiere de la ficha de esta pista."
            )
        association = source.get("album") or {}
        tracks.append(
            dict(
                number=i,
                disc=expected.get("discNumber") or 1,
                title=expected.get("title", ""),
                artist=artist,
                video_id=source.get("videoId") or "",
                source_type=source.get("videoType") or "",
                source_album=association.get("name", "")
                if isinstance(association, dict)
                else association,
                duration=album_duration or duration,
                unavailable=bool(reason),
                reason=reason,
            )
        )
    return dict(
        id=browse_id,
        playlist_id=album["audioPlaylistId"],
        title=album["title"],
        artist=names(album.get("artists")),
        year=str(album.get("year") or ""),
        image=image(album),
        tracks=tracks,
        other_versions=[
            dict(id=row["browseId"], title=row["title"])
            for row in album.get("other_versions", [])
            if row.get("browseId") and row.get("title")
        ],
    )


def client():
    import truststore

    truststore.inject_into_ssl()
    import requests
    from ytmusicapi import YTMusic

    class Session(requests.Session):
        def request(self, *args, **kwargs):
            kwargs.setdefault("timeout", 25)
            return super().request(*args, **kwargs)

    return YTMusic(language="en", requests_session=Session())


def resolve_id(api, value):
    if re.fullmatch(r"MPRE[\w-]{1,176}", value):
        return value
    parsed = urlparse(value)
    if parsed.scheme != "https" or parsed.hostname not in (
        "music.youtube.com",
        "www.youtube.com",
        "youtube.com",
    ):
        raise ValueError("Pega el enlace del álbum de YouTube Music.")
    if parsed.path.startswith("/browse/"):
        return resolve_id(api, parsed.path.split("/")[-1])
    playlist = parse_qs(parsed.query).get("list", [""])[0]
    if not playlist.startswith("OLAK5uy_"):
        raise ValueError(
            "Ese enlace no es un álbum. Abre su publicación en YouTube Music y copia el enlace del álbum."
        )
    result = api.get_album_browse_id(playlist)
    if not result:
        raise ValueError("No se pudo identificar el álbum del enlace.")
    return result


def embedded(audio):
    from mutagen.mp4 import MP4

    if isinstance(audio, MP4):
        return "\n".join(audio.tags.get("\xa9lyr", [])) if audio.tags else ""
    if audio.tags:
        return "\n".join(
            t.text if isinstance(t.text, str) else "\n".join(t.text)
            for t in audio.tags.getall("USLT")
        )
    return ""


def inspect_file(path):
    from mutagen import File
    from mutagen.mp4 import MP4

    audio = File(path)
    if audio is None:
        raise ValueError("No se pudo leer el audio.")
    tags = audio.tags
    if tags is None:
        return dict(
            duration=audio.info.length,
            lyrics=False,
            source_id="",
            album_id="",
            title="",
            album="",
            artist="",
            album_artist="",
            year="",
            number=0,
        )
    if isinstance(audio, MP4):
        ids = tags.get("----:com.surco:source_id", [])
        source = bytes(ids[0]).decode() if ids else ""
        ids = tags.get("----:com.surco:album_id", [])
        album_id = bytes(ids[0]).decode() if ids else ""
        if not source:
            match = re.search(
                r"[?&]v=([\w-]{11})", " ".join(audio.tags.get("\xa9cmt", []))
            )
            source = match.group(1) if match else ""
    else:
        ids = tags.getall("TXXX:SOURCE_ID")
        source = str(ids[0].text[0]) if ids else ""
        ids = tags.getall("TXXX:ALBUM_ID")
        album_id = str(ids[0].text[0]) if ids else ""
        if not source and audio.tags:
            comments = " ".join(str(t) for t in audio.tags.getall("COMM"))
            match = re.search(r"[?&]v=([\w-]{11})", comments)
            source = match.group(1) if match else ""
    if isinstance(audio, MP4):
        title = (audio.tags.get("\xa9nam") or [""])[0]
        album = (audio.tags.get("\xa9alb") or [""])[0]
        artist = (audio.tags.get("\xa9ART") or [""])[0]
        number = (audio.tags.get("trkn") or [(0, 0)])[0][0]
        album_artist = (audio.tags.get("aART") or [""])[0]
        year = (audio.tags.get("\xa9day") or [""])[0][:4]
    else:
        title = str(audio.tags.get("TIT2", ""))
        album = str(audio.tags.get("TALB", ""))
        artist = str(audio.tags.get("TPE1", ""))
        number = int(str(audio.tags.get("TRCK", "0")).split("/")[0] or 0)
        album_artist = str(audio.tags.get("TPE2", ""))
        year = str(audio.tags.get("TDRC", ""))[:4]
    return dict(
        duration=audio.info.length,
        lyrics=bool(embedded(audio)),
        source_id=source,
        album_id=album_id,
        title=title,
        album=album,
        artist=artist,
        album_artist=album_artist,
        year=year,
        number=number,
    )


def tag(request):
    from mutagen import File
    from mutagen.mp4 import MP4, MP4Cover, MP4FreeForm
    from mutagen.id3 import TIT2, TPE1, TPE2, TALB, TRCK, TPOS, TDRC, USLT, APIC, TXXX

    audio = File(request["path"])
    if audio is None:
        raise ValueError("El archivo preparado no contiene audio legible.")
    if audio.tags is None:
        audio.add_tags()
    album = request["album"]
    track = request["track"]
    lyric = embedded(audio) or request.get("lyrics", "")
    cover = Path(request.get("cover", ""))
    cover_data = cover.read_bytes() if cover.is_file() else None
    if isinstance(audio, MP4):
        audio["\xa9nam"] = [track["title"]]
        audio["\xa9ART"] = [track["artist"]]
        audio["aART"] = [album["artist"]]
        audio["\xa9alb"] = [album["title"]]
        audio["\xa9day"] = [album["year"]]
        audio["trkn"] = [(track["number"], len(album["tracks"]))]
        audio["disk"] = [(track["disc"], 0)]
        audio["----:com.surco:source_id"] = [MP4FreeForm(track["video_id"].encode())]
        audio["----:com.surco:album_id"] = [MP4FreeForm(album["id"].encode())]
        audio["----:com.surco:source_album"] = [
            MP4FreeForm(track["source_album"].encode())
        ]
        if lyric:
            audio["\xa9lyr"] = [lyric]
        if cover_data:
            audio["covr"] = [MP4Cover(cover_data, imageformat=MP4Cover.FORMAT_JPEG)]
    else:
        for tag_name in (
            "TIT2",
            "TPE1",
            "TPE2",
            "TALB",
            "TRCK",
            "TPOS",
            "TDRC",
            "USLT",
            "APIC",
        ):
            audio.tags.delall(tag_name)
        for frame, value in (
            (TIT2, track["title"]),
            (TPE1, track["artist"]),
            (TPE2, album["artist"]),
            (TALB, album["title"]),
            (TRCK, f"{track['number']}/{len(album['tracks'])}"),
            (TPOS, str(track["disc"])),
            (TDRC, album["year"]),
        ):
            audio.tags.add(frame(encoding=3, text=value))
        for name, value in (
            ("SOURCE_ID", track["video_id"]),
            ("ALBUM_ID", album["id"]),
            ("SOURCE_ALBUM", track["source_album"]),
        ):
            audio.tags.delall("TXXX:" + name)
            audio.tags.add(TXXX(encoding=3, desc=name, text=value))
        if lyric:
            audio.tags.add(USLT(encoding=3, lang="und", desc="", text=lyric))
        if cover_data:
            audio.tags.add(
                APIC(
                    encoding=3, mime="image/jpeg", type=3, desc="Cover", data=cover_data
                )
            )
    audio.save()
    return dict(
        lyrics="Incorporada"
        if lyric
        else request.get("lyrics_status", "No disponible en la fuente"),
        duration=audio.info.length,
    )


def search_catalog(api, query):
    if query.startswith("https://"):
        bid = resolve_id(api, query)
        return {"artists": [], "releases": [dict(compact(api.get_album(bid)), id=bid)]}

    with ThreadPoolExecutor(max_workers=2) as pool:
        artists = pool.submit(api.search, query, filter="artists", limit=12)
        releases = api.search(query, filter="albums", limit=30)
        return {
            "artists": [
                {"id": row["browseId"], "title": row["artist"], "image": image(row)}
                for row in artists.result()
                if row.get("browseId") and row.get("artist")
            ],
            "releases": [
                compact(row)
                for row in releases
                if row.get("browseId") and not live(row.get("title", ""))
            ],
        }


def artist_catalog(api, artist_id):
    artist = api.get_artist(artist_id)
    name = artist["name"]

    def releases(section, default_type):
        shelf = artist.get(section) or {}
        preview = shelf.get("results") or []
        rows = preview
        if shelf.get("browseId") and shelf.get("params"):
            rows = api.get_artist_albums(shelf["browseId"], shelf["params"], limit=None)
        types = {
            row["browseId"]: row["type"]
            for row in preview
            if row.get("browseId") and row.get("type")
        }
        return [
            dict(
                compact(row),
                artist=names(row.get("artists")) or name,
                type=row.get("type") or types.get(row["browseId"]) or default_type,
            )
            for row in rows
            if row.get("browseId", "").startswith("MPRE")
            and not live(row.get("title", ""))
        ]

    shelf = artist.get("songs") or {}
    with ThreadPoolExecutor(max_workers=3) as pool:
        albums = pool.submit(releases, "albums", "Album")
        singles = pool.submit(releases, "singles", "Single")
        rows = shelf.get("results") or []
        if shelf.get("browseId"):
            rows = api.get_playlist(shelf["browseId"].removeprefix("VL"), limit=None)[
                "tracks"
            ]
        publications = {row["id"]: row for row in albums.result() + singles.result()}

    songs = {}
    for row in rows:
        release = row.get("album")
        video_id = row.get("videoId") or ""
        title = row.get("title") or ""
        if (
            not isinstance(release, dict)
            or not (release.get("id") or "").startswith("MPRE")
            or row.get("videoType") != "MUSIC_VIDEO_TYPE_ATV"
            or not row.get("isAvailable", True)
            or not title
            or live(title)
            or not re.fullmatch(r"[A-Za-z0-9_-]{11}", video_id)
        ):
            continue
        songs[(release["id"], video_id)] = {
            "id": video_id,
            "title": title,
            "album_id": release["id"],
            "album": release.get("name") or "",
            "artist": names(row.get("artists")) or name,
            "image": image(row),
            "duration": seconds(row.get("duration_seconds") or row.get("duration")),
        }
    return {
        "id": artist_id,
        "title": name,
        "image": image(artist),
        "songs": list(songs.values()),
        "releases": list(publications.values()),
    }


def main(request):
    op = request["op"]
    if op == "inspect":
        return inspect_file(request["path"])
    if op == "tag":
        return tag(request)
    if op == "cover":
        import truststore

        truststore.inject_into_ssl()
        import requests
        from PIL import Image

        host = urlparse(request["url"]).hostname or ""
        if not any(
            host == h or host.endswith("." + h)
            for h in ("ytimg.com", "googleusercontent.com", "ggpht.com")
        ):
            raise ValueError("Servidor de portada no reconocido.")
        r = requests.get(request["url"], timeout=20)
        r.raise_for_status()
        Image.open(io.BytesIO(r.content)).convert("RGB").save(request["path"], "JPEG")
        return {"ok": True}
    api = client()
    if op == "search":
        return search_catalog(api, request["q"])
    if op == "artist":
        return artist_catalog(api, request["id"])
    if op == "album":
        bid = resolve_id(api, request["id"])
        a = api.get_album(bid)
        playlist = a.get("audioPlaylistId", "")
        if not playlist.startswith("OLAK5uy_"):
            raise ValueError(
                "El catálogo no publicó una lista de audio para este álbum."
            )
        return build_album(bid, a, api.get_playlist(playlist, limit=None))
    if op == "source":
        data = api.get_watch_playlist(videoId=request["id"], limit=1)
        row = next(
            (t for t in data.get("tracks", []) if t.get("videoId") == request["id"]),
            None,
        )
        if not row:
            raise ValueError(
                "La fuente de audio ya no está disponible. No se elegirá otra grabación."
            )
        lyric = ""
        lyric_status = "No disponible en la fuente"
        if data.get("lyrics"):
            try:
                result = api.get_lyrics(data["lyrics"]) or {}
                lyric = result.get("lyrics") or ""
            except Exception:
                lyric_status = "No se pudo consultar; el audio se conserva"
        return dict(
            video_id=row["videoId"],
            source_type=row.get("videoType", ""),
            title=row.get("title", ""),
            artist=names(row.get("artists")),
            duration=seconds(row.get("length")),
            lyrics=lyric,
            lyrics_status=lyric_status,
        )
    raise ValueError("Operación no reconocida.")


if __name__ == "__main__":
    sys.stdin.reconfigure(encoding="utf-8")
    sys.stdout.reconfigure(encoding="utf-8")
    try:
        print(
            json.dumps(
                {"ok": True, "data": main(json.load(sys.stdin))}, ensure_ascii=False
            )
        )
    except Exception as exc:
        print(json.dumps({"ok": False, "error": str(exc)}, ensure_ascii=False))
        sys.exit(1)
