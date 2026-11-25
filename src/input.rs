use std::env;
use std::io::{self, IsTerminal, Read};

use crate::Res;

/// Command-line options.
pub struct Args {
    pub cv: String,
    pub urls: Vec<String>,
    pub out: String,
    pub kill_dups: bool,
    pub verbose: bool,
    pub print_results: bool,
    pub dump: Option<String>,
}

const USAGE: &str = "\
Usage: job_radar [--cv <cv.txt|cv.pdf>] [--out <jobs.csv>] [--kill-dups] [-v] [-p] [--dump <dir>] [<url> ...]
       job_radar application [--cv <cv.txt|cv.pdf>] [--jobs <jobs.csv>] <number|url>
       job_radar convert-cv <cv.pdf> [<cv.txt>]
       job_radar page-text <url>
       job_radar init

Without URLs, the enabled sources in sources.yaml are read.
`page-text` prints the text the AI would see (no AI call), to test a new source.
`init` creates the missing settings files (config.yaml, sources.yaml, ignore.yaml,
letter.yaml) from the examples. A search also asks for this when files are missing.
`--kill-dups` makes one more AI call that merges the same job found on several platforms.
`-v`, `--verbose` shows the progress and the token usage.
`-p`, `--print-results` prints a table of the best jobs to stdout.
Without them, only warnings, errors and the result line are shown.
`--dump <dir>` writes two files for each website into <dir>: the page text that the AI got
(<name>.page.txt) and all jobs found on it, before the ignore list (<name>.jobs.txt).
`application` writes a cover letter as a LaTeX file into a new folder in `applications_dir`
(config.yaml). The job is a row number of the CSV file (1 = best match) or a job link.

The CV comes from --cv, or from stdin if no --cv is given:
  pbpaste | job_radar https://example.com/careers
  job_radar --cv cv.md --out jobs.csv https://a.com/jobs https://b.com/careers";

/// The options from the command line, before the CV is read.
#[derive(Debug, PartialEq)]
struct Flags {
    cv_path: Option<String>,
    out: String,
    urls: Vec<String>,
    kill_dups: bool,
    verbose: bool,
    print_results: bool,
    dump: Option<String>,
    help: bool,
}

/// Reads the options from the arguments (without the program name).
fn parse_flags(args: impl IntoIterator<Item = String>) -> Res<Flags> {
    let mut flags = Flags {
        cv_path: None,
        out: "jobs.csv".to_string(),
        urls: Vec::new(),
        kill_dups: false,
        verbose: false,
        print_results: false,
        dump: None,
        help: false,
    };
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--cv" => flags.cv_path = Some(args.next().ok_or("--cv needs a file path")?),
            "--out" | "-o" => flags.out = args.next().ok_or("--out needs a file path")?,
            "--kill-dups" => flags.kill_dups = true,
            "-v" | "--verbose" => flags.verbose = true,
            "-p" | "--print-results" => flags.print_results = true,
            "--dump" => flags.dump = Some(args.next().ok_or("--dump needs a folder")?),
            "-h" | "--help" | "help" => flags.help = true,
            _ if arg.starts_with('-') => return Err(format!("unknown option: {arg}").into()),
            _ => flags.urls.push(arg),
        }
    }
    Ok(flags)
}

/// Parses the arguments and reads the CV. Prints usage and exits if they are incomplete.
pub fn parse_args() -> Res<Args> {
    let flags = parse_flags(env::args().skip(1))?;
    if flags.help {
        print_usage_and_exit();
    }

    let cv = read_cv(flags.cv_path.as_deref())?;
    if cv.trim().is_empty() {
        eprintln!("error: no CV given: use --cv <file> or pipe the CV into stdin\n\n{USAGE}");
        std::process::exit(1);
    }

    Ok(Args {
        cv,
        urls: flags.urls,
        out: flags.out,
        kill_dups: flags.kill_dups,
        verbose: flags.verbose,
        print_results: flags.print_results,
        dump: flags.dump,
    })
}

/// The CV from the file, or from stdin if no file is given. Empty if there is none.
fn read_cv(path: Option<&str>) -> Res<String> {
    match path {
        Some(path) => crate::cv::read(path),
        None => Ok(read_piped_input()?),
    }
}

/// Options of the `application` command.
pub struct ApplicationArgs {
    pub cv: String,
    /// The CSV file with the jobs.
    pub jobs: String,
    /// A row number of the CSV file (1 = best match) or a job link.
    pub job: String,
}

#[derive(Debug, PartialEq)]
struct ApplicationFlags {
    cv_path: Option<String>,
    jobs: String,
    job: Option<String>,
    help: bool,
}

