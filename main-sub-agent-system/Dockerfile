FROM rust:1.82-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY core core
COPY provider provider
COPY agents agents
COPY coordinator coordinator
COPY storage storage
COPY runtime runtime

RUN cargo build --release --bin agent_server

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/agent_server .
COPY config.json .

EXPOSE 3000

CMD ["./agent_server"]
