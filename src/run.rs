//! One complete search: read each page, find and rate the jobs, apply the ignore list and
//! (optionally) remove duplicates. The CLI and the desktop app call [`run`].

use serde::Serialize;

use crate::Res;
use crate::backend::{Backend, ProviderUnusable, Usage};
use crate::config::{Config, Source};
use crate::ignore::{self, Rule};
use crate::jobs::{self, JobRow};
use crate::{dedup, dump, fetch};

/// What [`run`] needs besides the config.
pub struct Options<'a> {
    /// The CV as text.
    pub cv: &'a str,
    /// The pages to read, as (name, URL). See [`targets`].
    pub targets: &'a [(String, String)],
    /// One more AI call that merges the same job found on several pages.
    pub kill_dups: bool,
    /// Folder for the `--dump` files, if wanted.
    pub dump: Option<&'a str>,
}

/// A step of the run, for a progress display.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum Progress {
    /// Reading page `index` (starts at 1) of `total`.
    Source {
        index: usize,
        total: usize,
        name: String,
    },
    /// A page is done: the number of jobs found, or the error.
    SourceDone {
        name: String,
        jobs: usize,
        error: Option<String>,
    },
    /// The duplicate check starts.
    Dedup,
}

/// The result of a run.
pub struct Outcome {
    /// The jobs, best match first.
    pub rows: Vec<JobRow>,
    /// Token usage of each step (each page, then the duplicate check).
    pub usage: Vec<(String, Usage)>,
    /// How many pages failed.
    pub failed: usize,
}

/// The pages to read, as (name for the log, URL). URLs given by the user win; otherwise the
/// enabled entries from sources.yaml are used. `load_sources` is called only without URLs.
pub fn targets(
    urls: &[String],
    load_sources: impl FnOnce() -> Res<Vec<Source>>,
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
        return Err("no URLs given and no enabled sources in sources.yaml".into());
    }
    Ok(targets)
}

