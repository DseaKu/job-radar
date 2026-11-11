use serde::Serialize;
use serde_json::Value;

use crate::Res;
use crate::backend::Backend;
use crate::fetch;

/// Pages longer than this are cut, so that small local models do not overflow.
const MAX_PAGE_CHARS: usize = 200_000;

const EXTRACTOR: &str = r#"You extract job postings from the text of a web page and rate how well each job fits a candidate's CV.

Reply with only a JSON array. Do not write prose or code fences.
Each element is an object with these keys:
- "title": the job title
- "company": the hiring company, or "" if unknown
- "location": the location and remote policy, or "" if unknown
- "url": the job's own link if the page shows one (links appear as "text (url)"), else ""
- "description": 2 to 4 sentences about the role and its key requirements, taken from the page. Do not invent facts.
- "match_score": an integer from 0 to 100 for how well the CV fits the job
- "match_reason": one sentence that explains the score

If the page contains no job postings, reply with []."#;

/// One CSV row.
#[derive(Serialize)]
pub struct JobRow {
    pub match_score: Option<i64>,
    pub title: String,
    pub company: String,
    pub location: String,
    pub description: String,
    pub match_reason: String,
    pub url: String,
    pub source: String,
}

/// Fetches one page, asks the LLM for the jobs on it, and returns them as rows.
pub async fn from_url(
    backend: &Backend,
    http: &reqwest::Client,
    cv: &str,
    url: &str,
    language: &str,
) -> Res<Vec<JobRow>> {
    let mut text = fetch::page_text(http, url).await?;
    if text.trim().is_empty() {
        return Err(format!(
            "{url}: the page has no visible text (it probably needs JavaScript to render)"
        )
        .into());
    }
    if text.len() > MAX_PAGE_CHARS {
        eprintln!(
            "  warning: page text is {} chars, only the first {MAX_PAGE_CHARS} are used",
            text.len()
        );
        let mut cut = MAX_PAGE_CHARS;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
    }

    let prompt = format!("CV:\n{cv}\n\nPage URL: {url}\n\nPage text:\n{text}");
    let system = format!(
        "{EXTRACTOR}\n\nWrite \"description\" and \"match_reason\" in {language}. \
         Copy \"title\", \"company\" and \"location\" exactly as they appear on the page."
    );
    let reply = backend.chat(&system, &prompt).await?;
    let items = parse_json_array(&reply).map_err(|e| format!("{url}: {e}"))?;

    Ok(items
        .iter()
        .map(|job| JobRow {
            match_score: job["match_score"]
                .as_i64()
                .or_else(|| job["match_score"].as_f64().map(|f| f.round() as i64))
                .or_else(|| job["match_score"].as_str().and_then(|s| s.trim().parse().ok())),
            title: field(job, "title"),
            company: field(job, "company"),
            location: field(job, "location"),
            description: field(job, "description"),
            match_reason: field(job, "match_reason"),
            url: field(job, "url"),
            source: url.to_string(),
        })
        .collect())
}

/// Writes the rows to a CSV file, best match first.
pub fn write_csv(path: &str, mut rows: Vec<JobRow>) -> Res<()> {
    rows.sort_by_key(|r| std::cmp::Reverse(r.match_score.unwrap_or(-1)));
    let mut writer = csv::Writer::from_path(path).map_err(|e| format!("cannot write {path}: {e}"))?;
    for row in rows {
        writer.serialize(row)?;
    }
    writer.flush()?;
    Ok(())
}

fn field(job: &Value, key: &str) -> String {
    match &job[key] {
        Value::String(s) => s.trim().to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Finds the JSON array in the reply, also if the model wrapped it in prose or code fences.
fn parse_json_array(reply: &str) -> Res<Vec<Value>> {
    let (Some(start), Some(end)) = (reply.find('['), reply.rfind(']')) else {
        return Err(format!("the model did not return a JSON array: {}", preview(reply)).into());
    };
    let items: Vec<Value> = serde_json::from_str(&reply[start..=end])
        .map_err(|e| format!("invalid JSON from the model ({e}): {}", preview(reply)))?;
    Ok(items.into_iter().filter(Value::is_object).collect())
}

fn preview(s: &str) -> String {
    s.chars().take(300).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fenced_array() {
        let reply = "```json\n[{\"title\": \"Dev\", \"match_score\": \"80\"}]\n```";
        let items = parse_json_array(reply).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(field(&items[0], "title"), "Dev");
    }
}
