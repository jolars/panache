//! Shared 2D geometry pass for grid tables.
//!
//! A grid table's logical cells are rectangles over a canonical column/row
//! grid, and a spanning cell's content is *non-contiguous* in the byte stream
//! (a rowspan cell's text is interleaved with other cells' bytes and separator
//! lines). A rowan CST node covers a single contiguous range, so the cell
//! tiling cannot be represented as CST nodes and must be recovered by a 2D pass
//! downstream of the parser. This module is the single home for that pass,
//! consumed by both the pandoc-native projector (`pandoc_ast::grid_table`) and
//! the formatter's spanning-grid engine, so the geometry is computed one way.
//!
//! The algorithm mirrors pandoc's `gridtables`: build a padded char grid,
//! take the canonical column boundaries as the union of `+` positions across
//! every "sep-style" line and the canonical row boundaries as those lines'
//! indices, then detect each cell as the smallest valid bounding rectangle.
//! Positions are display columns. Cell contents are sliced from the original
//! source so wide characters and combining marks survive geometry analysis.

use crate::syntax::tables::column_slice_with_width;
use std::collections::BTreeSet;
use unicode_width::UnicodeWidthChar;

/// Pandoc measures Hangul vowel and trailing Jamo independently, even when
/// terminals compose them with a leading Jamo into a single glyph.
pub fn grid_char_width(ch: char) -> usize {
    match ch {
        '\u{1160}'..='\u{11ff}' | '\u{d7b0}'..='\u{d7ff}' => 1,
        _ => ch.width().unwrap_or(0),
    }
}

/// Match grid geometry even for characters joined into Hangul or emoji glyphs.
pub fn grid_display_width(text: &str) -> usize {
    text.chars().map(grid_char_width).sum()
}

/// Slice cell content using the same columns as grid geometry and rendering.
pub fn grid_column_slice(text: &str, start: usize, end: usize) -> &str {
    column_slice_with_width(text, start, end, grid_char_width)
}

/// One laid-out cell of a grid table over the canonical (row band × fine
/// column) grid. `content` is the cell's interior text with one leading pad
/// space stripped per line, trailing whitespace trimmed, and leading/trailing
/// blank lines dropped, joined with `\n`.
#[derive(Debug, Clone)]
pub struct GridCellRect {
    pub start_row: usize,
    pub start_col: usize,
    pub row_span: usize,
    pub col_span: usize,
    pub content: String,
}

/// Canonical geometry of a grid table plus its detected cells.
#[derive(Debug, Clone)]
pub struct GridLayout {
    /// Display columns of the canonical vertical boundaries (the union of
    /// `+` positions across all sep-style lines). `cols_pos.len() - 1` fine
    /// columns.
    pub cols_pos: Vec<usize>,
    /// Indices into the input `lines` of the canonical row boundaries:
    /// sep-style lines plus hybrid content lines that carry a separator
    /// segment aligned to the canonical columns (a rowspan cell's text
    /// sharing the line with a sub-row separator, e.g. `| spans  +----+`).
    /// `row_seps.len() - 1` row bands.
    pub row_seps: Vec<usize>,
    /// Subset of `row_seps` that are full sep-style lines (no cell text).
    /// The alignment-bearing separator is picked from these; a hybrid
    /// line's text could contain `=` or `:` without being an alignment row.
    pub full_seps: Vec<usize>,
    pub cells: Vec<GridCellRect>,
}

