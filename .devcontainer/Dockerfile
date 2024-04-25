FROM ubuntu:22.04 as rbase
RUN apt-get update && \
    apt-get install -y \
    git \
    curl \
    build-essential

# Install latest version of Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"
RUN cargo install cargo-dist
