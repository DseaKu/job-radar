# Testing

- **[CI](../.github/workflows/ci.yml)** (each push): `cargo fmt --check`, `cargo clippy -D warnings`
  and the unit tests. The tests need no network and no model: HTML to text, slicing, JSON
  repair, recovery of cut-off replies, the ignore list and the variables in `sources.yaml`.
- **[Demo](../.github/workflows/demo.yml)** (each change of the code or the demo): an end-to-end
  run with a real model. Ollama runs as a service container on the free GitHub runner, so it
  needs no API key and costs nothing. `temperature: 0` makes the result stable. The workflow
  checks only what does not depend on the model's wording (number of jobs, ignore list,
  scores present) and writes the result table to the run summary.

Open the latest [Demo run](https://github.com/DseaKu/job-radar/actions/workflows/demo.yml)
and look at its summary for the result table with the reason for each score.

## Local demo

The demo uses two small fictional job pages (`demo/site/`), a fictional CV (`demo/demo_cv.txt`)
and a small local model (`qwen3:1.7b`, about 1.4 GB).

```sh
ollama pull qwen3:1.7b
demo/run.sh
```

The demo does not use `--kill-dups`: a model this small does not merge duplicates reliably.

To record the GIF again (needs `vhs` from Homebrew): `vhs demo/demo.tape`.
