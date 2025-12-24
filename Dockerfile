FROM rust:1.85-slim as builder
WORKDIR /app

# Copy source
COPY . .

# Build deterministically
RUN cargo build --release --workspace --exclude valori-ffi --locked

# Runtime stage
FROM debian:bookworm-slim
WORKDIR /app

# Install standard dependencies (ca-certificates for HTTPS, etc)
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

# Copy the binary
COPY --from=builder /app/target/release/valori-node /usr/local/bin/valori-node

# Expose the port (assuming 3000 based on previous checks, but defaulting to env var)
ENV PORT=3000
ENV VALORI_BIND=0.0.0.0:3000
EXPOSE 3000

CMD ["valori-node"]
