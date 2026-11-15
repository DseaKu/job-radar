//! Writes a cover letter for one job. One AI call writes the text, a LaTeX template gives the
//! layout. Each letter gets its own folder in `applications_dir`, so that it can be edited later.
//!
//! The folder holds two files:
//! - `cover_letter.tex`: the letter (KOMA-Script `scrlttr2`, build it with pdflatex)
//! - `job.txt`: the job data and the posting text, for reference when the job page is gone
//! - `cover_letter.pdf`: built with latexmk or pdflatex, if a LaTeX distribution is installed

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::Res;
use crate::backend::Backend;
use crate::config::Config;
use crate::jobs::{JobRow, fix_escapes, floor_boundary};
use crate::{dump, fetch};

/// The letter file in each application folder.
pub const LETTER_FILE: &str = "cover_letter.tex";
/// The job data file in each application folder.
pub const JOB_FILE: &str = "job.txt";

/// Upper limit for the posting text in the prompt. A job posting is rarely longer, and more
/// text (menus, footers) costs tokens without a better letter.
const MAX_POSTING_CHARS: usize = 20_000;

const WRITER: &str = r#"You write the cover letter for a job application. You get the CV of the candidate and the job posting.

Rules:
- Write in the first person, as the candidate. Use only facts from the CV. Do not invent experience, skills, degrees or dates.
- Connect the most relevant experience in the CV to the requirements of the job. Be specific, not generic.
- Write 3 to 5 short paragraphs, together at most 350 words, so that the letter fits on one page.
- Write the letter in the language of the job posting, unless the instructions of the candidate say otherwise.
- Use the usual salutation and closing for that language. If the posting names a contact person, address that person.

