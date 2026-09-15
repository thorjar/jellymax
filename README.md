# Jellymax

A runnable Rust media-server backend, developed alongside the existing Jellyfin server. It has its own database and media catalog. It does **not** read or migrate Jellyfin's database, and is not yet a drop-in replacement for Jellyfin clients.

The long-term goal is broad feature coverage for movies, series, and music, excluding live TV. This first implementation delivers the basic path from adding a library to browsing, playing, and resuming media. See [FEATURES.md](FEATURES.md) for what works and what remains; no 90% compatibility claim is made.

For a packaged frontend and backend on one address, see [Docker deployment](DEPLOYMENT.md).

## Run locally

Install a current stable Rust toolchain and FFmpeg (including `ffprobe`). Development was validated with Rust 1.98.0. SQLite is bundled; no database service is required.

```sh
cd jellymax
cargo build --locked
```

On first launch, the bundled web frontend offers a one-time administrator setup form. If you run only the API or need headless setup, `create-admin` remains available. The password must be 12–1024 bytes. For example, in Bash, prompt without putting the password in shell history:

```bash
read -r -s -p 'Admin password: ' JELLYMAX_ADMIN_PASSWORD
export JELLYMAX_ADMIN_PASSWORD
./target/debug/jellymax create-admin --username admin
unset JELLYMAX_ADMIN_PASSWORD
```

Start the server:

```sh
./target/debug/jellymax serve
```

The default address is `http://127.0.0.1:8097`, separate from Jellyfin's usual port. Files are stored in `./data/jellyfin.db`. There is no arbitrary free-space minimum; actual SQLite and filesystem errors still apply.

Configuration examples:

```sh
./target/debug/jellymax --data-dir /path/to/server-data serve \
  --bind 127.0.0.1:8097 --name 'My Media' --ffprobe /path/to/ffprobe

# Allow a separately running development frontend.
./target/debug/jellymax serve --cors-origin http://localhost:5173
```

Equivalent environment variables: `JELLYMAX_DATA_DIR`, `JELLYMAX_BIND`, `JELLYMAX_NAME`, `JELLYMAX_FFPROBE`, and `JELLYMAX_CORS_ORIGIN`. Use the same data directory for `create-admin` and `serve`. The server can start before an administrator exists so the one-time `/System/Setup` route can complete setup. Keep a fresh installation on localhost until setup is complete; use a TLS reverse proxy when making authenticated access available beyond localhost.

HLS transcodes are written only under `<data-dir>/transcodes`. When FFmpeg supports it, encoding starts with an 18-second input burst, then reads at playback speed; older FFmpeg versions use playback speed from the start. Old segments are removed as playback advances. The server stops FFmpeg and clears session files when playback stops or after 60 seconds of inactivity. The total cache has a 512 MiB default budget. Set `JELLYMAX_TRANSCODE_CACHE_MB` (minimum 64) to change it, for example `JELLYMAX_TRANSCODE_CACHE_MB=128` for a small deployment. When the budget is reached, playback returns HTTP 507 instead of continuing to grow the cache. This is an application-level budget checked during encoding, so allow some headroom for bytes written between checks. Old transcodes are cleared when the server starts. Remote Jellyfin servers supply only original static streams; any conversion runs here.

## First API workflow

Routes and JSON use Jellyfin-style names, but only the documented subset is implemented. Administrative mutations accept JSON bodies; they do not reproduce all existing Jellyfin query-string conventions.

