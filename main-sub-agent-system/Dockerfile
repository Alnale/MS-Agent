FROM rust:1.82-bookworm AS builder

WORKDIR /app

# Cache dependency builds: copy manifests first, create dummy sources, build deps
COPY Cargo.toml Cargo.lock ./
COPY core/Cargo.toml core/Cargo.toml
COPY provider/Cargo.toml provider/Cargo.toml
COPY toolkit/Cargo.toml toolkit/Cargo.toml
COPY agents/Cargo.toml agents/Cargo.toml
COPY coordinator/Cargo.toml coordinator/Cargo.toml
COPY storage/Cargo.toml storage/Cargo.toml
COPY runtime/Cargo.toml runtime/Cargo.toml

RUN mkdir -p core/src provider/src toolkit/src agents/src coordinator/src storage/src runtime/src/bin && \
    echo "" > core/src/lib.rs && \
    echo "" > provider/src/lib.rs && \
    echo "" > toolkit/src/lib.rs && \
    echo "" > agents/src/lib.rs && \
    echo "" > coordinator/src/lib.rs && \
    echo "" > storage/src/lib.rs && \
    echo 'fn main() {}' > runtime/src/bin/server.rs && \
    cargo build --release --bin agent_server && \
    rm -rf core/src provider/src toolkit/src agents/src coordinator/src storage/src runtime/src

# Copy actual source and rebuild (only changed files trigger recompilation)
COPY core core
COPY provider provider
COPY toolkit toolkit
COPY agents agents
COPY coordinator coordinator
COPY storage storage
COPY runtime runtime

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release --bin agent_server && \
    cp /app/target/release/agent_server /app/agent_server

FROM gcr.io/distroless/cc-debian12

WORKDIR /app
COPY --from=builder /app/agent_server .
COPY config.json .

EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["/app/agent_server", "--health"] || exit 1

CMD ["./agent_server"]