Reply with only one JSON object. Do not write prose or code fences. The keys are:
- "language": the language of the letter, as an English word (for example "German")
- "sender_name": the full name of the candidate, from the CV
- "sender_address": the postal address from the CV, lines separated by "\n", or ""
- "sender_phone": the phone number from the CV, or ""
- "sender_email": the email address from the CV, or ""
- "recipient": the address of the company, lines separated by "\n": company name, contact person, street and city, as far as the posting gives them
- "subject": the subject line, with the job title
- "opening": the salutation, for example "Dear Ms Smith,"
- "paragraphs": an array of strings, one string for each paragraph, as plain text
- "closing": the closing phrase without the name, for example "Kind regards,"
- "enclosures": the enclosures, for example "CV, references", or """#;

/// The text of a cover letter. The AI writes it, [`latex`] gives it the layout.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Letter {
    pub language: String,
    pub sender_name: String,
    pub sender_address: Vec<String>,
    pub sender_phone: String,
    pub sender_email: String,
    pub recipient: Vec<String>,
    pub subject: String,
    pub opening: String,
    pub paragraphs: Vec<String>,
    pub closing: String,
    pub enclosures: String,
}

/// What [`write`] made.
#[derive(Debug)]
pub struct Written {
    /// The new folder of this application.
    pub dir: PathBuf,
    /// The LaTeX file in it.
    pub letter: PathBuf,
    /// The PDF file, if LaTeX built it.
    pub pdf: Option<PathBuf>,
}

/// Writes a cover letter for `job` into a new folder in `config.applications_dir`.
///
/// The job page is read again for the full posting text. If that fails, the AI gets only the
/// short description from the search.
pub async fn write(
    backend: &Backend,
    http: &reqwest::Client,
    config: &Config,
    cv: &str,
    job: &JobRow,
) -> Res<Written> {
    // Read the template first: an error in it must not cost an AI call.
    let template = LetterTemplate::load()?;
    info!("Writing the application: {} ...", label(job));
    let posting = posting_text(http, job).await;

    let system = system_prompt(&config.application_instructions);
    let head = job_head(cv, job);
    let budget = backend
        .page_char_budget(system.len() + head.len())
        .min(MAX_POSTING_CHARS);
    let posting = cut(&posting, budget);
    let reply = backend
        .chat(&system, &format!("{head}{}", posting_block(posting)))
        .await?;

    let mut letter = parse_letter(&reply)?;
    if letter.recipient.is_empty() {
        letter.recipient = [&job.company, &job.location]
            .into_iter()
            .filter(|s| !s.is_empty())
            .cloned()
            .collect();
    }
    if letter.subject.is_empty() {
        letter.subject = job.title.clone();
    }
    template.apply(&mut letter);
    let document = template.render(&letter, job)?;

    let dir = new_folder(
        &applications_dir(&config.applications_dir),
        &folder_name(job),
    )?;
    let letter_path = dir.join(LETTER_FILE);
    write_file(&letter_path, &document)?;
    write_file(&dir.join(JOB_FILE), &job_text(job, posting))?;
    info!("  written to {}", dir.display());
    let pdf = match build_pdf(&dir).await {
        Ok(pdf) => Some(pdf),
        Err(e) => {
            warn!("  warning: no PDF: {e}");
            None
        }
    };
    Ok(Written {
        dir,
        letter: letter_path,
        pdf,
    })
}

const DEFAULT_TEMPLATE_PATH: &str = "letter.yaml";

/// The letter template, loaded from `letter.yaml` (or the file named by `JOB_RADAR_LETTER`):
/// the fixed sender data, the enclosures and an own LaTeX layout. All fields are optional.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LetterTemplate {
    /// Replaces the sender data that the AI takes from the CV.
    pub sender: Sender,
    /// The enclosures in each letter. Empty: the AI writes them.
    pub enclosures: String,
    /// An own LaTeX document with `{{ name }}` placeholders. Empty: the built-in layout.
    pub template: String,
}

/// The sender data in `letter.yaml`. An empty field keeps the value from the CV.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sender {
    pub name: String,
    pub address: Vec<String>,
    pub phone: String,
    pub email: String,
}

impl LetterTemplate {
    /// Loads the template file. A missing default file gives the built-in template.
    pub fn load() -> Res<Self> {
        let (path, explicit) = match std::env::var("JOB_RADAR_LETTER") {
            Ok(p) if !p.trim().is_empty() => (p, true),
            _ => (DEFAULT_TEMPLATE_PATH.to_string(), false),
        };
        if !explicit && !Path::new(&path).exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(&path).map_err(|e| format!("cannot read {path}: {e}"))?;
        Self::parse(&text).map_err(|e| format!("invalid {path}: {e}").into())
    }

    fn parse(text: &str) -> Res<Self> {
        if text.trim().is_empty() {
            return Ok(Self::default());
        }
        let template: Self = serde_yaml::from_str(text)?;
        if !template.template.trim().is_empty()
            && !template.template.contains("{{ body }}")
            && !template.template.contains("{{body}}")
        {
            return Err("the template needs the placeholder {{ body }} for the letter text".into());
        }
        Ok(template)
    }

    /// Puts the fixed values of the template into the letter.
    pub fn apply(&self, letter: &mut Letter) {
        let set = |field: &mut String, value: &str| {
            if !value.trim().is_empty() {
                *field = one_line(value);
            }
        };
        set(&mut letter.sender_name, &self.sender.name);
        set(&mut letter.sender_phone, &self.sender.phone);
        set(&mut letter.sender_email, &self.sender.email);
        set(&mut letter.enclosures, &self.enclosures);
        let address: Vec<String> = self
            .sender
            .address
            .iter()
            .map(|l| one_line(l))
            .filter(|l| !l.is_empty())
            .collect();
        if !address.is_empty() {
            letter.sender_address = address;
        }
    }

    /// The LaTeX document: the own template with its placeholders filled, or the built-in one.
    pub fn render(&self, letter: &Letter, job: &JobRow) -> Res<String> {
        if self.template.trim().is_empty() {
            return Ok(latex(letter, job));
        }
        let recipient = if letter.recipient.is_empty() {
            "\\mbox{}".to_string()
        } else {
            tex_lines(&letter.recipient, "\\\\\n")
        };
        let body = letter
            .paragraphs
            .iter()
            .map(|p| tex(p))
            .collect::<Vec<_>>()
            .join("\n\n");
        let values = [
            ("sender_name", tex(&letter.sender_name)),
            ("sender_address", tex_lines(&letter.sender_address, "\\\\")),
            ("sender_phone", tex(&letter.sender_phone)),
            ("sender_email", tex(&letter.sender_email)),
            ("recipient", recipient),
            ("subject", tex(&letter.subject)),
            ("opening", tex(&letter.opening)),
            ("body", body),
            ("closing", tex(&letter.closing)),
            ("enclosures", tex(&letter.enclosures)),
            (
                "language",
                babel_language(&letter.language)
                    .unwrap_or("english")
                    .to_string(),
            ),
            ("job_title", tex(&job.title)),
            ("company", tex(&job.company)),
        ];
        let mut out = self.template.clone();
        for (name, value) in &values {
            out = out
                .replace(&format!("{{{{ {name} }}}}"), value)
                .replace(&format!("{{{{{name}}}}}"), value);
        }
        Ok(out)
    }
}

/// Upper limit for one LaTeX run. A one-page letter needs a few seconds.
const LATEX_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Folders where LaTeX is often installed. A desktop app started from the Finder or a
/// start menu does not get the PATH of the shell, so these are searched too.
const TEX_DIRS: &[&str] = &[
    "/Library/TeX/texbin",
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "/usr/bin",
];

/// The full path of a LaTeX program: from PATH, else from [`TEX_DIRS`].
fn find_program(name: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .chain(TEX_DIRS.iter().map(PathBuf::from))
        .map(|dir| dir.join(&exe))
        .find(|p| p.is_file())
}

/// Builds `cover_letter.pdf` in `dir` with latexmk (or pdflatex, if latexmk is missing) and
/// removes the helper files. On an error, the `.log` file stays for the details.
pub async fn build_pdf(dir: &Path) -> Res<PathBuf> {
    let stem = LETTER_FILE.trim_end_matches(".tex");
    let flags = ["-interaction=nonstopmode", "-halt-on-error"];
    let runs: Vec<(PathBuf, Vec<&str>)> = if let Some(latexmk) = find_program("latexmk") {
        let mut args = vec!["-pdf"];
        args.extend(flags);
        args.push(LETTER_FILE);
        vec![(latexmk, args)]
    } else if let Some(pdflatex) = find_program("pdflatex") {
        let mut args = flags.to_vec();
        args.push(LETTER_FILE);
        // The second run puts the page references of scrlttr2 in place.
        vec![(pdflatex.clone(), args.clone()), (pdflatex, args)]
    } else {
        return Err("LaTeX is not installed (latexmk and pdflatex not found). \
                    Install TeX Live, MacTeX or MiKTeX, or build the PDF yourself."
            .into());
    };

    info!("  building the PDF ...");
    for (program, args) in &runs {
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args)
            .current_dir(dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        // LaTeX finds its own tools (for example pdflatex for latexmk) next to it, and
        // latexmk needs perl from the system folders.
        if let Some(bin) = program.parent() {
            let path = std::env::var_os("PATH").unwrap_or_default();
            let system = ["/usr/bin", "/bin"].map(PathBuf::from);
            let paths = std::iter::once(bin.to_path_buf())
                .chain(std::env::split_paths(&path))
                .chain(system);
            if let Ok(joined) = std::env::join_paths(paths) {
                cmd.env("PATH", joined);
            }
        }
        let status = tokio::time::timeout(LATEX_TIMEOUT, cmd.status())
            .await
            .map_err(|_| format!("{} did not finish in time", program.display()))?
            .map_err(|e| format!("cannot start {}: {e}", program.display()))?;
        if !status.success() {
            return Err(format!(
                "{} failed ({status}); see {}",
                program.display(),
                dir.join(format!("{stem}.log")).display()
            )
            .into());
        }
    }
    for ext in ["aux", "log", "fls", "fdb_latexmk", "out"] {
        let _ = fs::remove_file(dir.join(format!("{stem}.{ext}")));
    }
    let pdf = dir.join(format!("{stem}.pdf"));
    if !pdf.is_file() {
        return Err(format!("LaTeX did not write {}", pdf.display()).into());
    }
    info!("  PDF: {}", pdf.display());
    Ok(pdf)
}

/// "Rust Developer at Nordlicht Robotics", for messages.
fn label(job: &JobRow) -> String {
    match (job.title.is_empty(), job.company.is_empty()) {
        (false, false) => format!("{} at {}", job.title, job.company),
        (false, true) => job.title.clone(),
        (true, false) => job.company.clone(),
        (true, true) => job.url.clone(),
    }
}

/// The text of the job's own page, or "" if the job has no own link or the page cannot be read.
async fn posting_text(http: &reqwest::Client, job: &JobRow) -> String {
    // Without an own link, the URL is the list page of the source: it shows many jobs.
    if job.url.is_empty() || job.url == job.source {
        return String::new();
    }
    info!("  reading the job page {} ...", job.url);
    match fetch::page_text(http, &job.url).await {
        Ok(text) => text,
        Err(e) => {
            warn!(
                "  warning: the job page cannot be read, only the short description is used: {e}"
            );
            String::new()
        }
    }
}

fn system_prompt(instructions: &str) -> String {
    let extra = instructions.trim();
    if extra.is_empty() {
        WRITER.to_string()
    } else {
        format!(
            "{WRITER}\n\nInstructions of the candidate. Follow them, but always keep the JSON format above:\n{extra}"
        )
    }
}

/// The prompt up to the posting text: the CV and what the search found about the job.
fn job_head(cv: &str, job: &JobRow) -> String {
    let mut out = format!("CV:\n{cv}\n\nJob:\n");
    for (name, value) in [
        ("Title", &job.title),
        ("Company", &job.company),
        ("Location", &job.location),
        ("Link", &job.url),
        ("Short description", &job.description),
    ] {
        if !value.is_empty() {
            let _ = writeln!(out, "{name}: {value}");
        }
    }
    out
}

fn posting_block(posting: &str) -> String {
    if posting.trim().is_empty() {
        "\nThe full job posting is not available. Use the short description.\n".to_string()
    } else {
        format!("\nJob posting (text of the job page):\n{posting}\n")
    }
}

/// The first `max` bytes of `text`, cut at a character boundary.
fn cut(text: &str, max: usize) -> &str {
    if text.len() <= max {
        text
    } else {
        &text[..floor_boundary(text, max)]
    }
}

/// Reads the letter from the reply, also if the model wrapped it in prose or code fences.
pub fn parse_letter(reply: &str) -> Res<Letter> {
    let (Some(start), Some(end)) = (reply.find('{'), reply.rfind('}')) else {
        return Err(format!("the model did not return a JSON object: {}", preview(reply)).into());
    };
    let json = &reply[start..=end];
    let v: Value = serde_json::from_str(json)
        .or_else(|e| serde_json::from_str(&fix_escapes(json)).map_err(|_| e))
        .map_err(|e| format!("invalid JSON from the model ({e}): {}", preview(reply)))?;

    // Some models write the paragraphs as one text with empty lines between them.
    let paragraphs: Vec<String> = match &v["paragraphs"] {
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .map(one_line)
            .collect(),
        Value::String(s) => s.split("\n\n").map(one_line).collect(),
        _ => Vec::new(),
    };
    let paragraphs: Vec<String> = paragraphs.into_iter().filter(|p| !p.is_empty()).collect();
    if paragraphs.is_empty() {
        return Err(format!(
            "the reply of the model has no letter text: {}",
            preview(reply)
        )
        .into());
    }
    Ok(Letter {
        language: text(&v, "language"),
        sender_name: text(&v, "sender_name"),
        sender_address: lines(&v, "sender_address"),
        sender_phone: text(&v, "sender_phone"),
        sender_email: text(&v, "sender_email"),
        recipient: lines(&v, "recipient"),
        subject: text(&v, "subject"),
        opening: text(&v, "opening"),
        paragraphs,
        closing: text(&v, "closing"),
        enclosures: text(&v, "enclosures"),
    })
}

fn text(v: &Value, key: &str) -> String {
    v[key].as_str().map(one_line).unwrap_or_default()
}

/// A field with several lines: a text with line breaks, or an array of texts.
fn lines(v: &Value, key: &str) -> Vec<String> {
    let all: Vec<&str> = match &v[key] {
        Value::String(s) => s.lines().collect(),
        Value::Array(items) => items.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    all.into_iter()
        .map(one_line)
        .filter(|l| !l.is_empty())
        .collect()
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn preview(s: &str) -> String {
    s.chars().take(300).collect()
}

/// The `babel` option for a language name, or `None` for a language that is not in the list.
fn babel_language(language: &str) -> Option<&'static str> {
    let first = language
        .split(|c: char| !c.is_alphabetic())
        .next()
        .unwrap_or("")
        .to_lowercase();
    Some(match first.as_str() {
        "english" => "english",
        "german" | "deutsch" => "ngerman",
        "french" => "french",
        "spanish" => "spanish",
        "italian" => "italian",
        "dutch" => "dutch",
        "polish" => "polish",
        "portuguese" => "portuguese",
        "swedish" => "swedish",
        "danish" => "danish",
        "norwegian" => "norsk",
        "finnish" => "finnish",
        "czech" => "czech",
        _ => return None,
    })
}

/// The letter as a LaTeX document: KOMA-Script `scrlttr2` for pdflatex.
pub fn latex(letter: &Letter, job: &JobRow) -> String {
    let mut options = vec![
        "paper=a4",
        "fontsize=11pt",
        "parskip=half",
        "fromalign=right",
    ];
    if !letter.sender_phone.is_empty() {
        options.push("fromphone=true");
    }
    if !letter.sender_email.is_empty() {
        options.push("fromemail=true");
    }

    let mut out = String::new();
    let _ = writeln!(out, "% Cover letter: {}", one_line(&label(job)));
    out += "% Written by job_radar. Change the text as you like, then build the PDF with:\n\
            %   latexmk -pdf cover_letter.tex    (or: pdflatex cover_letter.tex)\n";
    let _ = writeln!(out, "\\documentclass[{}]{{scrlttr2}}", options.join(", "));
    out += "\\usepackage[T1]{fontenc}\n\\usepackage{lmodern}\n";
    if let Some(lang) = babel_language(&letter.language) {
        let _ = writeln!(out, "\\usepackage[{lang}]{{babel}}");
    }

    out += "\n% Sender (from the CV)\n";
    let _ = writeln!(
        out,
        "\\setkomavar{{fromname}}{{{}}}",
        tex(&letter.sender_name)
    );
    if !letter.sender_address.is_empty() {
        let _ = writeln!(
            out,
            "\\setkomavar{{fromaddress}}{{{}}}",
            tex_lines(&letter.sender_address, "\\\\")
        );
    }
    if !letter.sender_phone.is_empty() {
        let _ = writeln!(
            out,
            "\\setkomavar{{fromphone}}{{{}}}",
            tex(&letter.sender_phone)
        );
    }
    if !letter.sender_email.is_empty() {
        let _ = writeln!(
            out,
            "\\setkomavar{{fromemail}}{{{}}}",
            tex(&letter.sender_email)
        );
    }
    let _ = writeln!(out, "\n\\setkomavar{{subject}}{{{}}}", tex(&letter.subject));

    out += "\n\\begin{document}\n";
    let _ = writeln!(
        out,
        "\\begin{{letter}}{{%\n  {}%\n}}",
        if letter.recipient.is_empty() {
            // scrlttr2 stops with an error on an empty address.
            "\\mbox{}% write the address of the company here".to_string()
        } else {
            tex_lines(&letter.recipient, "\\\\\n  ")
        }
    );
    let _ = writeln!(out, "\n\\opening{{{}}}\n", tex(&letter.opening));
    for p in &letter.paragraphs {
        let _ = writeln!(out, "{}\n", tex(p));
    }
    let _ = writeln!(out, "\\closing{{{}}}", tex(&letter.closing));
    if !letter.enclosures.is_empty() {
        let _ = writeln!(out, "\n\\encl{{{}}}", tex(&letter.enclosures));
    }
    out += "\\end{letter}\n\\end{document}\n";
    out
}

/// Escapes the special characters of LaTeX in plain text.
fn tex(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out += r"\textbackslash{}",
            '&' | '%' | '$' | '#' | '_' | '{' | '}' => {
                out.push('\\');
                out.push(c);
            }
            '~' => out += r"\textasciitilde{}",
            '^' => out += r"\textasciicircum{}",
            // With babel ngerman, " starts a shorthand ("a gives ä).
            '"' => out += r"\textquotedbl{}",
            _ => out.push(c),
        }
    }
    out
}

/// Escaped lines, joined with `sep` (a LaTeX line break).
fn tex_lines(lines: &[String], sep: &str) -> String {
    lines
        .iter()
        .map(|l| {
            let l = tex(l);
            // After \\, a "[" or "*" would be read as an option of the line break.
            if l.starts_with(['[', '*']) {
                format!("{{}}{l}")
            } else {
                l
            }
        })
        .collect::<Vec<_>>()
        .join(sep)
}

/// The readable job data for `job.txt`.
fn job_text(job: &JobRow, posting: &str) -> String {
    let mut out = String::new();
    for (name, value) in [
        ("Title", job.title.clone()),
        ("Company", job.company.clone()),
        ("Location", job.location.clone()),
        ("Job page", job.url.clone()),
        ("Source", job.source.clone()),
        (
            "Match score",
            job.match_score.map(|s| s.to_string()).unwrap_or_default(),
        ),
        ("Why it fits", job.match_reason.clone()),
    ] {
        if !value.is_empty() {
            let _ = writeln!(out, "{name}: {value}");
        }
    }
    if !job.description.is_empty() {
        let _ = write!(out, "\nDescription:\n{}\n", job.description);
    }
    if !posting.trim().is_empty() {
        let _ = write!(out, "\nJob posting (text of the job page):\n{posting}\n");
    }
    out
}

/// The folder for all applications. A leading `~` is the home folder.
pub fn applications_dir(dir: &str) -> PathBuf {
    let dir = dir.trim();
    let rest = match dir {
        "~" => Some(""),
        _ => dir.strip_prefix("~/").or_else(|| dir.strip_prefix("~\\")),
    };
    if let Some(rest) = rest
        && let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(if dir.is_empty() { "." } else { dir })
}

/// "nordlicht-robotics-rust-developer-m-w-d" from the company and the title (or the link).
fn folder_name(job: &JobRow) -> String {
    let name = format!("{} {}", job.company, job.title);
    if name.trim().is_empty() {
        dump::slug(&job.url)
    } else {
        dump::slug(&name)
    }
}

/// Creates a new folder `<parent>/<name>`. If it exists, the name gets "-2", "-3" and so on,
/// so that an older (maybe edited) letter is never overwritten.
fn new_folder(parent: &Path, name: &str) -> Res<PathBuf> {
    fs::create_dir_all(parent)
        .map_err(|e| format!("cannot create the folder {}: {e}", parent.display()))?;
    for n in 1.. {
        let dir = parent.join(if n == 1 {
            name.to_string()
        } else {
            format!("{name}-{n}")
        });
        match fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("cannot create the folder {}: {e}", dir.display()).into()),
        }
    }
    unreachable!()
}

fn write_file(path: &Path, text: &str) -> Res<()> {
    fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()).into())
}

/// The job for the `application` command: `key` is a row number of the CSV (1 = best match)
/// or a job link. A link that is not in the CSV gives a job with only that link.
pub fn find_job(rows: &[JobRow], key: &str) -> Res<JobRow> {
    let key = key.trim();
    if let Ok(n) = key.parse::<usize>() {
        return match n.checked_sub(1).and_then(|i| rows.get(i)) {
            Some(row) => Ok(row.clone()),
            None => Err(format!(
                "there is no job number {n}: the CSV file has {} jobs",
                rows.len()
            )
            .into()),
        };
    }
    if !key.starts_with("http://") && !key.starts_with("https://") {
        return Err(format!("'{key}' is not a job number or a link").into());
    }
    Ok(rows
        .iter()
        .find(|r| r.url == key)
        .cloned()
        .unwrap_or_else(|| JobRow {
            match_score: None,
            title: String::new(),
            company: String::new(),
            location: String::new(),
            description: String::new(),
            match_reason: String::new(),
            url: key.to_string(),
            source: String::new(),
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job() -> JobRow {
        JobRow {
            match_score: Some(85),
            title: "Rust Developer (m/w/d)".into(),
            company: "Nordlicht Robotics".into(),
            location: "Bochum".into(),
            description: "Builds robot software.".into(),
            match_reason: "Rust and embedded experience.".into(),
            url: "https://nordlicht.example/jobs/1".into(),
            source: "https://nordlicht.example/careers".into(),
        }
    }

    const REPLY: &str = r#"```json
{"language": "German", "sender_name": "Jane Doe",
 "sender_address": "Main St 1\n44787 Bochum", "sender_phone": "", "sender_email": "jane_doe@example.com",
 "recipient": ["Nordlicht Robotics GmbH", "[HR] Team"],
 "subject": "Bewerbung als Rust Developer", "opening": "Sehr geehrte Damen und Herren,",
 "paragraphs": ["I have 5 years of Rust & C.", "  ", "I \"love\" 100% of {it}."],
 "closing": "Mit freundlichen Grüßen", "enclosures": "Lebenslauf"}
```"#;

    #[test]
    fn parses_the_letter() {
        let letter = parse_letter(REPLY).unwrap();
        assert_eq!(letter.sender_address, ["Main St 1", "44787 Bochum"]);
        assert_eq!(letter.recipient, ["Nordlicht Robotics GmbH", "[HR] Team"]);
        assert_eq!(letter.paragraphs.len(), 2);

        let one_text = r#"{"paragraphs": "First.\n\nSecond\nline."}"#;
        assert_eq!(
            parse_letter(one_text).unwrap().paragraphs,
            ["First.", "Second line."]
        );
        assert!(parse_letter(r#"{"subject": "x"}"#).is_err());
        assert!(parse_letter("no json").is_err());
    }

    #[test]
    fn latex_escapes_text_and_uses_the_language() {
        let doc = latex(&parse_letter(REPLY).unwrap(), &job());
        assert!(doc.contains("\\usepackage[ngerman]{babel}"));
        assert!(doc.contains("I have 5 years of Rust \\& C.\n"));
        assert!(doc.contains("I \\textquotedbl{}love\\textquotedbl{} 100\\% of \\{it\\}."));
        assert!(doc.contains("\\setkomavar{fromaddress}{Main St 1\\\\44787 Bochum}"));
        assert!(doc.contains("\\setkomavar{fromemail}{jane\\_doe@example.com}"));
        assert!(doc.contains("Nordlicht Robotics GmbH\\\\\n  {}[HR] Team%"));
        // No phone number: no phone line and no phone option.
        assert!(!doc.contains("fromphone"));
        assert!(doc.contains("fromemail=true"));
        assert!(doc.ends_with("\\end{letter}\n\\end{document}\n"));

        let other = Letter {
            language: "Klingon".into(),
            paragraphs: vec!["x".into()],
            ..Letter::default()
        };
        let doc = latex(&other, &job());
        assert!(!doc.contains("babel"));
        assert!(doc.contains("\\begin{letter}{%\n  \\mbox{}% write the address"));
        assert_eq!(babel_language("English (US)"), Some("english"));
        assert_eq!(
            tex(r"a\b~c^d#e$"),
            r"a\textbackslash{}b\textasciitilde{}c\textasciicircum{}d\#e\$"
        );
    }

    #[test]
    fn each_application_gets_a_new_folder() {
        let parent =
            std::env::temp_dir().join(format!("job_radar_test_apps_{}", std::process::id()));
        let _ = fs::remove_dir_all(&parent);
        let name = folder_name(&job());
        assert_eq!(name, "nordlicht-robotics-rust-developer-m-w-d");
        let first = new_folder(&parent, &name).unwrap();
        let second = new_folder(&parent, &name).unwrap();
        assert_eq!(first, parent.join(&name));
        assert_eq!(second, parent.join(format!("{name}-2")));
        fs::remove_dir_all(&parent).unwrap();
    }

    #[test]
    fn home_folder_is_expanded() {
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        assert_eq!(
            applications_dir("~/Bewerbungen"),
            PathBuf::from(&home).join("Bewerbungen")
        );
        assert_eq!(
            applications_dir("applications"),
            PathBuf::from("applications")
        );
        assert_eq!(applications_dir(" "), PathBuf::from("."));
    }

    #[test]
    fn finds_the_job_by_number_or_link() {
        let rows = vec![job()];
        assert_eq!(find_job(&rows, "1").unwrap().company, "Nordlicht Robotics");
        assert_eq!(
            find_job(&rows, "https://nordlicht.example/jobs/1")
                .unwrap()
                .title,
            "Rust Developer (m/w/d)"
        );
        let other = find_job(&rows, "https://other.example/job").unwrap();
        assert_eq!(other.url, "https://other.example/job");
        assert!(other.title.is_empty());
        assert!(find_job(&rows, "0").is_err());
        assert!(find_job(&rows, "2").is_err());
        assert!(find_job(&rows, "nordlicht").is_err());
    }

    #[test]
    fn template_file_sets_sender_and_layout() {
        let text = fs::read_to_string("letter.example.yaml").unwrap();
        let template = LetterTemplate::parse(&text).unwrap();
        let mut letter = parse_letter(REPLY).unwrap();
        template.apply(&mut letter);
        assert_eq!(letter.sender_name, "Jane Doe");
        assert_eq!(letter.sender_address, ["Hauptstraße 1", "44787 Bochum"]);
        assert_eq!(letter.sender_email, "jane.doe@example.com");
        assert_eq!(letter.enclosures, "Lebenslauf, Zeugnisse");

        let doc = template.render(&letter, &job()).unwrap();
        assert!(doc.contains("\\usepackage[ngerman]{babel}"));
        assert!(doc.contains("\\setkomavar{fromname}{Jane Doe}"));
        assert!(doc.contains("\\setkomavar{fromaddress}{Hauptstraße 1\\\\44787 Bochum}"));
        assert!(doc.contains("\\begin{letter}{Nordlicht Robotics GmbH\\\\\n{}[HR] Team}"));
        assert!(doc.contains("I have 5 years of Rust \\& C.\n\nI \\textquotedbl{}"));
        assert!(!doc.contains("{{"), "{doc}");

        // Without a template, the built-in layout applies; the sender still comes from the file.
        let sender_only = LetterTemplate::parse("sender:\n  name: Max\n").unwrap();
        let mut letter = parse_letter(REPLY).unwrap();
        sender_only.apply(&mut letter);
        assert_eq!(letter.sender_address, ["Main St 1", "44787 Bochum"]);
        assert_eq!(
            sender_only.render(&letter, &job()).unwrap(),
            latex(&letter, &job())
        );

        let err = |t: &str| LetterTemplate::parse(t).unwrap_err().to_string();
        assert!(err("template: \\begin{document}x\\end{document}").contains("{{ body }}"));
        assert!(err("sendr: {}").contains("unknown field"));
        assert!(LetterTemplate::parse("").unwrap().template.is_empty());
    }

    #[test]
    fn long_posting_is_cut_at_a_char_boundary() {
        assert_eq!(cut("äöü", 3), "ä");
        assert_eq!(cut("abc", 10), "abc");
    }
}
