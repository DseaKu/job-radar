use std::fs;
use std::path::Path;

use crate::Res;

/// `job_radar convert-cv <cv.pdf> [<cv.txt>]`: converts a PDF CV to a text file once.
pub fn convert_command(args: &[String]) -> Res<()> {
    let (pdf, out) = match args {
        [pdf] => (pdf.clone(), Path::new(pdf).with_extension("txt")),
        [pdf, out] => (pdf.clone(), out.into()),
        _ => return Err("usage: job_radar convert-cv <cv.pdf> [<cv.txt>]".into()),
    };

    let text = pdf_to_text(&pdf)?;
    fs::write(&out, &text).map_err(|e| format!("cannot write {}: {e}", out.display()))?;
    eprintln!(
        "Wrote {} ({} words). Check the text, then use: --cv {}",
        out.display(),
        text.split_whitespace().count(),
        out.display()
    );
    Ok(())
}

/// Reads a CV file. A `.pdf` file is converted to text, any other file is read as text.
pub fn read(path: &str) -> Res<String> {
    if path.to_ascii_lowercase().ends_with(".pdf") {
        pdf_to_text(path)
    } else {
        fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}").into())
    }
}

fn pdf_to_text(path: &str) -> Res<String> {
    let raw =
        pdf_extract::extract_text(path).map_err(|e| format!("cannot read PDF {path}: {e}"))?;
    let text = clean(&raw);
    if text.split_whitespace().count() < 20 {
        return Err(format!(
            "{path} contains almost no text. It is probably a scanned image; \
             export it again as a text PDF or copy the text by hand."
        )
        .into());
    }
    Ok(text)
}

/// Trims each line and keeps at most one empty line in a row.
fn clean(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut blank = 0;
    for line in raw.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            blank += 1;
            if blank > 1 || out.is_empty() {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(&line);
        out.push('\n');
    }
    out.trim_end().to_string() + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_whitespace() {
        assert_eq!(
            clean("\n\n  Jane   Doe \n\n\n\nRust  dev\n"),
            "Jane Doe\n\nRust dev\n"
        );
    }
}