fn parse_application_flags(args: impl IntoIterator<Item = String>) -> Res<ApplicationFlags> {
    let mut flags = ApplicationFlags {
        cv_path: None,
        jobs: "jobs.csv".to_string(),
        job: None,
        help: false,
    };
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--cv" => flags.cv_path = Some(args.next().ok_or("--cv needs a file path")?),
            "--jobs" => flags.jobs = args.next().ok_or("--jobs needs a file path")?,
            "-h" | "--help" => flags.help = true,
            _ if arg.starts_with('-') => return Err(format!("unknown option: {arg}").into()),
            _ if flags.job.is_some() => return Err("give only one job number or link".into()),
            _ => flags.job = Some(arg),
        }
    }
    Ok(flags)
}

/// Parses the arguments of the `application` command (after its name) and reads the CV.
pub fn parse_application_args(args: &[String]) -> Res<ApplicationArgs> {
    let flags = parse_application_flags(args.iter().cloned())?;
    if flags.help {
        print_usage_and_exit();
    }
    let job = flags.job.ok_or(
        "usage: job_radar application [--cv <cv.txt|cv.pdf>] [--jobs <jobs.csv>] <number|url>",
    )?;
    let cv = read_cv(flags.cv_path.as_deref())?;
    if cv.trim().is_empty() {
        return Err("no CV given: use --cv <file> or pipe the CV into stdin".into());
    }
    Ok(ApplicationArgs {
        cv,
        jobs: flags.jobs,
        job,
    })
}

/// Prints the usage text to stdout and exits successfully (for --help).
fn print_usage_and_exit() -> ! {
    println!("{USAGE}");
    std::process::exit(0);
}

/// Reads everything from stdin if it is piped, otherwise returns an empty string.
fn read_piped_input() -> io::Result<String> {
    let stdin = io::stdin();
    if stdin.is_terminal() {
        return Ok(String::new());
    }
    let mut buf = String::new();
    stdin.lock().read_to_string(&mut buf)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Res<Flags> {
        parse_flags(args.iter().map(|a| a.to_string()))
    }

    #[test]
    fn defaults() {
        let f = parse(&[]).unwrap();
        assert_eq!(f.out, "jobs.csv");
        assert!(
            f.cv_path.is_none()
                && f.urls.is_empty()
                && !f.kill_dups
                && !f.verbose
                && !f.print_results
                && !f.help
        );
    }

    #[test]
    fn all_options_in_any_order() {
        let f = parse(&[
            "https://a.de/jobs",
            "-v",
            "-p",
            "--cv",
            "cv.txt",
            "-o",
            "out.csv",
            "--kill-dups",
            "https://b.de/careers",
        ])
        .unwrap();
        assert_eq!(f.cv_path.as_deref(), Some("cv.txt"));
        assert_eq!(f.out, "out.csv");
        assert_eq!(f.urls, ["https://a.de/jobs", "https://b.de/careers"]);
        assert!(f.kill_dups && f.verbose && f.print_results && !f.help);
        assert!(parse(&["--verbose", "--out", "x.csv"]).unwrap().verbose);
        assert!(parse(&["--print-results"]).unwrap().print_results);
        assert_eq!(
            parse(&["--dump", "dump"]).unwrap().dump.as_deref(),
            Some("dump")
        );
        assert!(parse(&["help"]).unwrap().help);
    }

    #[test]
    fn errors() {
        let err = |args: &[&str]| parse(args).unwrap_err().to_string();
        assert_eq!(err(&["--cv"]), "--cv needs a file path");
        assert_eq!(err(&["--out"]), "--out needs a file path");
        assert_eq!(err(&["--verbos"]), "unknown option: --verbos");
        assert_eq!(err(&["--dump"]), "--dump needs a folder");
    }

    #[test]
    fn application_options() {
        let parse = |args: &[&str]| parse_application_flags(args.iter().map(|a| a.to_string()));
        let f = parse(&["3", "--cv", "cv.pdf"]).unwrap();
        assert_eq!(f.job.as_deref(), Some("3"));
        assert_eq!(f.cv_path.as_deref(), Some("cv.pdf"));
        assert_eq!(f.jobs, "jobs.csv");
        assert_eq!(parse(&["--jobs", "a.csv"]).unwrap().jobs, "a.csv");
        assert!(parse(&[]).unwrap().job.is_none());
        let err = |args: &[&str]| parse(args).unwrap_err().to_string();
        assert_eq!(err(&["1", "2"]), "give only one job number or link");
        assert_eq!(err(&["--out", "x"]), "unknown option: --out");
    }
}
