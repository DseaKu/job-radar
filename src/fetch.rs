use reqwest::Url;

use crate::Res;

/// Tags whose content is never visible text.
const SKIP_TAGS: &[&str] = &["script", "style", "noscript", "svg", "template", "head"];
/// Tags that start a new line in the text output.
const BLOCK_TAGS: &[&str] = &[
    "p", "div", "br", "li", "ul", "ol", "tr", "td", "th", "h1", "h2", "h3", "h4", "h5", "h6",
    "section", "article", "header", "footer", "table", "hr",
];

/// Downloads a page and returns its visible text. Links appear as `text (absolute-url)`.
pub async fn page_text(http: &reqwest::Client, url: &str) -> Res<String> {
    let base = Url::parse(url).map_err(|e| format!("invalid URL {url}: {e}"))?;
    let response = http.get(base.clone()).send().await?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("GET {url} failed ({status})").into());
    }
    let html = response.text().await?;
    Ok(html_to_text(&html, &base))
}

/// A small HTML-to-text converter. It is not a full parser, but it is enough for an LLM.
fn html_to_text(html: &str, base: &Url) -> String {
    // ASCII lowercasing keeps the byte offsets identical to `html`.
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len() / 4);
    let mut pending_href: Option<String> = None;
    let mut i = 0;

    while i < html.len() {
        if !html[i..].starts_with('<') {
            let next = html[i..].find('<').map_or(html.len(), |n| i + n);
            out.push_str(&decode_entities(&html[i..next]));
            i = next;
            continue;
        }

        if lower[i..].starts_with("<!--") {
            i = lower[i..].find("-->").map_or(html.len(), |n| i + n + 3);
            continue;
        }

        let Some(end) = html[i..].find('>').map(|n| i + n) else {
            break;
        };
        let tag = &lower[i + 1..end];
        let closing = tag.starts_with('/');
        let name: String = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();

        if !closing && SKIP_TAGS.contains(&name.as_str()) && !tag.ends_with('/') {
            // Jump past the matching closing tag.
            let close = format!("</{name}");
            i = match lower[end..].find(&close) {
                Some(n) => lower[end + n..].find('>').map_or(html.len(), |m| end + n + m + 1),
                None => html.len(),
            };
            continue;
        }

        if name == "a" {
            if closing {
                if let Some(href) = pending_href.take() {
                    out.push_str(&format!(" ({href})"));
                }
            } else {
                pending_href = attr(&html[i + 1..end], "href")
                    .filter(|h| !h.starts_with('#') && !h.starts_with("javascript:"))
                    .and_then(|h| base.join(&decode_entities(&h)).ok())
                    .map(|u| u.to_string());
            }
        }

        out.push(if BLOCK_TAGS.contains(&name.as_str()) { '\n' } else { ' ' });
        i = end + 1;
    }

    // Collapse whitespace and drop empty lines.
    out.lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Returns the value of an attribute in a raw tag body (the text between `<` and `>`).
fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let pos = lower.find(&format!("{name}="))? + name.len() + 1;
    let rest = &tag[pos..];
    let value = match rest.chars().next()? {
        q @ ('"' | '\'') => rest[1..].split(q).next()?,
        _ => rest.split(|c: char| c.is_whitespace() || c == '>').next()?,
    };
    Some(value.trim().to_string())
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_text_and_links() {
        let base = Url::parse("https://example.com/careers/").unwrap();
        let html = r#"<html><head><title>x</title></head><body>
            <script>var a = "<p>no</p>";</script>
            <h2>Rust Engineer</h2><p>Berlin &amp; remote</p>
            <a href="/jobs/42">Apply</a><!-- hidden --></body></html>"#;
        let text = html_to_text(html, &base);
        assert_eq!(
            text,
            "Rust Engineer\nBerlin & remote\nApply (https://example.com/jobs/42)"
        );
    }
}
