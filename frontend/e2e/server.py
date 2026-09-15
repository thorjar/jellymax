"""Disposable real backend/media fixture for browser tests (no TMDB credentials needed)."""
import json
import os
from pathlib import Path
import shutil
import signal
import sqlite3
import subprocess
import tempfile
import threading
import time
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit

root = Path(__file__).resolve().parents[2]
subprocess.run(["cargo", "build", "--locked"], cwd=root, check=True)
with tempfile.TemporaryDirectory(prefix="jellymax-browser-") as temporary:
    folder = Path(temporary)
    media = folder / "movies"
    media.mkdir()
    clip = media / "Example.Movie.2020.mp4"
    subprocess.run(["ffmpeg", "-v", "error", "-f", "lavfi", "-i", "color=c=blue:s=160x90:r=10", "-t", "3", "-c:v", "libx264", "-pix_fmt", "yuv420p", str(clip)], check=True)
    (media / "Example.Movie.2020.en.srt").write_text("1\n00:00:00,000 --> 00:00:02,500\nA sample subtitle\n\n2\n00:00:02,600 --> 00:00:02,900\nFinal subtitle\n", encoding="utf-8")
    embedded_clip = media / "Embedded.Subtitles.mkv"
    subprocess.run(["ffmpeg", "-v", "error", "-i", str(clip), "-i", str(media / "Example.Movie.2020.en.srt"), "-map", "0:v:0", "-map", "1:0", "-c", "copy", "-c:s", "srt", str(embedded_clip)], check=True)
    windowed_subtitles = media / "Windowed.Subtitles.mkv"
    windowed_srt = folder / "windowed.srt"
    windowed_srt.write_text("1\n00:00:01,000 --> 00:00:04,000\nOpening caption\n\n2\n00:00:48,000 --> 00:00:52,000\nCaption after seek\n\n3\n00:01:20,000 --> 00:01:24,000\nCaption in former gap\n", encoding="utf-8")
    subprocess.run(["ffmpeg", "-v", "error", "-f", "lavfi", "-i", "color=c=teal:s=160x90:r=10", "-f", "lavfi", "-i", "sine=frequency=550:sample_rate=48000", "-i", str(windowed_srt), "-t", "100", "-map", "0:v:0", "-map", "1:a:0", "-map", "2:s:0", "-c:v", "libx264", "-preset", "ultrafast", "-c:a", "aac", "-c:s", "srt", str(windowed_subtitles)], check=True)
    alternate_srt = folder / "alternate.srt"
    alternate_srt.write_text("1\n00:00:01,000 --> 00:00:04,000\nSecond track caption\n", encoding="utf-8")
    subprocess.run(["ffmpeg", "-v", "error", "-i", str(windowed_subtitles), "-i", str(alternate_srt),
                    "-map", "0:v:0", "-map", "0:a:0", "-map", "0:s:0", "-map", "1:s:0", "-c", "copy",
                    "-c:s", "srt", "-metadata:s:s:0", "language=eng", "-disposition:s:0", "default",
                    str(media / "Dual.Subtitles.mkv")], check=True)
    transcode_clip = media / "Needs.Transcode.mkv"
    subprocess.run(["ffmpeg", "-v", "error", "-f", "lavfi", "-i", "color=c=red:s=160x90:r=10", "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000", "-t", "13", "-c:v", "mpeg4", "-c:a", "aac", str(transcode_clip)], check=True)
    (media / "Needs.Transcode.en.srt").write_text("1\n00:00:00,000 --> 00:00:05,000\nHLS subtitle\n\n2\n00:00:10,000 --> 00:00:12,500\nSubtitle after seek\n", encoding="utf-8")
    eac3_clip = media / "Chrome.Audio.mkv"
    subprocess.run(["ffmpeg", "-v", "error", "-f", "lavfi", "-i", "color=c=green:s=160x90:r=10", "-f", "lavfi", "-i", "sine=frequency=660:sample_rate=48000", "-t", "13", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "eac3", "-ac", "6", str(eac3_clip)], check=True)
    mixed_clip = media / "Mixed.Codecs.mkv"
    subprocess.run(["ffmpeg", "-v", "error", "-f", "lavfi", "-i", "color=c=purple:s=160x90:r=10", "-f", "lavfi", "-i", "sine=frequency=770:sample_rate=48000", "-t", "13", "-c:v", "libx265", "-preset", "ultrafast", "-x265-params", "log-level=error", "-pix_fmt", "yuv420p", "-c:a", "eac3", "-ac", "6", str(mixed_clip)], check=True)
    sparse_clip = media / "Sparse.Keyframes.mkv"
    subprocess.run(["ffmpeg", "-v", "error", "-f", "lavfi", "-i", "testsrc2=size=160x90:rate=10", "-t", "45", "-c:v", "libx264", "-preset", "ultrafast", "-g", "100", "-keyint_min", "100", "-sc_threshold", "0", str(sparse_clip)], check=True)
    tv = folder / "series"
    for relative in ["Example Show/Season 1/S01E02.mp4", "Example Show/Season 1/S01E10.mp4", "Example Show/Season 2/S02E01.mp4"]:
        destination = tv / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(clip, destination)
    data = folder / "data"
    class SubtitleProvider(BaseHTTPRequestHandler):
        def respond(self, body, content_type="application/json"):
            data = body.encode()
            self.send_response(200)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def do_GET(self):
            if urlsplit(self.path).path == "/subtitles" and self.headers.get("Api-Key") == "browser-test-key":
                if "languages=zz" in self.path:
                    body = json.dumps({"message": "API key not authorized for search"}).encode()
                    self.send_response(401)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(body)))
                    self.end_headers()
                    self.wfile.write(body)
                    return
                self.respond(json.dumps({"data": [{"attributes": {"language": "en", "download_count": 12,
                    "hearing_impaired": False, "files": [{"file_id": 42, "file_name": "Example.en.srt"},
                    {"file_id": 401, "file_name": "Denied.en.srt"}]}}]}))
            elif urlsplit(self.path).path == "/downloaded.srt":
                self.respond("1\n00:00:01,000 --> 00:00:05,000\nDownloaded caption\n", "text/plain")
            else:
                self.send_error(404)

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers.get("Content-Length", "0"))))
            if self.path == "/download" and body.get("file_id") == 401:
                data = json.dumps({"message": "Account download access denied"}).encode()
                self.send_response(401)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)
                return
            if self.path == "/download" and body.get("file_id") == 42 and self.headers.get("Api-Key") == "browser-test-key":
                self.respond(json.dumps({"link": "http://127.0.0.1:18098/downloaded.srt"}))
            else:
                self.send_error(404)

        def log_message(self, *_):
            pass

    subtitle_provider = ThreadingHTTPServer(("127.0.0.1", 18098), SubtitleProvider)
    threading.Thread(target=subtitle_provider.serve_forever, daemon=True).start()
    remote_mkv = media / "Dual.Subtitles.mkv"
    class RemoteJellyfin(BaseHTTPRequestHandler):
        def do_OPTIONS(self):
            self.send_response(204)
            self.send_header("Access-Control-Allow-Origin", "*")
            self.send_header("Access-Control-Allow-Headers", "X-Emby-Token")
            self.send_header("Access-Control-Allow-Methods", "GET, OPTIONS")
            self.end_headers()

        def do_GET(self):
            route = urlsplit(self.path)
            if route.path == "/streamed-vtt":
                self.send_response(200)
                self.send_header("Content-Type", "text/vtt; charset=utf-8")
                self.send_header("Access-Control-Allow-Origin", "*")
                self.end_headers()
                self.wfile.write(b"WEBVTT\n\n00:00:01.000 --> 00:00:04.000\nFirst streamed subtitle\n\n")
                self.wfile.flush()
                time.sleep(2)
                try:
                    self.wfile.write(b"00:00:05.000 --> 00:00:08.000\nSecond streamed subtitle\n\n")
                    self.wfile.flush()
                except (BrokenPipeError, ConnectionResetError):
                    pass
            elif route.path == "/Items/remote-subtitle/PlaybackInfo":
                payload = json.dumps({"MediaSources": [
                    {"Id": "wrong-source", "MediaStreams": [{"Type": "Subtitle", "Index": 2, "Codec": "ass"}]},
                    {"Id": "remote-media-source", "MediaStreams": [
                        {"Type": "Subtitle", "Index": 2, "Codec": "subrip", "IsExternal": True},
                        {"Type": "Subtitle", "Index": 4, "Codec": "subrip", "IsExternal": False},
                        {"Type": "Subtitle", "Index": 5, "Codec": "subrip", "IsExternal": False}]}
                ]}).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)
            elif route.path == "/Videos/remote-subtitle/stream":
                size = remote_mkv.stat().st_size
                start, end = 0, size - 1
                requested_range = self.headers.get("Range")
                if requested_range and requested_range.startswith("bytes="):
                    parts = requested_range[6:].split("-", 1)
                    start = int(parts[0]) if parts[0] else 0
                    end = min(int(parts[1]), end) if parts[1] else end
                if start > end or start >= size:
                    self.send_error(416)
                    return
                self.send_response(206 if requested_range else 200)
                self.send_header("Content-Type", "video/x-matroska")
                self.send_header("Content-Length", str(end - start + 1))
                self.send_header("Accept-Ranges", "bytes")
                if requested_range:
                    self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
                self.end_headers()
                with remote_mkv.open("rb") as source:
                    source.seek(start)
                    remaining = end - start + 1
                    while remaining:
                        chunk = source.read(min(65536, remaining))
                        if not chunk:
                            break
                        try:
                            self.wfile.write(chunk)
                        except (BrokenPipeError, ConnectionResetError):
                            break
                        remaining -= len(chunk)
            else:
                self.send_error(404)

        def log_message(self, *_):
            pass

    remote_jellyfin = ThreadingHTTPServer(("127.0.0.1", 18099), RemoteJellyfin)
    threading.Thread(target=remote_jellyfin.serve_forever, daemon=True).start()
    command = [str(root / "target/debug/jellymax"), "--data-dir", str(data)]
    env = {**os.environ, "JELLYMAX_ADMIN_PASSWORD": "browser-test-password", "JELLYMAX_TMDB_API_KEY": "",
           "JELLYMAX_OPENSUBTITLES_API_KEY": "browser-test-key", "JELLYMAX_OPENSUBTITLES_TEST_BASE": "http://127.0.0.1:18098"}
    subprocess.run(command + ["create-admin", "--username", "browser"], env=env, check=True)
    process = subprocess.Popen(command + ["serve", "--bind", "127.0.0.1:18097"], env=env)
    token = None

    def api(path, body=None, method=None):
        headers = {"Content-Type": "application/json"}
        if token:
            headers["X-Emby-Token"] = token
        req = urllib.request.Request("http://127.0.0.1:18097" + path, data=json.dumps(body).encode() if body is not None else None, headers=headers, method=method)
        with urllib.request.urlopen(req, timeout=10) as response:
            content = response.read()
            return json.loads(content) if content else None

    def stop(*_):
        raise KeyboardInterrupt

    signal.signal(signal.SIGTERM, stop)
    try:
        for _ in range(100):
            try:
                api("/health")
                break
            except OSError:
                if process.poll() is not None:
                    raise RuntimeError("Backend exited during startup")
                time.sleep(0.05)
        token = api("/Users/AuthenticateByName", {"Username": "browser", "Pw": "browser-test-password"})["AccessToken"]
        for name, kind, directory in [("Movies", "movies", media), ("Series", "tvshows", tv)]:
            api("/Library/VirtualFolders", {"Name": name, "CollectionType": kind, "Locations": [str(directory)]})
        api("/Library/Refresh", method="POST")
        for _ in range(200):
            result = api("/ScheduledTasks")[0]
            if result["State"] != "Running":
                assert result["State"] == "Completed", result
                break
            time.sleep(0.05)
        else:
            raise RuntimeError("Library scan timed out")
        with sqlite3.connect(data / "jellyfin.db") as db:
            db.execute("UPDATE items SET name='Example Movie',tmdb_id='42',overview='Movie description',year=2020,genres='[\"Drama\"]',rating=8 WHERE kind='Movie' AND path LIKE '%Example.Movie.2020.mp4'")
            db.execute("UPDATE items SET name='Needs Transcode' WHERE kind='Movie' AND path LIKE '%Needs.Transcode.mkv'")
            db.execute("UPDATE items SET name='Chrome Audio' WHERE kind='Movie' AND path LIKE '%Chrome.Audio.mkv'")
            db.execute("UPDATE items SET name='Mixed Codecs' WHERE kind='Movie' AND path LIKE '%Mixed.Codecs.mkv'")
            remote_streams = json.loads(subprocess.check_output(["ffprobe", "-v", "error", "-show_streams", "-of", "json", str(remote_mkv)]))["streams"]
            streams = [{"Index": stream["index"], "Type": stream["codec_type"].capitalize(), "Codec": stream["codec_name"],
                        "Language": stream.get("tags", {}).get("language"), "IsDefault": stream.get("disposition", {}).get("default") == 1,
                        "IsExternal": False} for stream in remote_streams]
            for stream in streams:
                if stream["Type"] == "Subtitle":
                    stream["Index"] += 2
                    if stream["Index"] == 5:
                        stream["Language"] = "eng"
                        stream["Title"] = "SDH"
            streams.insert(2, {"Index": 2, "Type": "Subtitle", "Codec": "subrip", "Language": "eng", "IsExternal": True})
            db.execute("INSERT INTO remote_servers(id,name,base_url,server_id,user_id,access_token,device_id) VALUES ('browser-remote','Mock Jellyfin','http://127.0.0.1:18099','upstream','remote-user','secret','browser-device')")
            db.execute("INSERT INTO libraries(id,name,path,kind,remote_server_id,remote_item_id) VALUES ('browser-remote-library','Remote Movies','remote://browser/movies','movies','browser-remote','remote-view')")
            db.execute("INSERT INTO items(id,library_id,path,name,kind,container,size,modified,runtime_ticks,media_streams,scan_id,remote_server_id,remote_item_id) VALUES ('browser-remote-mkv','browser-remote-library','remote://browser/movie','Remote Dual Subtitles','Movie','mkv',?,0,1000000000,?,'browser','browser-remote','remote-subtitle')", (remote_mkv.stat().st_size, json.dumps(streams)))
            db.execute("UPDATE items SET tmdb_id='100',overview='Series description' WHERE kind='Series'")
            db.execute("UPDATE items SET name='Episode Two',tmdb_id='102',overview='Episode description' WHERE kind='Episode' AND parent_index_number=1 AND index_number=2")
        # Playwright waits for this marker, so tests cannot race fixture creation.
        (root / "frontend" / ".e2e-ready").write_text("ready")
        process.wait()
    except KeyboardInterrupt:
        pass
    finally:
        subtitle_provider.shutdown()
        subtitle_provider.server_close()
        remote_jellyfin.shutdown()
        remote_jellyfin.server_close()
        (root / "frontend" / ".e2e-ready").unlink(missing_ok=True)
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
