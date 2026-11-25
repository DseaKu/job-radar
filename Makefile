# Short commands for job_radar. Run `make` or `make help` for the list.
#
# The commands run job_radar in Docker (the newest image from ghcr.io).
# Add LOCAL=1 to use the code in this folder with cargo instead, for example:
#   make search LOCAL=1

CV    ?= cv.txt
ARGS  ?=
IMAGE ?= ghcr.io/dseaku/job-radar

# -it only in a terminal: then job_radar can ask (y/N). Without a terminal, -t fails.
TTY := $(shell [ -t 0 ] && echo -it)
# Linux: make Ollama on the host reachable, and make the new files belong to you, not to root.
ifeq ($(shell uname -s),Linux)
LINUX_OPTS := --add-host=host.docker.internal:host-gateway --user "$$(id -u):$$(id -g)"
endif

ifdef LOCAL
JOB_RADAR = cargo run --release --quiet --
else
JOB_RADAR = docker run --rm $(TTY) --pull always $(LINUX_OPTS) -v "$(CURDIR):/work" $(IMAGE)
endif

.DEFAULT_GOAL := help
.PHONY: help search init page-text application convert-cv \
        test lint fmt check demo gui gui-build docker-build

help: ## Show this list
	@echo "Usage: make <command> [CV=cv.txt] [LOCAL=1]"
	@echo
	@awk 'BEGIN {FS = ":.*## "} /^[a-z-]+:.*## / {printf "  %-13s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

# --- Use job_radar ---------------------------------------------------------------------------

search: ## Find and rate jobs. More options: ARGS="--kill-dups"
	$(JOB_RADAR) --cv $(CV) -v -p $(ARGS)

init: ## Create the missing settings files (*.yaml) from the examples
	$(JOB_RADAR) init

page-text: ## Show the page text without AI: make page-text URL=https://...
	@test -n "$(URL)" || { echo "error: give the page: make page-text URL=https://..."; exit 1; }
	$(JOB_RADAR) page-text '$(URL)'

application: ## Write a cover letter: make application JOB=3 (row of jobs.csv, or a job link)
	@test -n "$(JOB)" || { echo "error: give the job: make application JOB=3"; exit 1; }
	$(JOB_RADAR) application --cv $(CV) '$(JOB)'

convert-cv: ## Convert a PDF CV to cv.txt: make convert-cv PDF=cv.pdf
	@test -n "$(PDF)" || { echo "error: give the PDF: make convert-cv PDF=cv.pdf"; exit 1; }
	$(JOB_RADAR) convert-cv $(PDF)

# --- Development -----------------------------------------------------------------------------

test: ## Run the unit tests
	cargo test

lint: ## Check the format and run clippy, as the CI does
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings

fmt: ## Format the code
	cargo fmt

check: lint test ## All CI checks: lint and test

demo: ## End-to-end demo with fictional data (needs Ollama and qwen3:1.7b)
	demo/run.sh

gui: ## Start the desktop app in development mode
	cd gui && npm install && npm run tauri dev

gui-build: ## Build the desktop app installer
	cd gui && npm install && npm run tauri build

docker-build: ## Build the Docker image from this folder (tag: job-radar)
	docker build -t job-radar .
