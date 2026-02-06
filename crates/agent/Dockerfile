# Build stage
FROM rustlang/rust:nightly-slim as builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    protobuf-compiler \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy manifests
COPY Cargo.toml Cargo.lock ./
COPY build.rs ./

# Copy proto files
COPY proto/ ./proto/

# Copy source code
COPY src/ ./src/

# Build for release
RUN cargo build --release

# Runtime stage - use same base as builder for GLIBC compatibility
FROM debian:sid-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy binary from builder
COPY --from=builder /app/target/release/agent /usr/local/bin/agent

# Create directories for certificates
RUN mkdir -p /etc/docktail/certs

# Set default environment variables for multiline configuration
ENV AGENT_MULTILINE_ENABLED=true
ENV AGENT_MULTILINE_TIMEOUT_MS=300
ENV AGENT_MULTILINE_MAX_LINES=50
ENV AGENT_MULTILINE_REQUIRE_ERROR_ANCHOR=true

# Run as root (standard for Docker management tools)
# Security comes from mTLS and container isolation, not OS user

# Expose gRPC port
EXPOSE 50051

# Health check - verify process is running by checking if port is listening
HEALTHCHECK --interval=10s --timeout=3s --start-period=5s --retries=3 \
    CMD ["sh", "-c", "test -e /proc/1/cmdline && grep -q agent /proc/1/cmdline || exit 1"]

ENTRYPOINT ["/usr/local/bin/agent"]
