use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Res;
use crate::backend::{Backend, ReplyCutOff};

/// Below this page budget, the model would see too little of the page to be useful.
const MIN_PAGE_CHARS: usize = 2_000;

/// Upper limit for the AI calls on one page, so that a huge page cannot cause a high cost.
const MAX_CHUNKS: usize = 20;

/// How often a slice can be split again after a cut-off reply (1 → 1.1 → 1.1.1 ...).
const MAX_SPLITS: usize = 3;

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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Asks the LLM for the jobs in the text of one page (from `fetch::page_text`) and returns them as rows.
pub async fn from_text(
    backend: &Backend,
    cv: &str,
    url: &str,
    text: &str,
    language: &str,
    extra_instructions: &str,
) -> Res<Vec<JobRow>> {
    if text.trim().is_empty() {
        return Err(format!(
            "{url}: the page has no visible text (it probably needs JavaScript to render)"
        )
        .into());
    }

    let system = system_prompt(language, extra_instructions);
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
    let mut chunks = split_chunks(text, budget, budget / 10);
    if chunks.len() > MAX_CHUNKS {
        warn!(
            "  warning: the page needs {} slices, only the first {MAX_CHUNKS} are read; \
             increase context_length in config.yaml to read more",
            chunks.len()
        );
        chunks.truncate(MAX_CHUNKS);
    }
    if chunks.len() > 1 {
        info!(
            "  page too big ({} chars), begin slicing into {} slices; \
             beware: a job at a slice boundary can be lost",
            text.len(),
            chunks.len()
        );
    }

    // Work list of (label, text). A slice whose reply is cut off is split in half and put back.
    let sliced = chunks.len() > 1;
    let mut queue: VecDeque<(String, &str)> = chunks
        .iter()
        .enumerate()
        .map(|(i, c)| ((i + 1).to_string(), *c))
        .collect();
    let mut rows = Vec::new();
    let mut last_error = None;
    while let Some((label, chunk)) = queue.pop_front() {
        if sliced || label.contains('.') {
            info!("    read slice {label} ...");
        }
        let reply = match backend.chat(&system, &format!("{head}{chunk}")).await {
            Ok(r) => r,
            Err(e) => match e.downcast::<ReplyCutOff>() {
                Ok(cut) => {
                    let saved = parse_partial_array(&cut.partial);
                    // Complete jobs before the cut mean "too many jobs": a smaller slice helps.
                    // No complete job means the model did not write the list (for example it
                    // thought or looped): a smaller slice does not help, so do not split.
                    let can_split = chunk.len() >= 2 * MIN_PAGE_CHARS
                        && label.matches('.').count() < MAX_SPLITS;
                    if !saved.is_empty() && can_split {
                        let halves = split_chunks(chunk, chunk.len() * 55 / 100, chunk.len() / 20);
                        let names: Vec<String> =
                            (1..=halves.len()).map(|n| format!("{label}.{n}")).collect();
                        info!(
                            "    reply too long ({} jobs before the cut), split slice {label} into {}",
                            saved.len(),
                            names.join(", ")
                        );
                        for (name, half) in names.into_iter().zip(halves).rev() {
                            queue.push_front((name, half));
                        }
                        continue;
                    }
                    if saved.is_empty() {
                        warn!(
                            "    warning: slice {label}: the reply has no complete job before the cut \
                             at max_tokens ({}). The model probably thinks or repeats itself.\n    \
                             reply starts with: {}",
                            cut.max_tokens,
                            preview(&cut.partial).replace('\n', " ")
                        );
                        last_error = Some(cut as Box<dyn std::error::Error + Send + Sync>);
                    } else {
                        warn!(
                            "    warning: slice {label}: {cut}; kept {} complete jobs from the reply",
                            saved.len()
                        );
                    }
                    rows.extend(saved.iter().map(|job| to_row(job, url)));
                    continue;
                }
                Err(e) => {
                    if sliced {
                        warn!("    warning: slice {label} failed: {e}");
                    }
                    last_error = Some(e);
                    continue;
                }
            },
        };
        match parse_json_array(&reply) {
            Ok(items) => rows.extend(items.iter().map(|job| to_row(job, url))),
            Err(e) => {
                if sliced {
                    warn!("    warning: slice {label} failed: {e}");
                }
                last_error = Some(e);
            }
        }
    }
    if rows.is_empty()
        && let Some(e) = last_error
    {
        return Err(format!("{url}: {e}").into());
    }
    Ok(merge_overlap(rows))
}

/// The system prompt: the fixed rules, the output language and the user's extra instructions.
/// The extra instructions come last, but the JSON format rules stay binding.
fn system_prompt(language: &str, extra_instructions: &str) -> String {
    let mut system = format!(
        "{EXTRACTOR}\n\nWrite \"description\" and \"match_reason\" in {language}. \
         Copy \"title\", \"company\" and \"location\" exactly as they appear on the page."
    );
    let extra = extra_instructions.trim();
    if !extra.is_empty() {
        system += &format!(
            "\n\nExtra instructions from the user. Follow them when you select and rate the jobs, \
             but always keep the JSON format above:\n{extra}"
        );
    }
    system
}