/// Analyze a grid table's lines into its canonical geometry and cell tiling.
///
/// `lines` must already be dedented to the table's own left edge (no container
/// indent). Returns `None` when the input doesn't form a grid (fewer than two
/// column boundaries or fewer than two separator lines).
#[allow(clippy::needless_range_loop)]
pub fn analyze_grid(lines: &[&str]) -> Option<GridLayout> {
    // Tabs require a tab-stop-aware source map. Until geometry supports one,
    // leave these tables to callers' preservation paths.
    if lines.is_empty() || lines.iter().any(|line| line.contains('\t')) {
        return None;
    }

    let mut grid: Vec<Vec<char>> = lines
        .iter()
        .map(|l| {
            let mut chars = Vec::new();
            for ch in l.chars() {
                let width = grid_char_width(ch);
                if width > 0 {
                    chars.push(ch);
                    chars.resize(chars.len() + width - 1, '\0');
                }
            }
            chars
        })
        .collect();
    let max_width = grid.iter().map(Vec::len).max().unwrap_or(0);
    for row in &mut grid {
        row.resize(max_width, ' ');
    }
    let nlines = grid.len();

    let is_sep_line: Vec<bool> = grid
        .iter()
        .map(|row| {
            row.contains(&'+')
                && row
                    .iter()
                    .all(|&c| matches!(c, '+' | '-' | '=' | ':' | '|' | ' '))
        })
        .collect();

    let mut col_set: BTreeSet<usize> = BTreeSet::new();
    for (i, row) in grid.iter().enumerate() {
        if !is_sep_line[i] {
            continue;
        }
        for (j, &c) in row.iter().enumerate() {
            if c == '+' {
                col_set.insert(j);
            }
        }
    }
    let cols_pos: Vec<usize> = col_set.into_iter().collect();
    if cols_pos.len() < 2 {
        return None;
    }
    let left = cols_pos[0];
    let right = cols_pos[cols_pos.len() - 1];
    if grid.iter().any(|row| {
        row[..left]
            .iter()
            .chain(&row[right + 1..])
            .any(|&ch| ch != ' ')
    }) {
        return None;
    }
    let ncols = cols_pos.len() - 1;

    let on_boundary = |pos: usize| cols_pos.binary_search(&pos).is_ok();
    let has_hybrid_sep = |row: &[char]| -> bool {
        let mut i = 0;
        while i < row.len() {
            if !matches!(row[i], '+' | '-' | '=' | ':') {
                i += 1;
                continue;
            }
            let start = i;
            while i < row.len() && matches!(row[i], '+' | '-' | '=' | ':') {
                i += 1;
            }
            let run = &row[start..i];
            if run.len() >= 3
                && run[0] == '+'
                && run[run.len() - 1] == '+'
                && run.iter().any(|&c| matches!(c, '-' | '='))
                && run
                    .iter()
                    .enumerate()
                    .all(|(k, &c)| c != '+' || on_boundary(start + k))
            {
                return true;
            }
        }
        false
    };
    let row_seps: Vec<usize> = (0..nlines)
        .filter(|&i| is_sep_line[i] || has_hybrid_sep(&grid[i]))
        .collect();
    if row_seps.len() < 2 {
        return None;
    }
    let full_seps: Vec<usize> = row_seps
        .iter()
        .copied()
        .filter(|&i| is_sep_line[i])
        .collect();
    let nrows = row_seps.len() - 1;

    let mut occupied = vec![vec![false; ncols]; nrows];
    let mut cells: Vec<GridCellRect> = Vec::new();
    for sr in 0..nrows {
        for sc in 0..ncols {
            if occupied[sr][sc] {
                continue;
            }
            let i = row_seps[sr];
            let j = cols_pos[sc];
            if grid[i][j] != '+' {
                continue;
            }
            let Some((er, ec, content)) =
                find_grid_cell(&grid, lines, sr, sc, &cols_pos, &row_seps)
            else {
                continue;
            };
            for r in sr..er {
                for c in sc..ec {
                    occupied[r][c] = true;
                }
            }
            cells.push(GridCellRect {
                start_row: sr,
                start_col: sc,
                row_span: er - sr,
                col_span: ec - sc,
                content,
            });
        }
    }

    Some(GridLayout {
        cols_pos,
        row_seps,
        full_seps,
        cells,
    })
}

