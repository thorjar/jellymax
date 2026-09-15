FROM rust:1.98-bookworm AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install --no-install-recommends -y ca-certificates curl ffmpeg \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 jellymax \
    && useradd --uid 10001 --gid 10001 --no-create-home --home-dir /data jellymax \
    && mkdir -p /data/transcodes \
    && chown -R jellymax:jellymax /data
COPY --from=build /build/target/release/jellymax /usr/local/bin/jellymax
USER 10001:10001
EXPOSE 8097
ENTRYPOINT ["jellymax", "--data-dir", "/data"]
CMD ["serve", "--bind", "0.0.0.0:8097"]
