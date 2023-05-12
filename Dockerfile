FROM rust:1.69

# Install mold linker
RUN apt-get update && \
    apt-get install -y cmake clang && \
    rm -rf /var/lib/apt/lists/* && \
    git clone https://github.com/rui314/mold.git && \
    mkdir mold/build && \
    cd mold/build && \
    git checkout v1.7.1 && \
    ../install-build-deps.sh && \
    cmake -DCMAKE_BUILD_TYPE=Release -DCMAKE_CXX_COMPILER=c++ .. && \
    cmake --build . -j $(nproc) && \
    cmake --install .

# Install "just" command
RUN git config --global pull.rebase true && \
    mkdir -p /usr/temp/src/pilatus && \
    cargo install --locked just && \
    rm -rf /usr/local/cargo/registry

WORKDIR /usr/temp/src/pilatus

ENV NIGHTLY_VERSION "2023-04-29"
ENV DOCKER_BUILDKIT 1

RUN rustup toolchain add nightly-${NIGHTLY_VERSION} && \
    rustup default nightly-${NIGHTLY_VERSION} && \
    rustup component add rustfmt && \
    rustup component add clippy && \
    rustup component add miri && \
    rustup default $RUST_VERSION && \
    rustup component add rustfmt && \
    rustup component add clippy && \
    cargo install tokio-console --locked && \
    cargo install cargo-udeps --locked && \
    cargo install cargo-outdated --locked && \
    apt update && apt install -y vim jq htop && \
    rm -rf /usr/local/cargo/registry && \
    rm -rf /var/lib/apt/lists/*

# Install Docker-CLI
RUN apt update && \
    apt-get install -y ca-certificates curl gnupg && \
    mkdir -m 0755 -p /etc/apt/keyrings && \
    curl -fsSL https://download.docker.com/linux/debian/gpg | gpg --dearmor -o /etc/apt/keyrings/docker.gpg && \
    echo "deb [arch="$(dpkg --print-architecture)" signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/debian \
        "$(. /etc/os-release && echo "$VERSION_CODENAME")" stable" | \
        tee /etc/apt/sources.list.d/docker.list > /dev/null && \
    apt-get update && \
    apt-get install -y docker-ce-cli && \
    rm -rf /var/lib/apt/lists/*