#[allow(clippy::needless_range_loop)]
fn find_grid_cell(
    grid: &[Vec<char>],
    lines: &[&str],
    sr: usize,
    sc: usize,
    cols_pos: &[usize],
    row_seps: &[usize],
) -> Option<(usize, usize, String)> {
    let i = row_seps[sr];
    let j = cols_pos[sc];
    let nrows = row_seps.len() - 1;
    let ncols = cols_pos.len() - 1;

    for ec in (sc + 1)..=ncols {
        let k = cols_pos[ec];
        let top_ok = (j + 1..k).all(|c| matches!(grid[i][c], '-' | '=' | ':' | '+'));
        if !top_ok {
            break;
        }
        for er in (sr + 1)..=nrows {
            let l = row_seps[er];
            let left_ok = (i + 1..l).all(|r| matches!(grid[r][j], '|' | '+'));
            if !left_ok {
                break;
            }
            let right_ok = (i + 1..l).all(|r| matches!(grid[r][k], '|' | '+'));
            if !right_ok {
                continue;
            }
            let bot_ok = (j + 1..k).all(|c| matches!(grid[l][c], '-' | '=' | ':' | '+'));
            if !bot_ok {
                continue;
            }
            if grid[l][j] != '+' || grid[l][k] != '+' {
                continue;
            }
            let interior_split = (i + 1..l).any(|m| {
                grid[m][j] == '+'
                    && grid[m][k] == '+'
                    && (j + 1..k).all(|c| matches!(grid[m][c], '-' | '=' | ':' | '+'))
            });
            if interior_split {
                continue;
            }

            let mut content_lines: Vec<String> = Vec::new();
            for r in (i + 1)..l {
                let slice = grid_column_slice(lines[r], j + 1, k);
                let stripped = slice.strip_prefix(' ').unwrap_or(slice);
                content_lines.push(stripped.trim_end().to_string());
            }
            let first = content_lines.iter().position(|s| !s.is_empty());
            let last = content_lines.iter().rposition(|s| !s.is_empty());
            let content = match (first, last) {
                (Some(f), Some(l)) => content_lines[f..=l].join("\n"),
                _ => String::new(),
            };
            return Some((er, ec, content));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(layout: &GridLayout, r: usize, c: usize) -> &GridCellRect {
        layout
            .cells
            .iter()
            .find(|cell| cell.start_row == r && cell.start_col == c)
            .unwrap_or_else(|| panic!("no cell at ({r}, {c})"))
    }

    #[test]
    fn unicode_cells_use_display_columns_without_losing_text() {
        let lines = [
            "+----------+----+",
            "| 界 e\u{301} 😀  | x  |",
            "+----------+----+",
        ];
        let layout = analyze_grid(&lines).unwrap();
        assert_eq!(layout.cells.len(), 2);
        assert_eq!(cell(&layout, 0, 0).content, "界 e\u{301} 😀");
        assert_eq!(cell(&layout, 0, 1).content, "x");
    }

    #[test]
    fn decomposed_hangul_uses_pandoc_columns() {
        let lines = [
            "+--------+--------+",
            "| 가 + B         |",
            "+========+========+",
            "| 가    | ok     |",
            "+--------+--------+",
        ];
        let layout = analyze_grid(&lines).unwrap();
        assert_eq!(layout.cells.len(), 3);
        assert_eq!(cell(&layout, 0, 0).col_span, 2);
        assert_eq!(cell(&layout, 0, 0).content, "가 + B");
        assert_eq!(cell(&layout, 1, 0).content, "가");
        assert_eq!(cell(&layout, 1, 1).content, "ok");
    }

    /// A rowspan cell's text sharing a line with a sub-row separator
    /// (`| spans  +--------+`) splits the row band at that line, like
    /// pandoc's `gridtables` tracing.
    #[test]
    fn hybrid_text_separator_line_splits_row_band() {
        let lines = [
            "+--------+--------+",
            "| Name   | Value  |",
            "+:======:+=======:+",
            "| group  | 1.5    |",
            "| spans  +--------+",
            "| rows   | 22.0   |",
            "+--------+--------+",
        ];
        let layout = analyze_grid(&lines).unwrap();
        assert_eq!(layout.row_seps, vec![0, 2, 4, 6]);
        assert_eq!(layout.full_seps, vec![0, 2, 6]);
        let group = cell(&layout, 1, 0);
        assert_eq!(group.row_span, 2);
        assert_eq!(group.content, "group\nspans\nrows");
        assert_eq!(cell(&layout, 1, 1).content, "1.5");
        assert_eq!(cell(&layout, 2, 1).content, "22.0");
    }

    /// A `+--+` run inside ordinary cell text whose `+`s do not sit on
    /// canonical columns must not invent a row boundary.
    #[test]
    fn plus_run_off_boundary_is_not_a_row_sep() {
        let lines = [
            "+--------+--------+",
            "| a +--+ | 1      |",
            "+--------+--------+",
        ];
        let layout = analyze_grid(&lines).unwrap();
        assert_eq!(layout.row_seps, vec![0, 2]);
        assert_eq!(cell(&layout, 0, 0).content, "a +--+");
    }
}
