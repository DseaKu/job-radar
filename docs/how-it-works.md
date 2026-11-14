# How it works

- **One interface, three APIs.** Ollama (native chat API), any OpenAI-compatible API and the
  Anthropic Messages API sit behind one `Backend`. Switch with one line in `config.yaml`.
- **Pages larger than the context window.** The page text is cut into slices that fit next to
  the CV, the prompt and `max_tokens` (set `context_length` in `config.yaml`). The slices
  overlap by 10%, so a job at a boundary is complete in one of them, and the copies from the
  overlap are removed. Each slice is one AI call that sends the CV again. One page uses at most 20 slices.
- **Replies that are cut off.** If a reply stops at `max_tokens`, the complete jobs before the
  cut are kept, and the slice is split in half and read again (up to 3 levels).
- **Imperfect model output.** The parser finds the JSON array inside prose or code fences,
  repairs invalid escapes that models copy from pages (`Entwickler\*in`), and moves links that
  small models put into the title to the `url` column.
- **Cost control.** Each call logs its token usage, and a run ends with the total.
  `page-text` tests a new source without an AI call.

## Duplicates

The same job can appear on several platforms. `--kill-dups` makes one more AI call that groups
the rows that are the same job. For each group, the row with the highest `match_score` stays.
Its `source` column lists all sources, separated by ` | `.
If this step fails, the tool writes the CSV with the duplicates and shows a warning.

## CV as PDF

`convert-cv cv.pdf` writes `cv.txt` once, so you can check the result.
`--cv cv.pdf` also works directly, but then the PDF is converted on every run.
Scanned PDFs (images without a text layer) cannot be converted.
