# Use Debian as the base image
FROM debian:bookworm-slim

# Install required dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl \
    ca-certificates \
    build-essential \
    protobuf-compiler && \
    apt-get clean && \
    curl --proto '=https' --tlsv1.2 -sSf https://deb.nodesource.com/setup_22.x | bash && \
    apt-get install -y nodejs=22.13.0-1nodesource1 && \
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y && \
    npm install -g yarn@1.22.17

# Set the working directory
WORKDIR /usr/src/app

# Copy the current directory contents into the container at /usr/src/app
COPY . .

# Expose the port the app runs on (optional)
# EXPOSE 8080
