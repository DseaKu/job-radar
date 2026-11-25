# Changelog

All notable changes to job_radar are in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the
version numbers follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- `Makefile` with short commands: `make search`, `make init`, `make page-text URL=...`,
  `make application JOB=3` and more. They run Docker by default, or cargo with `LOCAL=1`. On
  Linux, they add the Docker options for Ollama and file owners. `make` shows the list. ([#5])
- `job_radar init` creates the missing settings files (`config.yaml`, `sources.yaml`,
  `ignore.yaml`, `letter.yaml`) from the examples. The examples are built into the program, so
  this also works in Docker. Files that exist are not changed. ([#4])
- When settings files are missing, a search (or `application`) asks (y/N) to create them from
  the examples. Without a terminal (for example Docker without `-it`), it shows a hint instead.
  ([#4])

### Changed

- README: new section "Make commands" with all commands of the Makefile. The quick start, the
  desktop build and the demo use the short `make` commands. ([#6])
- The Docker commands in the README and in `docs/docker.md` use `-it`, so job_radar can ask
  (y/N) to create missing settings files. Without `-it`, the hint in Docker tells you to add it. ([#5])
- The search stops at the first page when the AI provider rejects the API key (HTTP 401 or
  403). It shows one short message with the name of the key variable and what to do.
  Before, the same long error came for each page. ([#2])
- The search also stops at the first page when there is no connection to the AI provider,
  for example when Ollama does not run. In the Docker image, the message tells you that
  Ollama must run on your computer, and it gives the Linux option. ([#3])
- Errors are shown without quotation marks: `Error: all URLs failed`. ([#2])
- README: the Docker command uses `--pull always`, so it always gets the newest image. ([#1])

## [0.2.0] - 2026-11-16

### Added

- Cover letter drafts: `job_radar application <n>` (or _Write application_ in the desktop
  app) writes a LaTeX letter, and a PDF if LaTeX is installed, for one job. Your layout and
  your fixed data go into `letter.yaml`. Settings: `applications_dir` and
  `application_instructions`.
- `extra_body` in `config.yaml` adds extra fields to each API request.
- `stream: true` reads the reply of an OpenAI-compatible API as a stream.
- `timeout` in `config.yaml` sets the upper limit for one request.
- README: model recommendation (Claude Haiku 5.5).

## [0.1.0] - 2026-11-15

### Added

- First release: give a CV (text, PDF or stdin) and career pages (URLs or `sources.yaml`).
  An LLM finds the jobs, rates each job against the CV (0 to 100, with a reason) and writes
  `jobs.csv`, best match first.
- Providers: Ollama, Anthropic and OpenAI-compatible APIs.
- Big pages are split into slices.
- Ignore list (`ignore.yaml`).
- Options: `--kill-dups`, `-v` / `--verbose`, `-p` / `--print-results`, `--dump`.
- Commands: `page-text` and `convert-cv`.
- Docker image on `ghcr.io` and desktop app installers for macOS, Windows and Linux.

[Unreleased]: https://github.com/DseaKu/job-radar/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/DseaKu/job-radar/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/DseaKu/job-radar/releases/tag/v0.1.0
[#1]: https://github.com/DseaKu/job-radar/pull/1
[#2]: https://github.com/DseaKu/job-radar/pull/2
[#3]: https://github.com/DseaKu/job-radar/pull/3
[#4]: https://github.com/DseaKu/job-radar/pull/4
[#5]: https://github.com/DseaKu/job-radar/pull/5
[#6]: https://github.com/DseaKu/job-radar/pull/6
