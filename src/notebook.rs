//! Notebook container handling. Markdown parsing remains independent of JSON.
use crate::Config;
use serde::Deserialize;
use serde_json::value::RawValue;
use std::{fmt, path::Path};

/// An invalid or unsupported notebook. No partial result is returned.
#[derive(Debug)]
pub struct NotebookError(String);
impl fmt::Display for NotebookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for NotebookError {}

pub fn is_notebook_path(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ipynb"))
}

#[derive(Deserialize)]
struct Notebook<'a> {
    nbformat: u64,
    nbformat_minor: u64,
    #[serde(borrow)]
    cells: Vec<Cell<'a>>,
}
#[derive(Deserialize)]
struct Cell<'a> {
    cell_type: String,
    #[serde(borrow)]
    source: Option<&'a RawValue>,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Source {
    Text(String),
    Lines(Vec<String>),
}

/// Format Markdown cells using the supplied, already resolved configuration.
/// Every byte outside changed `source` values is retained verbatim.
pub fn format_notebook(input: &str, config: &Config) -> Result<String, NotebookError> {
    let notebook: Notebook<'_> =
        serde_json::from_str(input).map_err(|e| NotebookError(format!("invalid notebook: {e}")))?;
    if notebook.nbformat != 4 {
        return Err(NotebookError(format!(
            "unsupported notebook version {}; expected 4",
            notebook.nbformat
        )));
    }
    let _minor = notebook.nbformat_minor;
    let mut edits = Vec::new();
    for (index, cell) in notebook.cells.iter().enumerate() {
        if cell.cell_type != "markdown" {
            continue;
        }
        let invalid = |error: String| {
            NotebookError(format!(
                "invalid Markdown source in cell {}: {error}",
                index + 1
            ))
        };
        let raw = cell
            .source
            .ok_or_else(|| invalid("missing source".into()))?
            .get();
        let source: Source = serde_json::from_str(raw).map_err(|e| invalid(e.to_string()))?;
        let (text, array) = match source {
            Source::Text(s) => (s, false),
            Source::Lines(v) => (v.concat(), true),
        };
        if text.is_empty() {
            continue;
        }
        let formatted = crate::format(&text, Some(config.clone()), None);
        if formatted == text {
            continue;
        }
        let replacement = if array {
            let lines: Vec<_> = formatted.split_inclusive('\n').collect();
            if raw.contains('\n') {
                let eol = if raw.contains("\r\n") { "\r\n" } else { "\n" };
                let indent = raw
                    .lines()
                    .skip(1)
                    .find(|l| l.trim_start().starts_with('"'))
                    .map(|l| &l[..l.len() - l.trim_start().len()])
                    .unwrap_or(" ");
                let closing = raw.lines().next_back().unwrap_or("]");
                let closing_indent = &closing[..closing.len() - closing.trim_start().len()];
                let encoded = lines
                    .iter()
                    .map(|line| {
                        format!(
                            "{indent}{}",
                            serde_json::to_string(line).expect("string serialization")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(&format!(",{eol}"));
                format!("[{eol}{encoded}{eol}{closing_indent}]")
            } else {
                serde_json::to_string(&lines).expect("string serialization")
            }
        } else {
            serde_json::to_string(&formatted).expect("string serialization")
        };
        // RawValue borrows this precise slice from `input`; offsets remain UTF-8 boundaries.
        let start = raw.as_ptr() as usize - input.as_ptr() as usize;
        edits.push((start..start + raw.len(), replacement));
    }
    let mut output = input.to_owned();
    for (range, replacement) in edits.into_iter().rev() {
        output.replace_range(range, &replacement);
    }
    Ok(output)
}
