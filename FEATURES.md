# Feature coverage and implementation order

The objective is approximately 90% of the useful non-live-TV behavior, not 90% of C# lines rewritten. Before assigning a percentage, select supported clients and turn each required workflow into an acceptance test. Until then, this is a working first milestone, not a numerical parity claim.

| Capability | Current state | Remaining work |
| --- | --- | --- |
| Server bootstrap and configuration | Working CLI, SQLite, identity, graceful HTTP shutdown | Setup wizard, settings API, backups and restore |
| Authentication | Working users, admins, expiring sessions, logout | Password changes/recovery, disabling/deleting users, device management, granular permissions |
| Libraries | Working local create/list/remove plus imported libraries from existing Jellyfin servers | Multiple local roots, access policies, filesystem watcher, scheduled refresh |
| Scanning | Working recursive scan, stable IDs, incremental probing, conservative pruning | Rename detection, richer naming rules, job cancellation, durable status |
| Movies and home videos | Flat catalog and file-derived names | Metadata providers, NFO, year and version grouping, collections, trailers |
| Television series | Video files classified as Episode | Series/season hierarchy, episode parsing, next-up, specials, multi-episode files |
| Music | Audio scanning and direct streaming | Tags, artists, albums, discs, genres, lyrics, instant mixes |
| Browsing | Detail, search, filters, pagination | Full Jellyfin query model, sorting, recommendations, full-text indexes |
| Artwork | Local primary images | Remote providers, resizing, backdrops, embedded artwork, image caching |
| Direct playback | Original local and remote Jellyfin media with HTTP ranges and HEAD; short-lived browser playback tickets | Remote-server transcode negotiation, detailed device profiles, playback sessions and remote control |
| FFmpeg integration | Metadata probing, direct-play capability selection, persistent per-playback FFmpeg sessions, muxed fMP4 HLS, segment reuse and seek restarts, inactive-session cleanup, E-AC-3/AC-3/DTS/TrueHD to AAC, H.264/AAC fallback transcoding | Hardware acceleration, selectable streams, quality profiles and persistent job reporting |
| Subtitles | Local sidecars, embedded text extraction, remote external SRT/VTT delivery, WebVTT conversion, browser track selection, SRT/VTT import, optional OpenSubtitles search/download | Image-based subtitles, burn-in, provider support without an OpenSubtitles API key |
| Watch state | Per-user position, resume, favorites, explicit played state | Auto-completion policy, play counts, richer history and progress events |
| Playlists | Private ordered lists, transactional create/append/delete | Rename, reorder, remove individual entries, sharing and smart playlists |
| Collections | Not implemented | Movie collections and user collection management |
| Sessions and devices | Authentication sessions only | WebSockets, playback reporting, casting, remote commands, sync play |
| Discovery and networking | Configurable bind address and explicit CORS origin | Discovery protocols, reverse-proxy URL awareness, TLS deployment validation |
| Operations | Health, logs, bounded scan queue, schema-version guard | Metrics, database maintenance, backups, cancellation and job recovery |
| Compatibility | Documented Jellyfin-style route subset | Contract tests against selected real Jellyfin clients; existing plugins need replacements |
| Remote Jellyfin | Admin connection, catalog import, series hierarchy, metadata/artwork proxy, credential-shielded ranged playback, manual resync/disconnect | Remote HLS transcoding proxy, incremental/background sync, remote watch-state reconciliation, multiple remote users |
| Live TV / DVR | Excluded | No implementation planned |

## Next milestones

1. **Complete the browser playback path.** Add adaptive quality selection, stream selection, hardware acceleration, and subtitle conversion. The software HLS remux/transcode path is intended as a compatibility fallback.
2. **Build a rich catalog.** Parse movie years and episode numbering, model series/seasons/albums/artists, import local NFO/tags, then add optional metadata providers with rate limits and cache controls.
3. **Expand user workflows.** Library permissions, account management, next-up, collections, richer playlists, device/session management, durable background tasks, backup and restore.
4. **Prove compatibility and scale.** Run client contract tests and large-library benchmarks, address measured query/I/O bottlenecks, and test Linux/macOS/Windows behavior. Existing tests currently exercise the development platform rather than proving portability.
5. **Build the fresh frontend against tested contracts.** Browser authentication and transcoding must work before claiming general browser playback support. Deliver login, libraries, details, player, subtitles, resume, and admin settings as complete workflows.

A first milestone does not replace a mature Jellyfin deployment. Use a separate data directory and retain the existing server until the required workflows pass migration and playback tests.
