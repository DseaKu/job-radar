# Sources

Without URLs on the command line, the tool reads every enabled entry in `sources.yaml`.
Git ignores this file, so your searches stay private. Create it from the template:

```sh
cp sources.example.yaml sources.yaml
cargo run -- --cv cv.txt
```

To use another file, set `JOB_RADAR_SOURCES=<path>`.

## Add a site

Do the search in your browser, copy the URL, and test it first (no AI call, no cost):

```sh
cargo run -- page-text 'https://www.stepstone.de/jobs/it/in-witten?radius=25'
```

If the output shows the job titles, add the URL to `sources`.

## Pages that do not work

The tool does a plain HTTP GET. Pages that build their job list with JavaScript
(for example LinkedIn, Workday, some Greenhouse/Lever embeds) return no jobs.
For those sites, use the direct job-board URL (e.g. `boards.greenhouse.io/<company>`,
`jobs.lever.co/<company>`) when one exists.
