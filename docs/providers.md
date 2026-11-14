# Providers

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

## Customize

Edit the extraction prompt and the CSV columns in `src/jobs.rs`.
