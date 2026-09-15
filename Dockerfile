FROM rust:1.98-bookworm AS chef
RUN cargo install cargo-chef --locked --version 0.1.73
WORKDIR /build

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --release --locked --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

FROM node:22-bookworm-slim AS web
WORKDIR /web
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/index.html frontend/tsconfig*.json frontend/vite.config.ts ./
COPY frontend/src ./src
COPY frontend/public ./public
RUN npm run build

FROM debian:trixie-slim
RUN apt-get update     && apt-get install --no-install-recommends -y ca-certificates curl ffmpeg     && rm -rf /var/lib/apt/lists/*     && groupadd --gid 10001 jellymax     && useradd --uid 10001 --gid 10001 --no-create-home --home-dir /data jellymax     && mkdir -p /data/transcodes     && chown -R jellymax:jellymax /data
COPY --from=build /build/target/release/jellymax /usr/local/bin/jellymax
COPY --from=web /web/dist /srv
ENV JELLYMAX_WEB_DIR=/srv
USER 10001:10001
EXPOSE 8097
ENTRYPOINT ["jellymax", "--data-dir", "/data"]
CMD ["serve", "--bind", "0.0.0.0:8097"]
