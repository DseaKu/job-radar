use std::fmt::Write as _;

use serde_json::Value;

use crate::Res;
use crate::backend::Backend;
use crate::jobs::JobRow;

const DEDUPER: &str = r#"You find duplicate job postings in a list. The same job can appear on several job platforms.

Two entries are duplicates if they are the same position at the same company and location.
Company names can differ in form (for example "Brunel GmbH NL Essen" and "Brunel GmbH").
Titles can differ in gender suffixes like "(m/w/d)" or small wording changes.
Different positions at the same company are NOT duplicates.

Reply with only a JSON array of groups. Do not write prose or code fences.
Each group is an array of the entry numbers that are the same job, for example [[0, 4], [2, 7, 9]].
Only list groups with two or more entries. If there are no duplicates, reply with []."#;

/// Asks the LLM which rows are the same job. Returns groups of row indices.
pub async fn find_dups(backend: &Backend, rows: &[JobRow]) -> Res<Vec<Vec<usize>>> {
    if rows.len() < 2 {
        return Ok(Vec::new());
    }

    let mut list = String::new();
    for (i, r) in rows.iter().enumerate() {
        writeln!(
            list,
            "{i}: title={:?} company={:?} location={:?} url={:?}",
            r.title, r.company, r.location, r.url
        )?;
    }

    let reply = backend.chat(DEDUPER, &list).await?;
    parse_groups(&reply, rows.len())
}

/// Keeps one row for each group. The kept row is the one with the highest match score.
/// It gets the sources of all its duplicates.
pub fn merge(rows: Vec<JobRow>, groups: Vec<Vec<usize>>) -> Vec<JobRow> {
    let mut slots: Vec<Option<JobRow>> = rows.into_iter().map(Some).collect();
    let mut out = Vec::new();
    for group in groups {
        let best = *group
            .iter()
            .max_by_key(|&&i| slots[i].as_ref().map_or(-1, |r| r.match_score.unwrap_or(-1)))
            .expect("groups are not empty");
        let mut keep = slots[best].take().expect("each index is used once");
        for &i in &group {
            if let Some(dup) = slots[i].take() {
                if keep.url.is_empty() {
                    keep.url = dup.url;
                }
                if !keep.source.split(" | ").any(|s| s == dup.source) {
                    keep.source = format!("{} | {}", keep.source, dup.source);
                }
            }
        }
        out.push(keep);
    }
    out.extend(slots.into_iter().flatten());
    out
}

/// Reads the groups from the reply. Ignores bad indices and indices that are already in a group.
fn parse_groups(reply: &str, len: usize) -> Res<Vec<Vec<usize>>> {
    let (Some(start), Some(end)) = (reply.find('['), reply.rfind(']')) else {
        return Err(format!("dedup: the model did not return a JSON array: {}", preview(reply)).into());
    };
    let raw: Vec<Value> = serde_json::from_str(&reply[start..=end])
        .map_err(|e| format!("dedup: invalid JSON from the model ({e}): {}", preview(reply)))?;

    let mut used = vec![false; len];
    let mut groups = Vec::new();
    for g in raw {
        let Value::Array(items) = g else { continue };
        let group: Vec<usize> = items
            .iter()
            .filter_map(|v| v.as_u64().map(|n| n as usize))
            .filter(|&i| i < len && !std::mem::replace(&mut used[i], true))
            .collect();
        if group.len() >= 2 {
            groups.push(group);
        } else {
            for i in group {
                used[i] = false;
            }
        }
    }
    Ok(groups)
}

fn preview(s: &str) -> String {
    s.chars().take(300).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_groups_and_drops_bad_indices() {
        let groups = parse_groups("```json\n[[0, 2], [2, 3], [5, 1], [9]]\n```", 4).unwrap();
        assert_eq!(groups, vec![vec![0, 2]]);
    }
}
