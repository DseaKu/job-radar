mod backend;
mod config;
mod cv;
mod fetch;
mod input;
mod jobs;

use std::time::Duration;

use backend::Backend;
use config::Config;

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
    let config = Config::load()?;

    // URLs on the command line win; otherwise use the enabled sources from the config.
    let targets: Vec<(String, String)> = if args.urls.is_empty() {
        config::load_sources()?
            .iter()
            .filter(|s| s.enabled)
            .map(|s| (s.name.clone(), s.url.clone()))
            .collect()
    } else {
        args.urls.iter().map(|u| (u.clone(), u.clone())).collect()
    };
    if targets.is_empty() {
        return Err("no URLs given and no enabled sources in sources.yaml \
                    (create it with: cp sources.example.yaml sources.yaml)"
            .into());
    }

    let backend = Backend::from_config(&config)?;
    eprintln!("Using backend: {}", backend.describe());

    let mut rows = Vec::new();
    let mut failed = 0;
    for (name, url) in &targets {
        eprintln!("Reading {name} ...");
        match jobs::from_url(&backend, &http, &args.cv, url, &config.output_language).await {
            Ok(found) => {
                eprintln!("  {} jobs found", found.len());
                rows.extend(found);
            }
            Err(e) => {
                eprintln!("  error: {e}");
                failed += 1;
            }
        }
    }

    let count = rows.len();
    jobs::write_csv(&args.out, rows)?;
    eprintln!("Wrote {count} jobs to {}", args.out);
    eprintln!("{}", backend.usage_summary());

    if failed == targets.len() {
        return Err("all URLs failed".into());
    }
    Ok(())
}
