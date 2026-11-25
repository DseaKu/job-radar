mod input;
mod setup;

use std::process::ExitCode;

use job_radar::backend::{Backend, Usage};
use job_radar::config::{self, Config};
use job_radar::{Level, Res, application, cv, fetch, ignore, jobs, run};

/// How many of the best jobs the `--print-results` table shows.
const TABLE_ROWS: usize = 10;

#[tokio::main]
async fn main() -> ExitCode {
    match cli().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // Display, not Debug: no quotation marks around the message.
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn cli() -> Res<()> {
    // Load secrets (API tokens) from a .env file if one exists.
    dotenvy::dotenv().ok();

    // Offline subcommand: needs no provider and no API key.
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.first().map(String::as_str) == Some("convert-cv") {
        return cv::convert_command(&argv[1..]);
    }
    if argv.first().map(String::as_str) == Some("init") {
        return setup::init();
    }

    let http = job_radar::http_client()?;

    // Offline subcommand: shows the page text that the AI would get.
    if argv.first().map(String::as_str) == Some("page-text") {
        let url = argv.get(1).ok_or("usage: job_radar page-text <url>")?;
        let text = fetch::page_text(&http, url).await?;
        println!("{text}");
        eprintln!("({} chars)", text.chars().count());
        return Ok(());
    }

    if argv.first().map(String::as_str) == Some("application") {
        return application_command(&argv[1..], &http).await;
    }

    let args = input::parse_args()?;
    // URLs on the command line make sources.yaml unnecessary.
    if args.urls.is_empty() {
        setup::offer(&[&setup::CONFIG, &setup::SOURCES, &setup::IGNORE])?;
    } else {
        setup::offer(&[&setup::CONFIG, &setup::IGNORE])?;
    }
    // Without -v, only warnings and errors are shown.
    let verbose = args.verbose;
    job_radar::set_logger(move |level, msg| {
        if verbose || level == Level::Warn {
            eprintln!("{msg}");
        }
    });
    let config = Config::load()?;
    let ignore_rules = ignore::load()?;

    // URLs on the command line win; otherwise use the enabled sources from the config.
    let targets = run::targets(&args.urls, config::load_sources)?;
    let opts = run::Options {
        cv: &args.cv,
        targets: &targets,
        kill_dups: args.kill_dups,
        dump: args.dump.as_deref(),
    };
    let backend = Backend::from_config(&config)?;
    let mut outcome = run::run(&backend, &config, &ignore_rules, &opts, &http, |_| {}).await?;

    jobs::write_csv(&args.out, &mut outcome.rows)?;
    eprintln!("Wrote {} jobs to {}", outcome.rows.len(), args.out);
    if verbose {
        eprint!("\n{}", usage_table(&outcome.usage));
    }
    if args.print_results && !outcome.rows.is_empty() {
        print!("\n{}", jobs::table(&outcome.rows, TABLE_ROWS));
    }
    Ok(())
}

/// `job_radar application <number|url>`: writes a cover letter for one job.
async fn application_command(argv: &[String], http: &reqwest::Client) -> Res<()> {
    let args = input::parse_application_args(argv)?;
    setup::offer(&[&setup::CONFIG, &setup::LETTER])?;
    // One short step: show all progress messages.
    job_radar::set_logger(|_, msg| eprintln!("{msg}"));
    // A job link works without a CSV file; a row number needs one.
    let rows = if std::path::Path::new(&args.jobs).exists() {
        jobs::read_csv(&args.jobs)?
    } else if args.job.trim().parse::<usize>().is_ok() {
        return Err(format!(
            "{} does not exist: run a search first, or give a job link",
            args.jobs
        )
        .into());
    } else {
        Vec::new()
    };
    let job = application::find_job(&rows, &args.job)?;
    let config = Config::load()?;
    let backend = Backend::from_config(&config)?;
    let written = application::write(&backend, http, &config, &args.cv, &job).await?;
    eprintln!("Wrote {}", written.letter.display());
    if let Some(pdf) = &written.pdf {
        eprintln!("Wrote {}", pdf.display());
    }
    Ok(())
}

/// A table of the token usage of each step, with a total row if there is more than one step.
fn usage_table(steps: &[(String, Usage)]) -> String {
    const MAX_NAME: usize = 30;
    let name = |n: &str| {
        if n.chars().count() <= MAX_NAME {
            n.to_string()
        } else {
            format!("{}…", n.chars().take(MAX_NAME - 1).collect::<String>())
        }
    };
    let total = steps
        .iter()
        .fold(Usage::default(), |sum, (_, u)| sum + u.to_owned());
    let mut lines: Vec<(String, Usage)> = steps.iter().map(|(n, u)| (name(n), *u)).collect();
    if steps.len() > 1 {
        lines.push(("total".to_string(), total));
    }
    let w = lines
        .iter()
        .map(|(n, _)| n.chars().count())
        .fold("Token usage".len(), usize::max);

    let mut out = format!(
        "{:<w$}  {:>9}  {:>9}  {:>5}\n",
        "Token usage", "input", "output", "calls"
    );
    for (n, u) in &lines {
        out += &format!("{n:<w$}  {:>9}  {:>9}  {:>5}\n", u.input, u.output, u.calls);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_table_has_one_row_per_step_and_a_total() {
        let u = |input, output, calls| Usage {
            input,
            output,
            calls,
        };
        let text = usage_table(&[
            ("Arbeitsagentur".into(), u(5501, 2597, 1)),
            ("StepStone".into(), u(12000, 3000, 2)),
            ("duplicate check".into(), u(800, 50, 1)),
        ]);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0], "Token usage          input     output  calls");
        assert_eq!(lines[1], "Arbeitsagentur        5501       2597      1");
        assert_eq!(lines[4], "total                18301       5647      4");
        // One step: no total row.
        assert_eq!(usage_table(&[("A".into(), u(1, 2, 1))]).lines().count(), 2);
    }
}
