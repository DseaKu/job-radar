use serde::Serialize;
use serde_json::Value;

use crate::Res;
use crate::backend::Backend;
use crate::fetch;

/// Below this page budget, the model would see too little of the page to be useful.
const MIN_PAGE_CHARS: usize = 2_000;

/// Upper limit for the AI calls on one page, so that a huge page cannot cause a high cost.
const MAX_CHUNKS: usize = 20;

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
    let text = fetch::page_text(http, url).await?;
    if text.trim().is_empty() {
        return Err(format!(
            "{url}: the page has no visible text (it probably needs JavaScript to render)"
        )
        .into());
    }

    let system = format!(
        "{EXTRACTOR}\n\nWrite \"description\" and \"match_reason\" in {language}. \
         Copy \"title\", \"company\" and \"location\" exactly as they appear on the page."
    );
    let head = format!("CV:\n{cv}\n\nPage URL: {url}\n\nPage text:\n");

    // Cut the page into parts that fit into the context window, and send one AI call for each.
    let budget = backend.page_char_budget(system.len() + head.len());
    if budget < MIN_PAGE_CHARS {
        return Err(format!(
            "{url}: the context window is too small for the CV, the prompt and max_tokens; \
             increase context_length or decrease max_tokens in config.yaml"
        )
        .into());
    }
    let mut chunks = split_chunks(&text, budget, budget / 10);
    if chunks.len() > MAX_CHUNKS {
        eprintln!(
            "  warning: the page needs {} slices, only the first {MAX_CHUNKS} are read; \
             increase context_length in config.yaml to read more",
            chunks.len()
        );
        chunks.truncate(MAX_CHUNKS);
    }
    if chunks.len() > 1 {
        eprintln!(
            "  page too big ({} chars), begin slicing into {} slices; \
             beware: a job at a slice boundary can be lost",
            text.len(),
            chunks.len()
        );
    }

    let mut rows = Vec::new();
    let mut last_error = None;
    let sliced = chunks.len() > 1;
    for (i, chunk) in chunks.iter().enumerate() {
        if sliced {
            eprintln!("    read slice {}/{} ...", i + 1, chunks.len());
        }
        let reply = match backend.chat(&system, &format!("{head}{chunk}")).await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("    warning: slice {}/{} failed: {e}", i + 1, chunks.len());
                last_error = Some(e);
                continue;
            }
        };
        match parse_json_array(&reply) {
            Ok(items) => rows.extend(items.iter().map(|job| to_row(job, url))),
            Err(e) => {
                eprintln!("    warning: slice {}/{} failed: {e}", i + 1, chunks.len());
                last_error = Some(e);
            }
        }
    }
    if rows.is_empty() {
        if let Some(e) = last_error {
            return Err(format!("{url}: {e}").into());
        }
    }
    Ok(merge_overlap(rows))
}

fn to_row(job: &Value, source: &str) -> JobRow {
    JobRow {
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
        source: source.to_string(),
    }
}

/// Splits the text into parts of at most `size` bytes. Each part starts `overlap` bytes before
/// the end of the previous part, so that a job at a boundary is complete in one of the parts.
/// Cuts at line breaks where possible.
fn split_chunks(text: &str, size: usize, overlap: usize) -> Vec<&str> {
    let mut chunks = Vec::new();
    let mut start = 0;
    loop {
        if text.len() - start <= size {
            chunks.push(&text[start..]);
            return chunks;
        }
        let hard_end = floor_boundary(text, start + size);
        let mid = floor_boundary(text, start + size / 2);
        let end = text[mid..hard_end].rfind('\n').map_or(hard_end, |i| mid + i + 1);
        chunks.push(&text[start..end]);

        let back = floor_boundary(text, end - overlap);
        let next = text[back..end].find('\n').map_or(back, |i| back + i + 1);
        // Always move forward, also if the overlap has no line break.
        start = if next > start && next < end { next } else { end };
    }
}

fn floor_boundary(text: &str, mut i: usize) -> usize {
    while !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Removes the copies of a job that the overlap between parts produces. Keeps the higher score.
fn merge_overlap(rows: Vec<JobRow>) -> Vec<JobRow> {
    let key = |r: &JobRow| {
        if r.url.is_empty() {
            format!("{}|{}|{}", r.title, r.company, r.location).to_lowercase()
        } else {
            r.url.clone()
        }
    };
    let mut out: Vec<JobRow> = Vec::new();
    for row in rows {
        match out.iter_mut().find(|r| key(r) == key(&row)) {
            Some(r) if row.match_score > r.match_score => *r = row,
            Some(_) => {}
            None => out.push(row),
        }
    }
    out
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

    #[test]
    fn chunks_cover_text_with_overlap() {
        let text: String = (0..200).map(|i| format!("line {i} äöü\n")).collect();
        let chunks = split_chunks(&text, 500, 50);
        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|c| c.len() <= 500));
        assert!(chunks[0].starts_with("line 0 "));
        assert!(chunks.last().unwrap().ends_with("line 199 äöü\n"));
        // Each part starts with a full line that the previous part also contains.
        for pair in chunks.windows(2) {
            let first_line = pair[1].lines().next().unwrap();
            assert!(pair[0].contains(first_line));
        }
    }

    #[test]
    fn short_text_is_one_chunk() {
        assert_eq!(split_chunks("abc", 500, 50), vec!["abc"]);
    }
}
