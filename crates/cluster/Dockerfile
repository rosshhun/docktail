# Build stage
FROM rustlang/rust:nightly-slim as builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    protobuf-compiler \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace manifest
COPY Cargo.toml Cargo.lock ./

# Copy agent crate (workspace dependency - needed for proto builds)
COPY crates/agent/ ./crates/agent/

# Copy cluster crate
COPY crates/cluster/ ./crates/cluster/

# Build for release (only cluster binary)
RUN cargo build --release --package cluster

# Runtime stage
FROM debian:sid-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy binary from builder
COPY --from=builder /app/target/release/cluster /usr/local/bin/cluster

# Create directories for certificates and config
RUN mkdir -p /etc/docktail/certs /etc/docktail/config

# Copy configuration
COPY crates/cluster/config/cluster.toml /etc/docktail/cluster.toml

# Expose GraphQL/HTTP port
EXPOSE 8080

# Health check - verify GraphQL endpoint is responding
HEALTHCHECK --interval=10s --timeout=3s --start-period=5s --retries=3 \
    CMD ["sh", "-c", "test -e /proc/1/cmdline && grep -q cluster /proc/1/cmdline || exit 1"]

ENTRYPOINT ["/usr/local/bin/cluster"]
