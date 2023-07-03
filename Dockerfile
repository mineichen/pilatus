FROM rust:1.70-bookworm as builder

RUN cargo install tokio-console --locked && \
    cargo install just --locked && \
    cargo install cargo-udeps --locked && \
    cargo install cargo-outdated --locked 

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


FROM rust:1.70-bookworm

ENV DOCKER_BUILDKIT 1

RUN rustup toolchain add nightly -c rustfmt -c clippy -c miri && \
    rustup component add rustfmt && \
    rustup component add clippy && \    
    apt update && apt install -y vim htop mold iputils-ping && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/cargo/bin/tokio-console /usr/local/cargo/bin/tokio-console
COPY --from=builder /usr/local/cargo/bin/just /usr/local/cargo/bin/just
COPY --from=builder /usr/local/cargo/bin/cargo-udeps /usr/local/cargo/bin/cargo-udeps
COPY --from=builder /usr/local/cargo/bin/cargo-outdated /usr/local/cargo/bin/cargo-outdated
COPY --from=builder /usr/bin/docker /usr/bin/docker