# syntax=docker/dockerfile:1.7

FROM rust:1.97-bookworm AS build
WORKDIR /src
# CI passes the commit, which the log and the game's SystemInfo report.
ARG SBRW_GIT_COMMIT
ARG SBRW_GIT_COMMIT_LONG
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p sbrw \
    && install -D target/release/sbrw /out/sbrw \
    && mkdir -p /out/data

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /out/sbrw /usr/local/bin/sbrw
# Owned by the nonroot user so a fresh named volume inherits writable ownership.
COPY --from=build --chown=65532:65532 /out/data /data
ENV SBRW__SERVER__DATA_DIR=/data
VOLUME /data
EXPOSE 8680/tcp 5222/tcp 9998/udp 9999/udp
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s CMD ["/usr/local/bin/sbrw", "healthcheck"]
ENTRYPOINT ["/usr/local/bin/sbrw"]
CMD ["serve"]
