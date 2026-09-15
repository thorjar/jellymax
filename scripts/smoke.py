#!/usr/bin/env python3
"""Exercise the real binary, SQLite, FFprobe, and HTTP streaming using disposable media."""
import argparse
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import time
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default="target/debug/jellymax")
    args = parser.parse_args()
    binary = str(Path(args.binary).resolve())
    ffmpeg = shutil.which("ffmpeg")
    ffprobe = shutil.which("ffprobe")
    if not ffmpeg or not ffprobe:
        raise SystemExit("Install FFmpeg/FFprobe before running this smoke test")
    with tempfile.TemporaryDirectory(prefix="jellymax-smoke-") as temporary:
        root = Path(temporary)
        media = root / "media"
        media.mkdir()
        fixture = media / "Smoke.Test.mp4"
        subprocess.run([
            ffmpeg, "-v", "error", "-f", "lavfi", "-i", "color=c=blue:s=160x90:r=10",
            "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=44100", "-t", "1",
            "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", str(fixture),
        ], check=True, timeout=30)
        env = {**os.environ, "JELLYMAX_ADMIN_PASSWORD": "disposable-smoke-password"}
        common = [binary, "--data-dir", str(root / "data")]
        subprocess.run(common + ["create-admin", "--username", "smoke"], env=env,
                       check=True, stdout=subprocess.DEVNULL, timeout=30)
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            port = reservation.getsockname()[1]
        base = f"http://127.0.0.1:{port}"
        token = None

        def request(path, body=None, method=None, headers=None):
            actual_headers = dict(headers or {})
            if token:
                actual_headers["X-Emby-Token"] = token
            if body is not None:
                actual_headers["Content-Type"] = "application/json"
            req = urllib.request.Request(base + path,
                data=json.dumps(body).encode() if body is not None else None,
                headers=actual_headers, method=method)
            return urllib.request.urlopen(req, timeout=10)

        def api(path, body=None, method=None):
            with request(path, body, method) as response:
                content = response.read()
                return json.loads(content) if content else None

        with (root / "server.log").open("w+") as log:
            server = subprocess.Popen(common + ["serve", "--bind", f"127.0.0.1:{port}",
                                      "--ffprobe", ffprobe], stdout=log, stderr=log)
            try:
                for _ in range(100):
                    if server.poll() is not None:
                        log.seek(0)
                        raise RuntimeError(log.read())
                    try:
                        api("/health")
                        break
                    except OSError:
                        time.sleep(0.05)
                else:
                    raise RuntimeError("Server did not become ready")
                session = api("/Users/AuthenticateByName", {"Username": "smoke", "Pw": env["JELLYMAX_ADMIN_PASSWORD"]})
                token = session["AccessToken"]
                api("/Library/VirtualFolders", {"Name": "Smoke", "CollectionType": "movies", "Locations": [str(media)]})
                api("/Library/Refresh", method="POST")
                for _ in range(100):
                    status = api("/ScheduledTasks")[0]
                    if status["State"] != "Running":
                        break
                    time.sleep(0.05)
                assert status["State"] == "Completed", status
                assert status["ProbeFailures"] == 0, status
                items = api("/Items")
                assert items["TotalRecordCount"] == 1, items
                item = items["Items"][0]
                assert item["RunTimeTicks"] > 0, item
                assert {s["Type"] for s in item["MediaStreams"]} >= {"Video", "Audio"}, item
                playback = api(f'/Items/{item["Id"]}/PlaybackInfo')
                with request(playback["MediaSources"][0]["DirectStreamUrl"], headers={"Range": "bytes=0-31"}) as response:
                    assert response.status == 206
                    assert response.read() == fixture.read_bytes()[:32]
                api("/Sessions/Playing/Progress", {"ItemId": item["Id"], "PositionTicks": 5_000_000})
                resumed = api(f'/Users/{session["User"]["Id"]}/Items/Resume')
                assert resumed["TotalRecordCount"] == 1, resumed
                print("PASS: real server startup, login, scan, FFprobe metadata, ranged video streaming, and resume")
            finally:
                server.terminate()
                try:
                    server.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait()


if __name__ == "__main__":
    main()