1. `POST /Users/AuthenticateByName` with `{"Username":"admin","Pw":"your password"}`. Save `AccessToken` and `User.Id`.
2. Send `X-Emby-Token: <AccessToken>` with subsequent requests. `Authorization: Bearer <AccessToken>` and MediaBrowser authorization headers are also accepted.
3. `POST /Library/VirtualFolders` with `{"Name":"Movies","CollectionType":"movies","Locations":["/absolute/media/path"]}`. Supported types: `movies`, `tvshows`, `music`, `homevideos`. Each library has one root; roots cannot overlap. All users can read every library in this version.
4. `POST /Library/Refresh`, then poll `GET /ScheduledTasks`. Only one scan runs at a time. A `202` response means scheduled, not completed. An incomplete filesystem traversal produces `Failed` and retains unseen catalog entries for that library.
5. `GET /Items?SearchTerm=example&Limit=50`. Use `ParentId` to filter by library ID. `StartIndex`, `IncludeItemTypes`, `IsFavorite`, and `IsPlayed` are supported. Pagination defaults to 100 and is capped at 200.
6. `GET /Items/{id}/PlaybackInfo` to inspect streams, duration, and playback URLs. Compatible MP4/WebM files are served directly with byte ranges; MKV can also play directly when the browser reports MKV support, with HLS fallback if playback fails. Other containers are remuxed when keyframes align with the HLS segment grid; sparse or misaligned keyframes require video conversion so seeking remains reliable. E-AC-3/AC-3/DTS/TrueHD audio is converted to AAC when the browser cannot play it, and unsupported video is converted to H.264/AAC. Managed FFmpeg sessions produce muxed fMP4 HLS fragments, rate-limit encoding after an initial burst, restart near distant seeks, and clean old segments as playback advances.
7. `POST /Sessions/Playing/Progress` or `/Sessions/Playing/Stopped` with `{"ItemId":"...","PositionTicks":50000000}`. One tick is 100 ns. `GET /Users/{userId}/Items/Resume` lists unfinished items with saved progress. Completion is explicit through the played endpoint, not inferred from a percentage.

To connect another Jellyfin installation, open **Administration → Remote servers** in the web frontend and enter its URL and a Jellyfin account. The Rust backend exchanges the password for a Jellyfin access token, imports the account's visible movie, series, season, episode, and music items, and then discards the password. Remote artwork and byte-range media are proxied through this backend, so the remote token is never included in frontend URLs. Use **Sync** after the remote library changes. Browser-compatible remote media direct plays. For incompatible media, this Rust server reads the original stream with `static=true` and transcodes locally; it does not request HLS transcoding from the remote server.

Authentication is header-based. Long-lived tokens in URL query strings and browser cookie authentication are deliberately not implemented. A frontend must make authenticated fetches; the returned media URL alone is not sufficient for a plain HTML `<video src>` element. Playback tickets or browser session authentication are a next-phase requirement for the fresh frontend.

### Additional routes

| Method | Route | Behavior |
| --- | --- | --- |
| GET | `/health`, `/System/Info/Public` | Public health and stable server identity |
| GET | `/Users/Me` | Current user |
| GET / POST | `/Users`, `/Users/New` | Admin user listing / creation; creation takes `Name`, `Password`, optional `IsAdministrator` |
| POST | `/Sessions/Logout` | Revoke current token |
| GET | `/Library/VirtualFolders` | Libraries; filesystem paths visible only to admins |
| DELETE | `/Library/VirtualFolders/{id}` | Admin removes catalog and associated user data; media files remain on disk |
| GET / POST | `/RemoteServers` | Admin lists or connects a remote Jellyfin server; credentials are never returned |
| POST / DELETE | `/RemoteServers/{id}/Sync`, `/RemoteServers/{id}` | Admin reimports or disconnects a remote server |
| GET | `/Items/{id}`, `/Users/{userId}/Items/{id}` | Item detail and current user's data |
| GET | `/Users/{userId}/Items` | Browse current user's items |
| POST / DELETE | `/Users/{userId}/FavoriteItems/{id}` | Set / clear favorite |
| POST / DELETE | `/Users/{userId}/PlayedItems/{id}` | Set / clear watched state |
| GET / HEAD | `/Videos/{id}/stream`, `/Audio/{id}/stream`, `/Items/{id}/Download` | Authenticated original-file streaming |
| GET | `/Items/{id}/Images/Primary` | Local primary artwork |
| GET | `/Items/{id}/Subtitles` | Local sidecar subtitle descriptors |
| GET | `/Items/{id}/Subtitles/{index}` | Browser-ready WebVTT for SRT and embedded text tracks; original VTT sidecars |
| GET / POST | `/Items/{id}/SubtitleSearch`, `/Items/{id}/SubtitleDownload` | Search OpenSubtitles and retrieve a selected SRT file when configured |
| GET / POST | `/Playlists` | List private playlists / create with `Name` and `Ids` array |
| GET / POST | `/Playlists/{id}/Items` | Paged ordered items / append `Ids` array atomically |
| DELETE | `/Playlists/{id}` | Delete own playlist |

