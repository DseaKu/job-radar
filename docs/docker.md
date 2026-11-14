# Docker

The image contains only the program. It needs no Rust toolchain and works the same on
Linux, macOS and Windows.

Your files stay on your machine. Mount your folder to `/work`: the program reads
`config.yaml`, `sources.yaml`, `ignore.yaml`, `.env` and your CV from there, and writes
`jobs.csv` back into it. Without a `config.yaml`, it uses Ollama with `qwen3:1.7b`.

## Run

macOS and Windows (Docker Desktop):

```sh
docker run --rm -v "$PWD:/work" ghcr.io/dseaku/job-radar --cv cv.txt
```

Linux: add two options. The first one makes Ollama on the host reachable. The second one
makes `jobs.csv` belong to you, not to root:

```sh
docker run --rm -v "$PWD:/work" \
  --add-host=host.docker.internal:host-gateway \
  --user "$(id -u):$(id -g)" \
  ghcr.io/dseaku/job-radar --cv cv.txt
```

All [options](../README.md#options) work the same. Put them after the image name:

```sh
docker run --rm -v "$PWD:/work" ghcr.io/dseaku/job-radar page-text https://example.com/careers
pbpaste | docker run --rm -i -v "$PWD:/work" ghcr.io/dseaku/job-radar   # CV from stdin: add -i
```

Tip: make an alias, then use `job_radar` as if it was installed:

```sh
alias job_radar='docker run --rm -i -v "$PWD:/work" ghcr.io/dseaku/job-radar'
```

## Providers in the container

- **Ollama on your machine**: works without changes. Inside a container, `localhost` is the
  container itself, so the image sets `JOB_RADAR_OLLAMA_URL=http://host.docker.internal:11434`.
  A `base_url` in `config.yaml` has priority over this variable.
- **Anthropic, OpenAI**: put the key into `.env` in the mounted folder, or pass it with
  `-e ANTHROPIC_API_KEY`.

## Build the image yourself

```sh
docker build -t job-radar .
docker run --rm -v "$PWD:/work" job-radar --cv cv.txt
```

`.dockerignore` lets only `Cargo.toml`, `Cargo.lock` and `src/` into the build, so your CV,
`.env` and private config files never go into an image.

The [Docker workflow](../.github/workflows/docker.yml) builds the image for `amd64` and `arm64`
and pushes it to `ghcr.io` on each change of the code on `main` (tag `latest`) and for each
version tag `v*` (for example `1.2.0`).
