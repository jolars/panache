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
//! Positions are **character** indices (matching pandoc, which lays grid tables
//! out on the source character grid), not display columns.

use std::collections::BTreeSet;

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
    /// Character columns of the canonical vertical boundaries (the union of
    /// `+` positions across all sep-style lines). `cols_pos.len() - 1` fine
    /// columns.
    pub cols_pos: Vec<usize>,
    /// Indices into the input `lines` of the sep-style lines (canonical row
    /// boundaries). `row_seps.len() - 1` row bands.
    pub row_seps: Vec<usize>,
    pub cells: Vec<GridCellRect>,
}

/// Analyze a grid table's lines into its canonical geometry and cell tiling.
///
/// `lines` must already be dedented to the table's own left edge (no container
/// indent). Returns `None` when the input doesn't form a grid (fewer than two
/// column boundaries or fewer than two separator lines).
#[allow(clippy::needless_range_loop)]
pub fn analyze_grid(lines: &[&str]) -> Option<GridLayout> {
    if lines.is_empty() {
        return None;
    }

    // Pad lines into a 2D char grid.
    let max_width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let grid: Vec<Vec<char>> = lines
        .iter()
        .map(|l| {
            let mut chars: Vec<char> = l.chars().collect();
            chars.resize(max_width, ' ');
            chars
        })
        .collect();
    let nlines = grid.len();

    // A line is "sep-style" if it contains at least one `+` and no chars
    // outside `+`/`-`/`=`/`:`/`|`/` `. Partial separators (lines mixing `|`
    // and `+`) qualify; content lines do not.
    let is_sep_line: Vec<bool> = grid
        .iter()
        .map(|row| {
            row.contains(&'+')
                && row
                    .iter()
                    .all(|&c| matches!(c, '+' | '-' | '=' | ':' | '|' | ' '))
        })
        .collect();

    // Canonical column boundaries: union of `+` columns across all sep-style lines.
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
    let ncols = cols_pos.len() - 1;

    // Canonical row boundaries: line indices of sep-style lines.
    let row_seps: Vec<usize> = (0..nlines).filter(|&i| is_sep_line[i]).collect();
    if row_seps.len() < 2 {
        return None;
    }
    let nrows = row_seps.len() - 1;

    // Detect cells.
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
                // No corner here — the canonical column is missing on this
                // sep line, meaning the cell that owns this position must
                // have been emitted earlier and `occupied` should already be
                // set. If not, the table is malformed; skip.
                continue;
            }
            let Some((er, ec, content)) = find_grid_cell(&grid, i, j, sr, sc, &cols_pos, &row_seps)
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
        cells,
    })
}

/// Find the smallest valid grid-table cell with its top-left `+` at
/// `(i, j)` in the char grid, where `(sr, sc)` are the canonical row /
/// column indices of that corner.
///
/// Returns `(end_row_idx, end_col_idx, content_text)` where the cell
/// occupies canonical rows `sr..end_row_idx` and canonical columns
/// `sc..end_col_idx`. Content is the text inside the cell, with one
/// leading-space pad stripped per line and trailing whitespace trimmed,
/// joined with `\n`.
#[allow(clippy::needless_range_loop)]
fn find_grid_cell(
    grid: &[Vec<char>],
    i: usize,
    j: usize,
    sr: usize,
    sc: usize,
    cols_pos: &[usize],
    row_seps: &[usize],
) -> Option<(usize, usize, String)> {
    let nrows = row_seps.len() - 1;
    let ncols = cols_pos.len() - 1;

    for ec in (sc + 1)..=ncols {
        let k = cols_pos[ec];
        // Top edge (i, j+1..k) must be all sep chars (intermediate `+`s OK).
        let top_ok = (j + 1..k).all(|c| matches!(grid[i][c], '-' | '=' | ':' | '+'));
        if !top_ok {
            // Hit a `|` or ` `; can't extend further right.
            break;
        }
        for er in (sr + 1)..=nrows {
            let l = row_seps[er];
            // Left edge col j from i+1..l: chars in {|, +}.
            let left_ok = (i + 1..l).all(|r| matches!(grid[r][j], '|' | '+'));
            if !left_ok {
                break;
            }
            // Right edge col k from i+1..l: chars in {|, +}.
            let right_ok = (i + 1..l).all(|r| matches!(grid[r][k], '|' | '+'));
            if !right_ok {
                continue;
            }
            // Bottom edge (l, j+1..k): chars in {-, =, :, +}.
            let bot_ok = (j + 1..k).all(|c| matches!(grid[l][c], '-' | '=' | ':' | '+'));
            if !bot_ok {
                continue;
            }
            if grid[l][j] != '+' || grid[l][k] != '+' {
                continue;
            }
            // No interior partial separator that fully spans this cell.
            // A line m strictly between i and l splits the cell if it has
            // `+` at both col j and col k AND all chars between are sep
            // chars (i.e., the partial sep extends across the whole cell
            // horizontally).
            let interior_split = (i + 1..l).any(|m| {
                grid[m][j] == '+'
                    && grid[m][k] == '+'
                    && (j + 1..k).all(|c| matches!(grid[m][c], '-' | '=' | ':' | '+'))
            });
            if interior_split {
                continue;
            }

            // Extract content text. For each interior line, take chars
            // [j+1..k], strip one leading space (cell padding), trim
            // trailing whitespace.
            let mut content_lines: Vec<String> = Vec::new();
            for r in (i + 1)..l {
                let slice: String = grid[r][j + 1..k].iter().collect();
                let stripped = slice.strip_prefix(' ').unwrap_or(&slice).to_string();
                content_lines.push(stripped.trim_end().to_string());
            }
            // Drop leading/trailing empty lines.
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