/// Runs one search. Returns an error only if the setup fails or all pages fail.
pub async fn run(
    backend: &Backend,
    config: &Config,
    ignore_rules: &[Rule],
    opts: &Options<'_>,
    http: &reqwest::Client,
    on_progress: impl Fn(Progress),
) -> Res<Outcome> {
    info!("Using backend: {}", backend.describe());

    let mut rows = Vec::new();
    let mut failed = 0;
    let mut usage: Vec<(String, Usage)> = Vec::new();
    let mut dump = opts.dump.map(dump::Dump::new).transpose()?;
    let total = opts.targets.len();
    for (i, (name, url)) in opts.targets.iter().enumerate() {
        info!("Reading {name} ...");
        on_progress(Progress::Source {
            index: i + 1,
            total,
            name: name.clone(),
        });
        let before = backend.usage();
        let stem = dump.as_mut().map(|d| d.stem(name));
        let result = async {
            let text = fetch::page_text(http, url).await?;
            if let (Some(d), Some(stem)) = (&dump, &stem)
                && let Err(e) = d.write_page(stem, &text)
            {
                warn!("warning: {e}");
            }
            jobs::from_text(
                backend,
                opts.cv,
                url,
                &text,
                &config.output_language,
                &config.extra_instructions,
            )
            .await
        }
        .await;
        if let (Some(d), Some(stem)) = (&dump, &stem)
            && let Err(e) = d.write_jobs(stem, name, url, &result, ignore_rules)
        {
            warn!("warning: {e}");
        }
        match result {
            Ok(found) => {
                info!("  {} jobs found", found.len());
                on_progress(Progress::SourceDone {
                    name: name.clone(),
                    jobs: found.len(),
                    error: None,
                });
                rows.extend(found);
            }
            // A rejected API key or no connection to the AI fails each page the same way: stop.
            Err(e) if e.is::<ProviderUnusable>() => {
                on_progress(Progress::SourceDone {
                    name: name.clone(),
                    jobs: 0,
                    error: Some(e.to_string()),
                });
                return Err(e);
            }
            Err(e) => {
                warn!("error: {name}: {e}");
                on_progress(Progress::SourceDone {
                    name: name.clone(),
                    jobs: 0,
                    error: Some(e.to_string()),
                });
                failed += 1;
            }
        }
        usage.push((name.clone(), backend.usage() - before));
    }

    // Remove ignored jobs before the dedup step, so that it gets fewer rows.
    if !ignore_rules.is_empty() {
        let removed = ignore::apply(&mut rows, ignore_rules);
        info!(
            "{removed} jobs ignored (ignore list: {} entries)",
            ignore_rules.len()
        );
    }

    if opts.kill_dups && rows.len() > 1 {
        info!("Removing duplicates ...");
        on_progress(Progress::Dedup);
        let before = rows.len();
        let usage_before = backend.usage();
        let result = dedup::find_dups(backend, &rows).await;
        usage.push((
            "duplicate check".to_string(),
            backend.usage() - usage_before,
        ));
        match result {
            Ok(groups) => {
                rows = dedup::merge(rows, groups);
                info!("  {} duplicates removed", before - rows.len());
            }
            Err(e) => warn!("  warning: duplicates not removed: {e}"),
        }
    }

    if total > 0 && failed == total {
        return Err("all URLs failed".into());
    }
    jobs::sort(&mut rows);
    Ok(Outcome {
        rows,
        usage,
        failed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(name: &str, enabled: bool) -> Source {
        Source {
            name: name.into(),
            url: format!("https://{name}.de"),
            enabled,
        }
    }

    /// A local server: GET gives a small career page, POST (an AI call) gives 401.
    /// Returns the address and the number of AI calls.
    async fn server_with_wrong_key() -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let calls = std::sync::Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let counter = counter.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 65536];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let (status, body) = if buf[..n].starts_with(b"GET") {
                        (
                            "200 OK",
                            "<html><body><p>Developer in Witten</p></body></html>",
                        )
                    } else {
                        counter.fetch_add(1, Ordering::SeqCst);
                        (
                            "401 Unauthorized",
                            r#"{"error":{"message":"invalid x-api-key"}}"#,
                        )
                    };
                    let reply = format!(
                        "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(reply.as_bytes()).await;
                });
            }
        });
        (format!("http://{addr}"), calls)
    }

    /// Runs a search over two pages ("a" and "b") of `pages` with the AI at `ai_url`.
    async fn search_with_bad_ai(
        pages: &str,
        ai_url: &str,
    ) -> (Box<dyn std::error::Error + Send + Sync>, Vec<Progress>) {
        let config: Config = serde_yaml::from_str(&format!(
            "active: p\nproviders:\n  p:\n    type: openai\n    model: m\n    \
             base_url: {ai_url}\n    api_key_env: TEST_KEY\n"
        ))
        .unwrap();
        let backend = Backend::from_config_with_keys(&config, |_| Some("wrong".into())).unwrap();
        let targets = vec![
            ("a".to_string(), format!("{pages}/a")),
            ("b".to_string(), format!("{pages}/b")),
        ];
        let opts = Options {
            cv: "CV",
            targets: &targets,
            kill_dups: false,
            dump: None,
        };
        let steps = std::sync::Mutex::new(Vec::new());
        let http = crate::http_client().unwrap();
        let result = run(&backend, &config, &[], &opts, &http, |p| {
            steps.lock().unwrap().push(p)
        })
        .await;
        let Err(err) = result else {
            panic!("the search must fail")
        };
        (err, steps.into_inner().unwrap())
    }

    /// Page "a" is read and fails, page "b" is not read.
    fn stopped_at_first_page(steps: &[Progress]) -> bool {
        matches!(steps, [
            Progress::Source { name, .. },
            Progress::SourceDone { error: Some(_), .. },
        ] if name == "a")
    }

    #[tokio::test]
    async fn a_rejected_key_stops_the_search_at_the_first_page() {
        let (url, calls) = server_with_wrong_key().await;
        let (err, steps) = search_with_bad_ai(&url, &url).await;
        assert!(err.is::<ProviderUnusable>(), "{err}");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(stopped_at_first_page(&steps), "{steps:?}");
    }

    #[tokio::test]
    async fn no_connection_to_the_ai_stops_the_search_at_the_first_page() {
        let (url, _) = server_with_wrong_key().await;
        // Nothing listens on port 1.
        let (err, steps) = search_with_bad_ai(&url, "http://127.0.0.1:1").await;
        assert!(err.is::<ProviderUnusable>(), "{err}");
        assert!(stopped_at_first_page(&steps), "{steps:?}");
    }

    #[test]
    fn urls_from_the_user_win_and_sources_are_not_loaded() {
        let urls = vec!["https://x.de/jobs".to_string()];
        let targets = targets(&urls, || panic!("sources must not be loaded")).unwrap();
        assert_eq!(targets, [(urls[0].clone(), urls[0].clone())]);
    }

    #[test]
    fn only_enabled_sources_are_read() {
        let targets = targets(&[], || Ok(vec![source("a", true), source("b", false)])).unwrap();
        assert_eq!(targets, [("a".to_string(), "https://a.de".to_string())]);
    }

    #[test]
    fn nothing_to_read_is_an_error() {
        let err = targets(&[], || Ok(vec![source("b", false)])).unwrap_err();
        assert!(
            err.to_string()
                .starts_with("no URLs given and no enabled sources")
        );
        assert!(targets(&[], || Err("invalid sources.yaml".into())).is_err());
    }
}
