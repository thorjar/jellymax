# Docker deployment

The Compose stack builds two images: the Rust API with FFmpeg and a Caddy-served frontend. Caddy sends API and media paths to the Rust service, so the browser uses one origin and needs no CORS configuration. The API is internal to the Compose network; only the frontend is published.

## Install on another server without source code

The repository includes [compose.deploy.yaml](compose.deploy.yaml), which uses container images rather than `build:`. First, publish the images to GitHub Container Registry by running the repository's **Publish Jellymax images** GitHub Actions workflow, or by pushing a tag named `jellymax-v...`. The images are named `ghcr.io/thorjar/jellymax-backend:latest` and `ghcr.io/thorjar/jellymax-frontend:latest`. Make both packages public in GitHub's package settings so another server can pull them without credentials. Until the workflow has completed and the packages are accessible, the deployment Compose file cannot start.

On the other server, copy only `compose.deploy.yaml` as `compose.yaml`, then replace `/srv/media` with the media directory on that server. Add another read-only mount line for each additional folder. No source code, Dockerfiles, or `.env` file is needed for the basic setup. Optional settings and API keys can be placed in a `.env` file beside `compose.yaml`; the Compose variables are forwarded to the backend. Run:

```sh
docker compose up -d
```

Open `http://<server-ip>:8097` and complete the first-run administrator setup. When adding a library, use the path **inside** the container (`/media` for the default mount), not the host path. The `server_data` volume persists the database and artwork. To update later, run `docker compose pull && docker compose up -d`. For reproducible deployments, replace `:latest` in both image names with the same published release tag.

## First run

Install Docker Engine or Docker Desktop with Compose. Run the commands below from `jellymax`. No `.env` file is required for a basic installation. Copy `.env.example` to `.env` only if you want to change the default settings or add optional API keys.

Edit the backend `volumes` in `compose.yaml` and set the media directory on the **Docker host** directly. A single folder containing both movies and series is fine. For example, use `/srv/media:/media:ro`. Add more mount lines if needed, each with a different path inside the container:

```yaml
volumes:
  - server_data:/data
  - /srv/media:/media:ro
  - /mnt/usb1:/media-usb1:ro
```

Start the stack, then open <http://127.0.0.1:8097>:

```sh
docker compose up -d --build
```

On a new data volume, the sign-in page becomes a one-time setup form. Choose an administrator name and a password of at least 12 characters. The form disappears after the first administrator is created. The setup request is rejected after that point. You do not need to put an administrator password in `.env` or run a separate container command.

Create a library using an **inside-container** path such as `/media`, `/media/Movies`, or `/media-usb1`, not the host path. If media cannot be listed, the container's user (UID 10001) needs read and directory-traverse access to the host media tree. Connected Jellyfin servers are configured in the frontend's Administration page. A remote server on the Docker host should use a host-reachable address, not `localhost` (which refers to the container).

`JELLYMAX_TMDB_API_KEY` and `JELLYMAX_OPENSUBTITLES_API_KEY` are optional. Put them in `.env` before `docker compose up -d`; the file is ignored by Git. `JELLYMAX_NAME` changes the displayed server name.

## AWS S3 and Cloudflare R2 media

After the first administrator is created, open **Administration → Object storage**. Choose AWS S3 or Cloudflare R2, enter a bucket and optional folder prefix, and select the library type. Jellymax tests the credentials, scans supported media objects, and adds the bucket as a library. A later **Sync** only probes objects whose size or modification time changed. Disconnecting removes Jellymax's catalog records and never deletes objects from the bucket.

For R2, use `https://<ACCOUNT_ID>.r2.cloudflarestorage.com` as the endpoint and `auto` as the region. Create an R2 API token with Object Read permission for the chosen bucket and use its Access Key ID and Secret Access Key. For AWS S3, leave the endpoint blank, enter the bucket's AWS region, and use credentials with `s3:ListBucket` and `s3:GetObject` permission. Limit credentials to the bucket and prefix Jellymax needs.

Object media is streamed through Jellymax, so the bucket can remain private and browser credentials are never exposed. Direct play uses HTTP byte ranges; FFmpeg reads the same private proxy for remuxing, transcoding, probing, and embedded subtitle extraction. The SQLite database stores the server-side credentials in `/data`, so protect and back up the `server_data` volume accordingly. Media objects are not copied into that volume.

## Storage and exposure

The `server_data` Docker volume persists the SQLite database, artwork, and configuration across container rebuilds. The media bind mount is read-only. Transcode segments use a separate 256 MiB tmpfs mounted over `/data/transcodes`; they do not persist in `server_data`. The application's default 192 MiB transcode cache limit leaves room below that tmpfs ceiling. If you change `JELLYMAX_TRANSCODE_CACHE_MB`, also set `JELLYMAX_TRANSCODE_TMPFS_BYTES` higher than the cache limit in bytes, with headroom for writes between checks. Temporary transcoding consumes memory and may fail when the memory limit is reached.

By default, `JELLYMAX_LISTEN=127.0.0.1:8097` exposes the site only on the Docker host. To access it from another device, put an HTTPS reverse proxy in front of it or change `JELLYMAX_LISTEN` to a host interface such as `0.0.0.0:8097` on a trusted network. Use HTTPS when sending login credentials over a network. This Compose stack itself serves HTTP and does not configure a public TLS certificate.

Complete the administrator setup before exposing a fresh installation beyond the Docker host. Until an administrator exists, the setup form is available to visitors who can reach the site.

An existing non-container database may contain host-absolute library paths that do not exist inside the container. Recreate those libraries with `/media/...` paths after moving; there is no automatic path migration.

## Routine operations

```sh
docker compose logs -f backend frontend
docker compose up -d --build
docker compose down
```

`docker compose down` retains the `server_data` volume. **Do not use `docker compose down -v` unless you intend to delete the database and metadata.** Stop the backend and back up the data volume before upgrades or moves. The database uses SQLite WAL, so copy the entire volume while the server is stopped:

```sh
docker compose stop backend
mkdir -p backup
docker compose cp backend:/data backup/data
docker compose start backend
```

The images include only this `jellymax` directory; they do not depend on the .NET Jellyfin server tree. The frontend is built with a relative API base and works at the same origin as the backend proxy.
