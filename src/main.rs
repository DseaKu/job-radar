use std::sync::atomic::{AtomicBool, Ordering};

/// Set by `-v` / `--verbose`. Without it, only warnings, errors and the result line are shown.
static VERBOSE: AtomicBool = AtomicBool::new(false);

pub fn verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

/// Progress message: printed to stderr only with `-v` / `--verbose`.
macro_rules! info {
    ($($arg:tt)*) => {
        if $crate::verbose() {
            eprintln!($($arg)*);
        }
    };
}

mod backend;
mod config;
mod cv;
mod dedup;
mod fetch;
mod ignore;
mod input;
mod jobs;

use std::time::Duration;

use backend::{Backend, Usage};
use config::Config;

/// How many of the best jobs the `--print-results` table shows.
const TABLE_ROWS: usize = 10;

pub type Res<T> = Result<T, Box<dyn std::error::Error>>;

#[tokio::main]
async fn main() -> Res<()> {
    // Load secrets (API tokens) from a .env file if one exists.
    dotenvy::dotenv().ok();

    // Offline subcommand: needs no provider and no API key.
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.first().map(String::as_str) == Some("convert-cv") {
        return cv::convert_command(&argv[1..]);
    }

    let http = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (compatible; job_radar/0.1)")
        .timeout(Duration::from_secs(30))
        .build()?;

    // Offline subcommand: shows the page text that the AI would get.
    if argv.first().map(String::as_str) == Some("page-text") {
        let url = argv.get(1).ok_or("usage: job_radar page-text <url>")?;
        let text = fetch::page_text(&http, url).await?;
        println!("{text}");
        eprintln!("({} chars)", text.chars().count());
        return Ok(());
    }

    let args = input::parse_args()?;
    VERBOSE.store(args.verbose, Ordering::Relaxed);
    let config = Config::load()?;
    let ignore_rules = ignore::load()?;

    // URLs on the command line win; otherwise use the enabled sources from the config.
    let targets = select_targets(&args.urls, config::load_sources)?;

    let backend = Backend::from_config(&config)?;
    info!("Using backend: {}", backend.describe());

    let mut rows = Vec::new();
    let mut failed = 0;
    // Token usage of each step, for the table at the end.
    let mut usage: Vec<(String, Usage)> = Vec::new();
    for (name, url) in &targets {
        info!("Reading {name} ...");
        let before = backend.usage();
        match jobs::from_url(&backend, &http, &args.cv, url, &config.output_language).await {
            Ok(found) => {
                info!("  {} jobs found", found.len());
                rows.extend(found);
            }
            Err(e) => {
                if verbose() {
                    eprintln!("  error: {e}");
                } else {
                    eprintln!("error: {name}: {e}");
                }
                failed += 1;
            }
        }
        usage.push((name.clone(), backend.usage() - before));
    }

    // Remove ignored jobs before the dedup step, so that it gets fewer rows.
    if !ignore_rules.is_empty() {
        let removed = ignore::apply(&mut rows, &ignore_rules);
        info!(
            "{removed} jobs ignored (ignore list: {} entries)",
            ignore_rules.len()
        );
    }

    if args.kill_dups && rows.len() > 1 {
        info!("Removing duplicates ...");
        let before = rows.len();
        let usage_before = backend.usage();
        let result = dedup::find_dups(&backend, &rows).await;
        usage.push((
            "duplicate check".to_string(),
            backend.usage() - usage_before,
        ));
        match result {
            Ok(groups) => {
                rows = dedup::merge(rows, groups);
                info!("  {} duplicates removed", before - rows.len());
            }
            Err(e) => eprintln!("  warning: duplicates not removed: {e}"),
        }
    }

    jobs::write_csv(&args.out, &mut rows)?;
    eprintln!("Wrote {} jobs to {}", rows.len(), args.out);
    if verbose() {
        eprint!("\n{}", usage_table(&usage));
    }
    if args.print_results && !rows.is_empty() {
        print!("\n{}", jobs::table(&rows, TABLE_ROWS));
    }

    if failed == targets.len() {
        return Err("all URLs failed".into());
    }
    Ok(())
}

/// The pages to read, as (name for the log, URL). URLs on the command line win; otherwise the
/// enabled entries from sources.yaml are used. `load_sources` is called only without URLs.
fn select_targets(
    urls: &[String],
    load_sources: impl FnOnce() -> Res<Vec<config::Source>>,
) -> Res<Vec<(String, String)>> {
    let targets: Vec<(String, String)> = if urls.is_empty() {
        load_sources()?
            .into_iter()
            .filter(|s| s.enabled)
            .map(|s| (s.name, s.url))
            .collect()
    } else {
        urls.iter().map(|u| (u.clone(), u.clone())).collect()
    };
    if targets.is_empty() {
        return Err("no URLs given and no enabled sources in sources.yaml \
                    (create it with: cp sources.example.yaml sources.yaml)"
            .into());
    }
    Ok(targets)
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
    use config::Source;

    fn source(name: &str, enabled: bool) -> Source {
        Source {
            name: name.into(),
            url: format!("https://{name}.de"),
            enabled,
        }
    }

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

    #[test]
    fn command_line_urls_win_and_sources_are_not_loaded() {
        let urls = vec!["https://x.de/jobs".to_string()];
        let targets = select_targets(&urls, || panic!("sources must not be loaded")).unwrap();
        assert_eq!(targets, [(urls[0].clone(), urls[0].clone())]);
    }

    #[test]
    fn only_enabled_sources_are_read() {
        let targets =
            select_targets(&[], || Ok(vec![source("a", true), source("b", false)])).unwrap();
        assert_eq!(targets, [("a".to_string(), "https://a.de".to_string())]);
    }

    #[test]
    fn nothing_to_read_is_an_error() {
        let err = select_targets(&[], || Ok(vec![source("b", false)])).unwrap_err();
        assert!(
            err.to_string()
                .starts_with("no URLs given and no enabled sources")
        );
        assert!(select_targets(&[], || Err("invalid sources.yaml".into())).is_err());
    }
}
