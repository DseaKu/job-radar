# job_radar as a container: no Rust toolchain needed, same on Linux, macOS and Windows.
#
# The image contains only the program. Your files stay on your machine: mount your folder
# to /work, and the program reads config.yaml, sources.yaml, ignore.yaml, .env and your CV
# from there and writes jobs.csv back into it.
#
#   docker run --rm -v "$PWD:/work" ghcr.io/dseaku/job-radar --cv cv.txt

# --- Build stage -----------------------------------------------------------------------------
FROM rust:1-slim-trixie AS build
RUN apt-get update \
 && apt-get install -y --no-install-recommends pkg-config libssl-dev \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /src

# Build the dependencies first, so that a change in src/ does not build them again.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs \
 && cargo build --release --locked \
 && rm -rf src

COPY src ./src
RUN touch src/main.rs && cargo build --release --locked

# --- Runtime stage ---------------------------------------------------------------------------
FROM debian:trixie-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates libssl3t64 \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/job_radar /usr/local/bin/job_radar

# Ollama on the host machine. On Linux, add: --add-host=host.docker.internal:host-gateway
ENV JOB_RADAR_OLLAMA_URL=http://host.docker.internal:11434

WORKDIR /work
ENTRYPOINT ["job_radar"]
CMD ["--help"]
