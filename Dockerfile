FROM rust:1.97.1-bookworm AS builder

WORKDIR /src
COPY . .
ARG MAW_BUILD_VERSION=container
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    MAW_BUILD_VERSION="$MAW_BUILD_VERSION" \
    cargo build --release --locked --bin maw-rs --features wasm-host \
    && install -Dm755 target/release/maw-rs /out/maw

FROM debian:bookworm-slim AS runtime
LABEL org.opencontainers.image.source="https://github.com/Soul-Brews-Studio/maw-rs"

RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
        ca-certificates curl git libgcc-s1 lsof openssh-client procps sqlite3 tmux \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --shell /bin/bash maw \
    && install -d -o maw -g maw /repos /home/maw/.config/maw \
        /home/maw/.local/share/maw /home/maw/.local/state/maw \
        /home/maw/.cache/maw
COPY --from=builder /out/maw /usr/local/bin/maw

ENV HOME=/home/maw MAW_XDG=1 MAW_PORT=3456 GHQ_ROOT=/repos \
    XDG_CONFIG_HOME=/home/maw/.config \
    XDG_DATA_HOME=/home/maw/.local/share \
    XDG_STATE_HOME=/home/maw/.local/state \
    XDG_CACHE_HOME=/home/maw/.cache
USER maw
WORKDIR /repos
ENTRYPOINT ["maw"]
CMD ["serve", "--host", "0.0.0.0", "--port", "3456"]
