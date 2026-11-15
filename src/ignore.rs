use std::{env, fs, path::Path};

use serde::{Deserialize, Serialize};

use crate::Res;
use crate::jobs::JobRow;

const DEFAULT_IGNORE_PATH: &str = "ignore.yaml";

/// The ignore list, loaded from `ignore.yaml` (or the file named by `JOB_RADAR_IGNORE`).
/// Jobs that match an entry are not written to the CSV, for example jobs you already applied to.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IgnoreFile {
    #[serde(default)]
    ignore: Vec<Rule>,
}

/// One entry. A job matches if it matches all the fields that the entry gives.
/// - `url`: the same job link (case, a trailing `/` and a `#fragment` do not count).
/// - `company`, `title`: the text is in the job's company or title (case does not count).
///   It must start at the start of a word: `its` matches "ITS GmbH", but not "Digits GmbH";
///   `polizei` matches "Polizeipräsidium".
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub company: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Free text for you (for example "applied 2026-11-01"). The tool does not use it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Rule {
    fn matches(&self, row: &JobRow) -> bool {
        let contains = |hay: &str, needle: &str| contains_at_word_start(hay, needle);
        self.url
            .as_ref()
            .is_none_or(|u| !row.url.is_empty() && norm_url(u) == norm_url(&row.url))
            && self
                .company
                .as_ref()
                .is_none_or(|c| contains(&row.company, c))
            && self.title.as_ref().is_none_or(|t| contains(&row.title, t))
    }

    /// True if the rule has no url, company or title, so that it can never match.
    pub fn is_empty(&self) -> bool {
        [&self.url, &self.company, &self.title]
            .iter()
            .all(|f| f.as_deref().is_none_or(|s| s.trim().is_empty()))
    }
}

/// True if `needle` is in `hay` (case does not count) and starts at the start of a word.
fn contains_at_word_start(hay: &str, needle: &str) -> bool {
    let hay = hay.to_lowercase();
    let needle = needle.trim().to_lowercase();
    if needle.is_empty() {
        return false;
    }
    hay.match_indices(&needle).any(|(i, _)| {
        !hay[..i]
            .chars()
            .next_back()
            .is_some_and(char::is_alphanumeric)
    })
}

fn norm_url(url: &str) -> String {
    let url = url.trim();
    let url = url.split_once('#').map_or(url, |(u, _)| u);
    url.trim_end_matches('/').to_lowercase()
}

fn path_and_explicit() -> (String, bool) {
    match env::var("JOB_RADAR_IGNORE") {
        Ok(p) if !p.trim().is_empty() => (p, true),
        _ => (DEFAULT_IGNORE_PATH.to_string(), false),
    }
}

/// The path of the ignore list.
pub fn path() -> String {
    path_and_explicit().0
}

/// Writes the ignore list. Entries without url, company and title are left out.
/// The comments of the old file are lost.
pub fn save(rules: &[Rule]) -> Res<()> {
    let file = IgnoreFile {
        ignore: rules.iter().filter(|r| !r.is_empty()).cloned().collect(),
    };
    crate::config::write_yaml(
        &path(),
        "# Jobs to leave out of the results. Written by the Job Radar app; comments are not kept.\n\
         # All fields are explained in ignore.example.yaml.\n",
        &file,
    )
}

/// Loads the ignore list. A missing default file gives an empty list.
pub fn load() -> Res<Vec<Rule>> {
    let (path, explicit) = path_and_explicit();
    if !explicit && !Path::new(&path).exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path).map_err(|e| format!("cannot read {path}: {e}"))?;
    parse(&text).map_err(|e| format!("invalid {path}: {e}").into())
}

pub(crate) fn parse(text: &str) -> Res<Vec<Rule>> {
    // A file with only comments is valid: it is an empty list.
    if text
        .lines()
        .all(|l| l.trim().is_empty() || l.trim_start().starts_with('#'))
    {
        return Ok(Vec::new());
    }
    // Simple format: one company name on each line (a leading "- " is also accepted).
    if !matches!(
        serde_yaml::from_str(text),
        Ok(serde_yaml::Value::Mapping(_))
    ) {
        return Ok(text
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| Rule {
                url: None,
                company: Some(l.trim_start_matches("- ").trim().to_string()),
                title: None,
                note: None,
            })
            .collect());
    }
    let file: IgnoreFile = serde_yaml::from_str(text)?;
    if let Some(i) = file.ignore.iter().position(Rule::is_empty) {
        return Err(format!("entry {} needs at least one of url, company, title", i + 1).into());
    }
    Ok(file.ignore)
}

/// True if a rule matches this row.
pub fn is_ignored(row: &JobRow, rules: &[Rule]) -> bool {
    rules.iter().any(|r| r.matches(row))
}

/// Removes the rows that match a rule. Returns the number of removed rows.
pub fn apply(rows: &mut Vec<JobRow>, rules: &[Rule]) -> usize {
    let before = rows.len();
    rows.retain(|row| !is_ignored(row, rules));
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
    fn simple_format_is_company_names() {
        let rules = parse("ahe\n# comment\n\n- its\nbrunel\npolizei\n").unwrap();
        assert_eq!(rules.len(), 4);
        let mut rows = vec![
            row("Dev", "AHE GmbH", ""),
            row("Dev", "ITS Ingenieurbüro", ""),
            row("Dev", "Schwarz Digits", ""),
            row("Dev", "Polizeipräsidium Essen", ""),
            row("Dev", "Brunel GmbH NL Essen", ""),
            row("Its Dev", "Other", ""),
        ];
        assert_eq!(apply(&mut rows, &rules), 4);
        let left: Vec<_> = rows.iter().map(|r| r.company.as_str()).collect();
        assert_eq!(left, ["Schwarz Digits", "Other"]);
    }

    #[test]
    fn saved_rules_parse_again() {
        let rules = vec![
            Rule {
                company: Some("AHE".into()),
                note: Some("applied".into()),
                ..Rule::default()
            },
            Rule {
                url: Some("https://a.de/job/1".into()),
                ..Rule::default()
            },
        ];
        let text = serde_yaml::to_string(&IgnoreFile {
            ignore: rules.clone(),
        })
        .unwrap();
        assert_eq!(parse(&text).unwrap(), rules);
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
