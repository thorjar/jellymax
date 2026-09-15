# Jellymax frontend

A React + TypeScript single-page app for the **jellymax** media server
backend. It speaks the documented Jellyfin-style HTTP subset implemented by the
Rust server (see the repository `README.md`).

For a production build served together with the backend, see [Docker deployment](../DEPLOYMENT.md).

## Stack

- [Vite](https://vitejs.dev/) dev server and build
- [React](https://react.dev/) 18 + [react-router-dom](https://reactrouter.com/) 6
- TypeScript (strict)

## Features

- Sign in via `POST /Users/AuthenticateByName`; the access token is stored and
  sent on every request as the `X-Emby-Token` header.
- Browse libraries and their media, with pagination and per-library filtering.
- Global search (`GET /Items?SearchTerm=`).
- Item detail with local artwork, stream metadata, favorite / watched toggles,
  and "add to playlist".
- **Playback** using the backend's short-lived playback tickets so a plain
  `<video>`/`<audio>` element can play media without sending auth headers.
  Position is reported to `/Sessions/Playing/Progress` periodically and on
  stop, which feeds the **Resume** list (`/Users/{id}/Items/Resume`).
- Private playlists: create, list, view, append items, delete.
- Admin area (administrators only): create libraries (picking the media folder
  directly from the server's filesystem via an admin-only directory browser),
  manage users, and run / monitor the library scan
  (`POST /Library/Refresh` + `GET /ScheduledTasks`).

## Run locally

Start the Rust backend first (from the repository root):

```sh
cargo build --locked
./target/debug/jellymax serve \
  --bind 127.0.0.1:8097 --cors-origin http://localhost:5173
```

Then start the frontend:

```sh
cd frontend
npm install
npm run dev
```

Open <http://localhost:5173>. In development the Vite config proxies `/Users`,
`/Items`, `/Sessions`, `/Videos`, `/Audio`, `/Library`, `/ScheduledTasks`, and
`/Playlists` to the backend at `http://127.0.0.1:8097`, so no CORS is needed.
Override the backend target with the `VITE_API_TARGET` environment variable.
On a new data directory the login page prompts you to create the first administrator.

### Connecting to a remote backend without the proxy

If you run the built app against a backend that is not on the proxy host, set
`VITE_API_BASE` to the server origin (for example
`http://192.168.1.10:8097`) and make sure that backend was started with CORS
enabled for your frontend origin, e.g.:

```sh
./target/debug/jellymax serve --cors-origin https://your-app.example
```

`VITE_API_BASE` and `VITE_API_TARGET` are compile-time variables; set them
before running `npm run dev` / `npm run build`.

## Scripts

- `npm run dev` — start the Vite dev server (port 5173) with the API proxy.
- `npm run build` — type-check with `tsc` and produce a production build in `dist/`.
- `npm run preview` — serve the production build locally.
- `npm run typecheck` — run the TypeScript compiler only.

## Notes on playback

The backend intentionally does not implement browser cookie authentication, so
a media URL alone is not playable in a plain `<img>`/`<video>` tag. This app
handles that in two ways:

- `ItemImage` fetches artwork with the auth header and renders a blob URL.
- The `Player` component requests `/Items/{id}/PlaybackInfo`, which returns a
  `DirectStreamUrl` carrying a short-lived `PlaybackTicket`, and points the
  media element at that URL. External subtitles from the same response are
  wired to `<track>` elements.
