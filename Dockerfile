# Start from the latest Rust image for the build stage
FROM rust:latest AS builder

# Set the working directory
WORKDIR /app

# Copy the Cargo.toml and Cargo.lock files
COPY ./Cargo.toml ./Cargo.toml
COPY ./Cargo.lock ./Cargo.lock

# Copy the source code
COPY ./src ./src

# Build the application
RUN cargo build 

# Use Ubuntu 22.04 for runtime to ensure OpenSSL 3.x is available
FROM ubuntu:22.04

# Install OpenSSL and necessary libraries
RUN apt-get update && apt-get install -y \
    openssl \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Copy the built binary from the builder stage
COPY --from=builder /app/target/debug/preconf-boost /usr/local/bin/preconf-boost

# Set the entrypoint with the 'start' subcommand and the correct config path
ENTRYPOINT ["/usr/local/bin/preconf-boost"]
