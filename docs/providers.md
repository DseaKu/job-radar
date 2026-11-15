# Providers

All settings are in `config.yaml`. Git ignores this file. Create it from the template:

```sh
cp config.example.yaml config.yaml
```

Without `config.yaml`, job_radar uses Ollama with `qwen3:1.7b` and English output.
To use another file, set `AGENTS_CONFIG=<path>`.

Select the provider with `active` in `config.yaml` (`local`, `claude`, `openai`, ...).

Supported types: `ollama` (local), `anthropic`, and `openai` (any OpenAI-compatible API).

Put API tokens in a `.env` file (Git ignores it):

```sh
ANTHROPIC_API_KEY=your-key-here
OPENAI_API_KEY=your-key-here
```

## Context length

Set the model's context window with `context_length` in `config.yaml`.
Long pages are read in parts that fit into it. See [How it works](how-it-works.md).

## Output language

Set `output_language` in `config.yaml` (default: `English`). It controls the language of
`description` and `match_reason`. Titles, companies and locations stay as on the page.

## Extra instructions

Tell the AI what matters to you with `extra_instructions` in `config.yaml`:

```yaml
extra_instructions: |
  I look for a working-student job, 20 hours a week. Rate full-time jobs lower.
  Rate jobs with Rust or embedded systems higher.
  Ignore jobs in sales, marketing and management.
```

The text goes at the end of the prompt for finding and rating the jobs. The reply format stays
the same. How well the AI follows the instructions depends on the model: small local models
(for example `qwen3:1.7b`) follow them only partly. To remove companies or jobs reliably, use
the [ignore list](ignore-list.md).

## Customize

Edit the extraction prompt and the CSV columns in `src/jobs.rs`.
