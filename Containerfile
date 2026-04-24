# Stage 1: Builder
FROM rust:1.85-slim-bookworm AS builder

# Install protobuf compiler required for tonic-build
RUN apt-get update && apt-get install -y protobuf-compiler && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/testudo-raas
COPY . .

# Build for release
RUN cargo build --release

# Stage 2: Execution
FROM gcr.io/distroless/cc-debian12

WORKDIR /app
COPY --from=builder /usr/src/testudo-raas/target/release/testudo-raas /usr/local/bin/testudo-raas

EXPOSE 50051

ENTRYPOINT ["/usr/local/bin/testudo-raas"]
