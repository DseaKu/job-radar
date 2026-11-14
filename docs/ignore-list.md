# Ignore list

To skip jobs that you already applied to, put them in `ignore.yaml`.
Git ignores this file. Create it from the template:

```sh
cp ignore.example.yaml ignore.yaml
```

To use another file, set `JOB_RADAR_IGNORE=<path>`.

## Matching

Each entry has one or more of `url`, `company` and `title`. A job is ignored if all given
fields match.

- `url` must be the same link.
- `company` and `title` match if the text is part of the value at the start of a word
  (case does not count): `polizei` matches `Polizeipräsidium`, but `its` does not match `Schwarz Digits`.
- The optional `note` field is for you.

The same job has a different link on each platform, so use `company` + `title` to ignore it everywhere.

Simple format: write only company names, one on each line, without the `ignore:` key.

## Cost

The ignored jobs are removed after the AI read the pages, so they still cost tokens.
