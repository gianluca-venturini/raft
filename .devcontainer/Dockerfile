# Use Debian as the base image
FROM debian:bookworm-slim

# Install required dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl \
    build-essential \
    protobuf-compiler \
    nodejs=18.19.0+dfsg-6~deb12u2 \
    npm=9.2.0~ds1-1 && \
    apt-get clean && \
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y && \
    npm install -g yarn@1.22.17

# Set the working directory
WORKDIR /usr/src/app

# Copy the current directory contents into the container at /usr/src/app
COPY . .

# Expose the port the app runs on (optional)
# EXPOSE 8080
