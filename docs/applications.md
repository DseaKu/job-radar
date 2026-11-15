# Applications

job_radar can write a cover letter for a job that it found. The letter is a LaTeX file, so
you can change the text and the layout easily.

## Write a letter

- **App:** select a job, then click _Write application_. _Show in folder_ opens the folder.
- **Command line:**

  ```sh
  job_radar application --cv cv.txt 1                              # job 1 of jobs.csv (best match)
  job_radar application --cv cv.txt https://example.com/jobs/42   # a job link
  ```

  `--jobs <file>` reads another CSV file. A link that is not in the CSV also works: then the
  AI gets only the job page.

## Folder

Each letter gets its own folder in `applications_dir` (default `applications`; Git ignores it):

```
applications/
  nordlicht-robotics-rust-developer-m-w-d/
    cover_letter.tex   the letter
    cover_letter.pdf   the PDF of the letter (if LaTeX is installed)
    job.txt            the job data and the text of the job page
```

If the folder exists, the new folder gets `-2`, `-3` and so on. An existing letter is never
overwritten. Set the folder in `config.yaml` or in the app (_Settings → Applications_).
`~` is your home folder.

## How it works

1. job_radar reads the job page again for the full posting. If that fails (for example, the
   page needs JavaScript), the AI gets only the short description from the search.
2. One AI call writes the text from your CV and the posting: sender, recipient, subject,
   salutation, 3 to 5 paragraphs and closing. The AI must use only facts from your CV.
3. job_radar puts the text into a LaTeX template (KOMA-Script `scrlttr2`, a DIN letter) and
   escapes the special characters of LaTeX.

The letter is in the language of the job posting. To change this, or to add facts (for example
your start date), use `application_instructions` in `config.yaml` (app: _Settings →
Applications_).

## Your template: letter.yaml

Without `letter.yaml`, the AI takes your data from the CV and the built-in layout applies.
To set your data and your own layout, create the file from the template:

```sh
cp letter.example.yaml letter.yaml
```

Git ignores `letter.yaml`. To use another file, set `JOB_RADAR_LETTER=<path>`. The desktop
app reads the file from its data folder. All fields are optional:

- `sender`: your name, address (a list of lines), phone and email. They replace the values
  that the AI takes from the CV.
- `enclosures`: the enclosures in each letter, for example `Lebenslauf, Zeugnisse`.
- `template`: your own LaTeX document. job_radar replaces placeholders such as `{{ body }}`,
  `{{ recipient }}` and `{{ subject }}` with the escaped text. `{{ body }}` is required.
  [`letter.example.yaml`](../letter.example.yaml) lists all placeholders, and its template
  is the built-in layout.

job_radar reads the file before the AI call, so an error in it costs no tokens.

## The PDF

If a LaTeX distribution is installed (TeX Live, MacTeX or MiKTeX), job_radar builds
`cover_letter.pdf` automatically with `latexmk` (or `pdflatex`) and removes the helper files.
Without LaTeX, or if LaTeX fails, you still get the `.tex` file and a warning. After a failure,
`cover_letter.log` shows the details.

After you change the letter, build the PDF again:

```sh
cd applications/nordlicht-robotics-rust-developer-m-w-d
latexmk -pdf cover_letter.tex     # or: pdflatex cover_letter.tex
```

Always read the letter before you send it.
