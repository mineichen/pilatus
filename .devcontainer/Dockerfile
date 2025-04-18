FROM rust:1.82-bookworm AS builder

RUN curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash && \
    cargo binstall -y just && \
    cargo binstall -y cargo-udeps && \
    cargo binstall -y cargo-outdated

RUN apt-get update && \
    apt-get install -y ca-certificates curl gnupg && \
    mkdir -m 0755 -p /etc/apt/keyrings && \
    curl -fsSL https://download.docker.com/linux/debian/gpg | gpg --dearmor -o /etc/apt/keyrings/docker.gpg && \
    echo "deb [arch="$(dpkg --print-architecture)" signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/debian \
    "$(. /etc/os-release && echo "$VERSION_CODENAME")" stable" | \
    tee /etc/apt/sources.list.d/docker.list > /dev/null && \
    apt-get update && \
    apt-get install -y docker-ce-cli && \
    rm -rf /var/lib/apt/lists/*


FROM rust:1.82-bookworm

ENV DOCKER_BUILDKIT=1

RUN rustup toolchain add nightly -c rustfmt -c clippy -c miri && \
    rustup component add rustfmt && \
    rustup component add clippy && \    
    apt-get update && apt-get install -y vim htop mold iputils-ping && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/cargo/bin/cargo-binstall /usr/local/cargo/bin/cargo-binstall
COPY --from=builder /usr/local/cargo/bin/just /usr/local/cargo/bin/just
COPY --from=builder /usr/local/cargo/bin/cargo-udeps /usr/local/cargo/bin/cargo-udeps
COPY --from=builder /usr/local/cargo/bin/cargo-outdated /usr/local/cargo/bin/cargo-outdated
COPY --from=builder /usr/bin/docker /usr/bin/docker