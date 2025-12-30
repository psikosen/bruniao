# Build stage
FROM rust:1.74-bookworm as builder

WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Create dummy main to cache dependencies
RUN mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src

# Copy source code
COPY src ./src

# Build the actual application
RUN touch src/main.rs && \
    cargo build --release

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the binary
COPY --from=builder /app/target/release/bruniao /app/bruniao

# Create data directory
RUN mkdir -p /var/lib/bruniao

# Create non-root user
RUN useradd -r -s /bin/false bruniao && \
    chown -R bruniao:bruniao /app /var/lib/bruniao

USER bruniao

ENTRYPOINT ["/app/bruniao"]
