use std::{env, fs, path::Path};

use serde::Deserialize;

use crate::Res;
use crate::jobs::JobRow;

const DEFAULT_IGNORE_PATH: &str = "ignore.yaml";

/// The ignore list, loaded from `ignore.yaml` (or the file named by `JOB_RADAR_IGNORE`).
/// Jobs that match an entry are not written to the CSV, for example jobs you already applied to.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IgnoreFile {
    #[serde(default)]
    ignore: Vec<Rule>,
}

/// One entry. A job matches if it matches all the fields that the entry gives.
/// - `url`: the same job link (case, a trailing `/` and a `#fragment` do not count).
/// - `company`, `title`: the text is in the job's company or title (case does not count).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    url: Option<String>,
    company: Option<String>,
    title: Option<String>,
    /// Free text for you (for example "applied 2026-11-01"). The tool does not use it.
    #[allow(dead_code)]
    note: Option<String>,
}

impl Rule {
    fn matches(&self, row: &JobRow) -> bool {
        let contains =
            |hay: &str, needle: &str| hay.to_lowercase().contains(&needle.trim().to_lowercase());
        self.url
            .as_ref()
            .is_none_or(|u| !row.url.is_empty() && norm_url(u) == norm_url(&row.url))
            && self
                .company
                .as_ref()
                .is_none_or(|c| contains(&row.company, c))
            && self.title.as_ref().is_none_or(|t| contains(&row.title, t))
    }

    fn is_empty(&self) -> bool {
        [&self.url, &self.company, &self.title]
            .iter()
            .all(|f| f.as_deref().is_none_or(|s| s.trim().is_empty()))
    }
}

fn norm_url(url: &str) -> String {
    let url = url.trim();
    let url = url.split_once('#').map_or(url, |(u, _)| u);
    url.trim_end_matches('/').to_lowercase()
}

/// Loads the ignore list. A missing default file gives an empty list.
pub fn load() -> Res<Vec<Rule>> {
    let (path, explicit) = match env::var("JOB_RADAR_IGNORE") {
        Ok(p) if !p.trim().is_empty() => (p, true),
        _ => (DEFAULT_IGNORE_PATH.to_string(), false),
    };
    if !explicit && !Path::new(&path).exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path).map_err(|e| format!("cannot read {path}: {e}"))?;
    parse(&text).map_err(|e| format!("invalid {path}: {e}").into())
}

fn parse(text: &str) -> Res<Vec<Rule>> {
    // A file with only comments is valid: it is an empty list.
    if text
        .lines()
        .all(|l| l.trim().is_empty() || l.trim_start().starts_with('#'))
    {
        return Ok(Vec::new());
    }
    let file: IgnoreFile = serde_yaml::from_str(text)?;
    if let Some(i) = file.ignore.iter().position(Rule::is_empty) {
        return Err(format!("entry {} needs at least one of url, company, title", i + 1).into());
    }
    Ok(file.ignore)
}

/// Removes the rows that match a rule. Returns the number of removed rows.
pub fn apply(rows: &mut Vec<JobRow>, rules: &[Rule]) -> usize {
    let before = rows.len();
    rows.retain(|row| !rules.iter().any(|r| r.matches(row)));
    before - rows.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(title: &str, company: &str, url: &str) -> JobRow {
        JobRow {
            match_score: None,
            title: title.into(),
            company: company.into(),
            location: String::new(),
            description: String::new(),
            match_reason: String::new(),
            url: url.into(),
            source: String::new(),
        }
    }

    #[test]
    fn removes_matching_rows() {
        let rules = parse(
            "ignore:\n\
             - url: https://a.de/job/1/\n\
             - company: ahe gmbh\n  title: Junior Softwareentwickler\n  note: applied\n\
             - company: Brunel\n",
        )
        .unwrap();
        let mut rows = vec![
            row("Dev", "X", "https://A.de/job/1#top"),
            row("Junior Softwareentwickler\\*in (m/w/d)", "AHE GmbH", ""),
            row("Senior Dev", "AHE GmbH", ""),
            row("Embedded", "Brunel GmbH NL Essen", ""),
            row("Dev", "Y", "https://a.de/job/2"),
        ];
        assert_eq!(apply(&mut rows, &rules), 3);
        let left: Vec<_> = rows.iter().map(|r| r.company.as_str()).collect();
        assert_eq!(left, ["AHE GmbH", "Y"]);
    }

    #[test]
    fn example_file_parses() {
        let text = fs::read_to_string("ignore.example.yaml").unwrap();
        assert!(!parse(&text).unwrap().is_empty());
    }

    #[test]
    fn rejects_empty_and_unknown_fields() {
        assert!(parse("ignore:\n  - note: x\n").is_err());
        assert!(parse("ignore:\n  - titel: x\n").is_err());
        assert!(parse("").unwrap().is_empty());
        assert!(parse("# only a comment\n").unwrap().is_empty());
    }
}
