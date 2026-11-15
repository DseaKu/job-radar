//! One complete search: read each page, find and rate the jobs, apply the ignore list and
//! (optionally) remove duplicates. The CLI and the desktop app call [`run`].

use serde::Serialize;

use crate::Res;
use crate::backend::{Backend, Usage};
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
        return Err("no URLs given and no enabled sources in sources.yaml \
                    (create it with: cp sources.example.yaml sources.yaml)"
            .into());
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