Artwork lookup tries `<media-stem>-poster`, `<media-stem>`, `poster`, `folder`, and `cover`, using jpg/jpeg/png/webp. External subtitles match `<media-stem>.srt` or `<media-stem>.<language>.vtt` and equivalent srt/vtt/ass/ssa suffixes. The player offers local sidecars, embedded text subtitles, and original-format SRT/VTT sidecars from connected Jellyfin servers. SRT and other supported text tracks are delivered as WebVTT. The player can also import SRT/VTT files and retain them across reloads in that browser's IndexedDB; imported subtitles are not shared with other devices. To enable **Get more from OpenSubtitles**, set `JELLYMAX_OPENSUBTITLES_API_KEY` in the backend environment before starting the server. The key stays server-side. Downloaded subtitles are also retained in that browser's IndexedDB. Image-based subtitles and burn-in are not supported. Names and suffixes are currently case-sensitive.

## Architecture and operational boundaries

- **Axum + Tokio:** async HTTP routing and streaming. [Axum documentation](https://docs.rs/axum/0.8.9/axum/).
- **SQLite + rusqlite:** foreign keys, WAL, persistent IDs, transactional playlists, and indexes. A single connection is serialized on blocking workers; database work does not block async request threads. Read pools and full-text indexing are future scalability work.
- **Filesystem scanner:** a bounded channel provides backpressure; directory symlinks are not followed. Unchanged size/mtime plus an existing duration skips FFprobe. Changing a file in place preserves its ID; renaming currently creates a new item. Deleted items are pruned only after successful library traversal, cascading their saved user data and playlist entries.
- **FFprobe:** extracts duration and stream information with a 10-second timeout. Missing FFprobe or unprobeable files increase `ProbeFailures`; those items remain browsable and streamable with incomplete metadata. Probe failures do not mean the scan failed to enumerate files.
- **Direct playback:** [tower-http ServeFile](https://docs.rs/tower-http/0.7.1/tower_http/services/struct.ServeFile.html) handles bounded-memory streaming and HTTP range semantics. No media is copied into the data directory.
- **Authentication:** Argon2id passwords; hashed random session tokens with 30-day expiry and logout revocation. Login hashing concurrency is bounded and attempts are globally limited to 30 per minute. This coarse limit is an initial safeguard, not a complete account-lockout system.
- **Path access:** only cataloged files and matching sidecars are served; canonical paths are checked against their permitted directory. Media directories must be trusted against hostile concurrent filesystem mutation; path validation and file opening are not an OS-level filesystem sandbox.
- **Jobs:** one in-process scan, with in-memory status. Scan completion state is lost on restart. Request bodies are capped at 64 KiB. SQL values are bound parameters.

No original Jellyfin server code is called by this service. Existing .NET plugins cannot run here. The source is in the same GPL-2.0 repository; see [LICENSE](LICENSE).

## Verify

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo build --locked
python3 scripts/smoke.py
```

The smoke test generates a one-second video with FFmpeg, starts the real binary on a temporary local port, creates an administrator and library, probes the media, checks byte-range streaming and resume, then stops the process and removes its temporary files. Integration tests use disposable SQLite databases and test authentication, authorization, persistence, scanning, playlists, assets, and streaming. No production media is needed.

## S3-compatible object storage

Administrators can connect private AWS S3 and Cloudflare R2 buckets from **Administration → Object storage**. Each connection becomes a media library and supports direct range streaming, FFmpeg playback, metadata scans, and embedded subtitle extraction without downloading the media into Jellymax's persistent storage. See [DEPLOYMENT.md](DEPLOYMENT.md#aws-s3-and-cloudflare-r2-media) for credentials and endpoint setup.
