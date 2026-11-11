use std::env;
use std::io::{self, IsTerminal, Read};

use crate::Res;

/// Command-line options.
pub struct Args {
    pub cv: String,
    pub urls: Vec<String>,
    pub out: String,
    pub kill_dups: bool,
}

const USAGE: &str = "\
Usage: job_radar [--cv <cv.txt|cv.pdf>] [--out <jobs.csv>] [--kill-dups] [<url> ...]
       job_radar convert-cv <cv.pdf> [<cv.txt>]
       job_radar page-text <url>

Without URLs, the enabled sources in sources.yaml are read.
`page-text` prints the text the AI would see (no AI call), to test a new source.
`--kill-dups` makes one more AI call that merges the same job found on several platforms.

The CV comes from --cv, or from stdin if no --cv is given:
  pbpaste | job_radar https://example.com/careers
  job_radar --cv cv.md --out jobs.csv https://a.com/jobs https://b.com/careers";

/// Parses the arguments. Prints usage and exits if they are incomplete.
pub fn parse_args() -> Res<Args> {
    let mut cv_path = None;
    let mut out = "jobs.csv".to_string();
    let mut urls = Vec::new();
    let mut kill_dups = false;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--cv" => cv_path = Some(args.next().ok_or("--cv needs a file path")?),
            "--out" | "-o" => out = args.next().ok_or("--out needs a file path")?,
            "--kill-dups" => kill_dups = true,
            "-h" | "--help" | "help" => print_usage_and_exit(),
            _ if arg.starts_with('-') => return Err(format!("unknown option: {arg}").into()),
            _ => urls.push(arg),
        }
    }

    let cv = match cv_path {
        Some(path) => crate::cv::read(&path)?,
        None => read_piped_input()?,
    };
    if cv.trim().is_empty() {
        eprintln!("error: no CV given: use --cv <file> or pipe the CV into stdin\n\n{USAGE}");
        std::process::exit(1);
    }

    Ok(Args { cv, urls, out, kill_dups })
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