fn to_row(job: &Value, source: &str) -> JobRow {
    let (title, link) = split_link(&field(job, "title"));
    let url = field(job, "url");
    JobRow {
        match_score: job["match_score"]
            .as_i64()
            .or_else(|| job["match_score"].as_f64().map(|f| f.round() as i64))
            .or_else(|| {
                job["match_score"]
                    .as_str()
                    .and_then(|s| s.trim().parse().ok())
            }),
        title,
        company: field(job, "company"),
        location: field(job, "location"),
        description: field(job, "description"),
        match_reason: field(job, "match_reason"),
        url: if url.is_empty() { link } else { url },
        source: source.to_string(),
    }
}

/// Page links appear as "text (url)", and small models often copy the link into the title.
/// Returns the title without a link at its end, and that link (or "").
fn split_link(title: &str) -> (String, String) {
    if let Some(start) = title.rfind(" (")
        && title.ends_with(')')
    {
        let link = &title[start + 2..title.len() - 1];
        if link.starts_with("http://") || link.starts_with("https://") {
            return (title[..start].trim_end().to_string(), link.to_string());
        }
    }
    (title.to_string(), String::new())
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
        let end = text[mid..hard_end]
            .rfind('\n')
            .map_or(hard_end, |i| mid + i + 1);
        chunks.push(&text[start..end]);

        let back = floor_boundary(text, end - overlap);
        let next = text[back..end].find('\n').map_or(back, |i| back + i + 1);
        // Always move forward, also if the overlap has no line break.
        start = if next > start && next < end {
            next
        } else {
            end
        };
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

/// Sorts the rows: best match first, rows without a score last.
pub fn sort(rows: &mut [JobRow]) {
    rows.sort_by_key(|r| std::cmp::Reverse(r.match_score.unwrap_or(-1)));
}

/// Sorts the rows (best match first) and writes them to a CSV file.
pub fn write_csv(path: &str, rows: &mut [JobRow]) -> Res<()> {
    sort(rows);
    let mut writer =
        csv::Writer::from_path(path).map_err(|e| format!("cannot write {path}: {e}"))?;
    for row in rows.iter() {
        writer.serialize(row)?;
    }
    writer.flush()?;
    Ok(())
}

/// Reads a CSV file from [`write_csv`].
pub fn read_csv(path: &str) -> Res<Vec<JobRow>> {
    let mut reader =
        csv::Reader::from_path(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    reader
        .deserialize()
        .collect::<Result<Vec<JobRow>, _>>()
        .map_err(|e| format!("{path} is not a job_radar CSV file: {e}").into())
}

/// A short text table of the first `max` rows: score, title, company and location.
/// Long values are cut, so that a line fits into a normal terminal.
pub fn table(rows: &[JobRow], max: usize) -> String {
    fn cut(s: &str, max: usize) -> String {
        if s.chars().count() <= max {
            s.to_string()
        } else {
            format!("{}…", s.chars().take(max - 1).collect::<String>())
        }
    }
    let shown = &rows[..rows.len().min(max)];
    let cells: Vec<[String; 3]> = shown
        .iter()
        .map(|r| [cut(&r.title, 42), cut(&r.company, 26), cut(&r.location, 32)])
        .collect();
    // The title and company columns are as wide as their longest value.
    let width = |i: usize, head: &str| {
        cells
            .iter()
            .map(|c| c[i].chars().count())
            .fold(head.len(), usize::max)
    };
    let (wt, wc) = (width(0, "title"), width(1, "company"));

    let mut out = format!("score  {:<wt$}  {:<wc$}  location\n", "title", "company");
    for (r, [title, company, location]) in shown.iter().zip(&cells) {
        let score = r.match_score.map_or("-".to_string(), |s| s.to_string());
        out += &format!("{score:>5}  {title:<wt$}  {company:<wc$}  {location}\n");
    }
    if rows.len() > shown.len() {
        out += &format!("  ... {} more in the CSV file\n", rows.len() - shown.len());
    }
    out
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
    let json = &reply[start..=end];
    // Models often copy a backslash from the page (for example `Entwickler\*in`), which is not a
    // valid JSON escape. If the first parse fails, escape such backslashes and try again.
    let items: Vec<Value> = serde_json::from_str(json)
        .or_else(|e| serde_json::from_str(&fix_escapes(json)).map_err(|_| e))
        .map_err(|e| format!("invalid JSON from the model ({e}): {}", preview(reply)))?;
    Ok(items.into_iter().filter(Value::is_object).collect())
}

/// Reads the complete objects from a reply that was cut off inside the JSON array.
fn parse_partial_array(reply: &str) -> Vec<Value> {
    let Some(start) = reply.find('[') else {
        return Vec::new();
    };
    // Try each `}` from the end: the text up to it plus `]` is valid if that `}` closes an object.
    for (end, _) in reply.match_indices('}').rev().take(50) {
        if end < start {
            break;
        }
        let candidate = format!("{}]", &reply[start..=end]);
        if let Ok(items) = parse_json_array(&candidate) {
            return items;
        }
    }
    Vec::new()
}

/// Doubles each backslash that does not start a valid JSON escape.
fn fix_escapes(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    let mut chars = json.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '\\' {
            match chars.peek() {
                Some('"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't' | 'u') => {
                    out.push(chars.next().unwrap());
                }
                _ => out.push('\\'),
            }
        }
    }
    out
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
    fn fixes_invalid_escapes() {
        let reply = r#"[{"title": "Entwickler\*in", "description": "a \"b\" c\nd"}]"#;
        let items = parse_json_array(reply).unwrap();
        assert_eq!(field(&items[0], "title"), r"Entwickler\*in");
        assert_eq!(field(&items[0], "description"), "a \"b\" c\nd");
    }

    #[test]
    fn keeps_complete_jobs_from_cut_reply() {
        let reply = r#"[{"title": "A {x}"}, {"title": "B"}, {"title": "C", "descr"#;
        let items = parse_partial_array(reply);
        assert_eq!(items.len(), 2);
        assert_eq!(field(&items[1], "title"), "B");
    }

    #[test]
    fn removes_link_from_title() {
        let job: Value = serde_json::from_str(
            r#"{"title": "Rust Dev (m/w/d) (https://a.de/jobs/1)", "url": ""}"#,
        )
        .unwrap();
        let row = to_row(&job, "src");
        assert_eq!(row.title, "Rust Dev (m/w/d)");
        assert_eq!(row.url, "https://a.de/jobs/1");
        assert_eq!(split_link("Dev (m/w/d)").0, "Dev (m/w/d)");
    }

    #[test]
    fn table_aligns_and_cuts() {
        let row = |score, title: &str, company: &str| JobRow {
            match_score: Some(score),
            title: title.into(),
            company: company.into(),
            location: "Bochum · Hybrid".into(),
            description: String::new(),
            match_reason: String::new(),
            url: String::new(),
            source: String::new(),
        };
        let rows = vec![
            row(90, "Rust Developer", "Nordlicht Robotics"),
            row(40, &"Very long title ".repeat(5), "Ä GmbH"),
            row(10, "Third", "X"),
        ];
        let text = table(&rows, 2);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 4);
        assert!(lines[0].starts_with("score  title"));
        assert!(lines[1].starts_with("   90  Rust Developer"));
        assert!(lines[2].contains('…'));
        // The company column starts at the same character position in each line.
        let col = |l: &str, word: &str| l[..l.find(word).unwrap()].chars().count();
        assert_eq!(col(lines[0], "company"), col(lines[1], "Nordlicht"));
        assert_eq!(col(lines[0], "company"), col(lines[2], "Ä GmbH"));
        assert_eq!(lines[3], "  ... 1 more in the CSV file");
    }

    #[test]
    fn csv_file_reads_back() {
        let row = |score: Option<i64>, title: &str| JobRow {
            match_score: score,
            title: title.into(),
            company: "Nordlicht, \"Robotics\"".into(),
            location: "Bochum".into(),
            description: "Line 1\nLine 2".into(),
            match_reason: String::new(),
            url: "https://a.de/1".into(),
            source: "https://a.de".into(),
        };
        let path = std::env::temp_dir().join("job_radar_test_read_csv.csv");
        let path = path.to_str().unwrap();
        write_csv(path, &mut [row(None, "No score"), row(Some(80), "Dev")]).unwrap();
        let rows = read_csv(path).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].match_score, Some(80));
        assert_eq!(rows[0].company, "Nordlicht, \"Robotics\"");
        assert_eq!(rows[0].description, "Line 1\nLine 2");
        assert_eq!(rows[1].match_score, None);

        std::fs::write(path, "a,b\n1,2\n").unwrap();
        assert!(
            read_csv(path)
                .unwrap_err()
                .to_string()
                .contains("not a job_radar CSV")
        );
    }

    #[test]
    fn extra_instructions_go_at_the_end_of_the_prompt() {
        let plain = system_prompt("German", "  \n");
        assert!(plain.ends_with("exactly as they appear on the page."));
        assert!(!plain.contains("Extra instructions"));

        let extra = system_prompt("English", "Rate remote jobs 10 points higher.\n");
        assert!(extra.contains("in English."));
        assert!(extra.ends_with("keep the JSON format above:\nRate remote jobs 10 points higher."));
    }

    #[test]
    fn short_text_is_one_chunk() {
        assert_eq!(split_chunks("abc", 500, 50), vec!["abc"]);
    }
}
