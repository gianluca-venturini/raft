FROM node:18

RUN apt-get update && \
    apt-get install -y curl protobuf-compiler libprotobuf-dev && \
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /usr/src/app