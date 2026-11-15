//! `--dump <dir>`: writes what the program saw and found on each website, for checks and
//! debugging. Two files for each website:
//! - `<name>.page.txt`: the page text that the AI got
//! - `<name>.jobs.txt`: all jobs found on the page, before the ignore list and the duplicate check

use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use crate::Res;
use crate::ignore::{self, Rule};
use crate::jobs::JobRow;

pub struct Dump {
    dir: PathBuf,
    used: HashSet<String>,
}

impl Dump {
    /// Creates the folder if it does not exist.
    pub fn new(dir: &str) -> Res<Self> {
        fs::create_dir_all(dir).map_err(|e| format!("cannot create the dump folder {dir}: {e}"))?;
        Ok(Self {
            dir: PathBuf::from(dir),
            used: HashSet::new(),
        })
    }

    /// A file name stem for this website. A second website with the same stem gets "-2", and so on.
    pub fn stem(&mut self, name: &str) -> String {
        let base = slug(name);
        let mut stem = base.clone();
        let mut n = 2;
        while !self.used.insert(stem.clone()) {
            stem = format!("{base}-{n}");
            n += 1;
        }
        stem
    }

    pub fn write_page(&self, stem: &str, text: &str) -> Res<()> {
        self.write(&format!("{stem}.page.txt"), text)
    }

    pub fn write_jobs(
        &self,
        stem: &str,
        name: &str,
        url: &str,
        result: &Res<Vec<JobRow>>,
        rules: &[Rule],
    ) -> Res<()> {
        self.write(
            &format!("{stem}.jobs.txt"),
            &jobs_text(name, url, result, rules),
        )
    }

    fn write(&self, file: &str, text: &str) -> Res<()> {
        let path = self.dir.join(file);
        fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()).into())
    }
}

/// "Nordlicht Robotics (careers page)" -> "nordlicht-robotics-careers-page".
/// A URL as the name (URLs from the command line) becomes its host and path.
pub(crate) fn slug(name: &str) -> String {
    let name = name
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.");
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out: String = out.trim_end_matches('-').chars().take(60).collect();
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "website".to_string()
    } else {
        out
    }
}

/// The readable list of all jobs found on one website, best match first.
fn jobs_text(name: &str, url: &str, result: &Res<Vec<JobRow>>, rules: &[Rule]) -> String {
    let mut out = format!("Source: {name}\nURL:    {url}\n\n");
    let rows = match result {
        Ok(rows) => rows,
        Err(e) => {
            out += &format!("Error: {e}\n");
            return out;
        }
    };
    let ignored = rows.iter().filter(|r| ignore::is_ignored(r, rules)).count();
    let _ = writeln!(
        out,
        "{} jobs found, {ignored} of them match the ignore list.\n\
         This is the result before the ignore list and the duplicate check.\n",
        rows.len()
    );

    let mut sorted: Vec<&JobRow> = rows.iter().collect();
    sorted.sort_by_key(|r| std::cmp::Reverse(r.match_score.unwrap_or(-1)));
    for r in sorted {
        let score = r.match_score.map_or("-".to_string(), |s| s.to_string());
        let mark = if ignore::is_ignored(r, rules) {
            "  [ignored]"
        } else {
            ""
        };
        let _ = writeln!(out, "[{score}] {}{mark}", r.title);
        let place: Vec<&str> = [r.company.as_str(), r.location.as_str()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect();
        if !place.is_empty() {
            let _ = writeln!(out, "     {}", place.join(" · "));
        }
        for (label, value) in [
            ("Link", &r.url),
            ("About", &r.description),
            ("Why", &r.match_reason),
        ] {
            if !value.is_empty() {
                let _ = writeln!(out, "     {label}: {value}");
            }
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(score: i64, title: &str, company: &str) -> JobRow {
        JobRow {
            match_score: Some(score),
            title: title.into(),
            company: company.into(),
            location: "Bochum".into(),
            description: String::new(),
            match_reason: "fits".into(),
            url: String::new(),
            source: String::new(),
        }
    }

    #[test]
    fn slugs_and_unique_stems() {
        assert_eq!(
            slug("Nordlicht Robotics (careers page)"),
            "nordlicht-robotics-careers-page"
        );
        assert_eq!(
            slug("https://www.stepstone.de/jobs/it?r=25"),
            "stepstone-de-jobs-it-r-25"
        );
        assert_eq!(slug("Ärzte & Co"), "ärzte-co");
        assert_eq!(slug("***"), "website");

        let dir = std::env::temp_dir().join("job_radar_dump_test_stems");
        let mut dump = Dump::new(dir.to_str().unwrap()).unwrap();
        assert_eq!(dump.stem("A"), "a");
        assert_eq!(dump.stem("a"), "a-2");
        assert_eq!(dump.stem("A!"), "a-3");
    }

    #[test]
    fn jobs_text_sorts_and_marks_ignored() {
        let rules = ignore::parse("bechtle\n").unwrap();
        let result: Res<Vec<JobRow>> = Ok(vec![
            row(40, "Admin", "Bechtle GmbH"),
            row(90, "Rust Dev", "Nordlicht"),
        ]);
        let text = jobs_text("Board", "https://b.de", &result, &rules);
        assert!(text.contains("2 jobs found, 1 of them match the ignore list."));
        let rust = text.find("[90] Rust Dev").unwrap();
        let admin = text.find("[40] Admin  [ignored]").unwrap();
        assert!(rust < admin, "best match first");
        assert!(text.contains("     Nordlicht · Bochum\n     Why: fits\n"));

        let failed: Res<Vec<JobRow>> = Err("timeout".into());
        assert!(jobs_text("Board", "https://b.de", &failed, &rules).ends_with("Error: timeout\n"));
    }
}
