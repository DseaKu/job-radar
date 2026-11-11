# job_radar

Give it your CV and one or more career-page URLs.
It downloads each page, asks an LLM to find the job postings, rates each job against your CV,
and writes everything to a CSV file (best match first).

## Run

```sh
# CV from the clipboard (macOS)
pbpaste | cargo run -- https://example.com/careers

# CV from a file, several URLs, custom output file
cargo run -- --cv cv.md --out jobs.csv https://a.com/jobs https://b.com/careers
```

CSV columns: `match_score, title, company, location, description, match_reason, url, source`.

## CV as PDF

Convert the PDF to text once, check the result, then use the text file:

```sh
cargo run -- convert-cv cv.pdf          # writes cv.txt
cargo run -- --cv cv.txt https://example.com/careers
```

`--cv cv.pdf` also works directly, but then the PDF is converted on every run.
Scanned PDFs (images without a text layer) cannot be converted.

## Limits

The tool does a plain HTTP GET. Pages that build their job list with JavaScript
(for example LinkedIn, Workday, some Greenhouse/Lever embeds) return no jobs.
For those sites, use the direct job-board URL (e.g. `boards.greenhouse.io/<company>`,
`jobs.lever.co/<company>`) when one exists.

## Sources

Without URLs on the command line, the tool reads every enabled entry in `sources.yaml`.
Git ignores this file, so your searches stay private. Create it from the template:

```sh
cp sources.example.yaml sources.yaml
cargo run -- --cv cv.txt
```

To use another file, set `JOB_RADAR_SOURCES=<path>`.

To add a site, do the search in your browser, copy the URL, and test it first
(no AI call, no cost):

```sh
cargo run -- page-text 'https://www.stepstone.de/jobs/it/in-witten?radius=25'
```

If the output shows the job titles, add the URL to `sources`.

## Output language

Set `output_language` in `config.yaml` (default: `English`). It controls the language of
`description` and `match_reason`. Titles, companies and locations stay as on the page.

## Providers

Select the provider with `active` in `config.yaml` (`local`, `claude`, `openai`, ...).

Supported types: `ollama` (local), `anthropic`, and `openai` (any OpenAI-compatible API).

Put API tokens in a `.env` file (Git ignores it):

```sh
ANTHROPIC_API_KEY=your-key-here
OPENAI_API_KEY=your-key-here
```

## Customize

Edit the extraction prompt and the CSV columns in `src/jobs.rs`.
