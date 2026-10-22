FROM rust:bookworm AS chef
WORKDIR /build
RUN cargo install cargo-chef --locked

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    curl \
    unzip \
    cmake \
    perl \
    clang \
    libclang-dev \
    && rm -rf /var/lib/apt/lists/*

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /build/recipe.json recipe.json
COPY scripts/fast-linker.sh scripts/fast-linker.sh

ENV SQLX_OFFLINE=true

RUN cargo chef cook --release --recipe-path recipe.json

ARG GIT_SHA=""
ENV GIT_SHA=$GIT_SHA

COPY . .

RUN cargo build --release -p kani-cli \
    && ./target/release/kani-cli setup

RUN cargo build --release -p kani-web

FROM debian:bookworm-slim AS runtime

ARG INSTALL_KCC=false

RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl3 \
    ca-certificates \
    curl \
    nodejs \
    && if [ "$INSTALL_KCC" = "true" ]; then \
        apt-get install -y --no-install-recommends python3 python3-pip p7zip-full \
        && pip3 install --no-cache-dir --break-system-packages KindleComicConverter; \
    fi \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd -g 1000 kani && useradd -u 1000 -g kani -d /app -M kani

WORKDIR /app
COPY --from=builder --chown=kani:kani /build/target/release/kani-web ./kani-web
COPY --chown=kani:kani entrypoint.sh ./entrypoint.sh
RUN chmod +x ./entrypoint.sh

RUN mkdir -p /data /library && chown kani:kani /data /library

WORKDIR /data

EXPOSE 8242

HEALTHCHECK --interval=30s --timeout=10s --start-period=15s --retries=3 \
    CMD curl -f http://localhost:8242/health || exit 1

ENV KANI_BIND=0.0.0.0:8242
ENV KANI_LIBRARY_DIR=/library

CMD ["/app/entrypoint.sh"]
