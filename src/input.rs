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
}

const USAGE: &str = "\
Usage: job_radar [--cv <cv.txt|cv.pdf>] [--out <jobs.csv>] [--kill-dups] [-v] [<url> ...]
       job_radar convert-cv <cv.pdf> [<cv.txt>]
       job_radar page-text <url>

Without URLs, the enabled sources in sources.yaml are read.
`page-text` prints the text the AI would see (no AI call), to test a new source.
`--kill-dups` makes one more AI call that merges the same job found on several platforms.
`-v`, `--verbose` shows the progress, the token usage and a table of the best jobs.
Without it, only warnings, errors and the result line are shown.

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
        help: false,
    };
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--cv" => flags.cv_path = Some(args.next().ok_or("--cv needs a file path")?),
            "--out" | "-o" => flags.out = args.next().ok_or("--out needs a file path")?,
            "--kill-dups" => flags.kill_dups = true,
            "-v" | "--verbose" => flags.verbose = true,
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

    let cv = match &flags.cv_path {
        Some(path) => crate::cv::read(path)?,
        None => read_piped_input()?,
    };
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
        assert!(f.cv_path.is_none() && f.urls.is_empty() && !f.kill_dups && !f.verbose && !f.help);
    }

    #[test]
    fn all_options_in_any_order() {
        let f = parse(&[
            "https://a.de/jobs",
            "-v",
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
        assert!(f.kill_dups && f.verbose && !f.help);
        assert!(parse(&["--verbose", "--out", "x.csv"]).unwrap().verbose);
        assert!(parse(&["help"]).unwrap().help);
    }

    #[test]
    fn errors() {
        let err = |args: &[&str]| parse(args).unwrap_err().to_string();
        assert_eq!(err(&["--cv"]), "--cv needs a file path");
        assert_eq!(err(&["--out"]), "--out needs a file path");
        assert_eq!(err(&["--verbos"]), "unknown option: --verbos");
    }
}
