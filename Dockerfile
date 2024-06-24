FROM node:18

RUN apt-get update && \
    apt-get install -y curl protobuf-compiler libprotobuf-dev && \
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y && \
    /root/.cargo/bin/rustup default stable

ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /usr/src/app

RUN /root/.cargo/bin/rustup --version && /root/.cargo/bin/cargo --version && /root/.cargo/bin/rustc --version

# Default command
CMD ["tail", "-f", "/dev/null"]
