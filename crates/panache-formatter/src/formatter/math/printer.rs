//! Width-aware printer for the math document IR.

// Flat printing is staged for the environment-grid lowering that follows this
// engine-only change.
#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::ir::Ir;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Flat,
    Break,
}

#[derive(Clone, Copy)]
struct Command<'a> {
    indent: usize,
    mode: Mode,
    document: &'a Ir,
}

pub(super) struct Printer {
    line_width: usize,
    indent_width: usize,
    /// Render `HardLine`, `EmptyLine`, and multiline `Verbatim` as a single
    /// space instead of a newline. Inline math lives inside a host line, so a
    /// newline there would break the surrounding paragraph or table cell.
    flatten_forced_breaks: bool,
    /// Memo for [`Printer::bounded_align_fits`], keyed by aligned document,
    /// start column, and indent. Deciding one bounded alignment renders its
    /// whole subtree, and a nested alignment is re-decided once per enclosing
    /// decision, so without this the renders compound.
    bounded_align_fits: RefCell<HashMap<(*const Ir, usize, usize), bool>>,
}

impl Printer {
    pub(super) fn new(line_width: usize, indent_width: usize) -> Self {
        Self {
            line_width,
            indent_width,
            flatten_forced_breaks: false,
            bounded_align_fits: RefCell::new(HashMap::new()),
        }
    }

    /// Print a document with its first visible token at `initial_indent`.
    pub(super) fn print(&self, document: &Ir, initial_indent: usize) -> String {
        self.run(document, initial_indent, 0, true, Mode::Break)
    }

    /// Print the whole document on one line, regardless of the configured
    /// width and of any forced break the document carries.
    pub(super) fn print_flat(&self, document: &Ir) -> String {
        let mut printer = self.wide();
        printer.flatten_forced_breaks = true;
        printer.run(document, 0, 0, false, Mode::Flat)
    }

    /// Return the flat source width, or `None` for an unflattenable document.
    pub(super) fn flat_width(&self, document: &Ir) -> Option<usize> {
        self.wide().flat_end(0, document)
    }

    fn wide(&self) -> Self {
        Self {
            line_width: usize::MAX / 2,
            indent_width: self.indent_width,
            flatten_forced_breaks: self.flatten_forced_breaks,
            bounded_align_fits: RefCell::new(HashMap::new()),
        }
    }

    fn run(
        &self,
        document: &Ir,
        base_indent: usize,
        initial_column: usize,
        emit_initial_indent: bool,
        initial_mode: Mode,
    ) -> String {
        let mut writer = Writer::new(
            initial_column,
            base_indent,
            emit_initial_indent,
            self.flatten_forced_breaks,
        );
        let mut stack = vec![Command {
            indent: base_indent,
            mode: initial_mode,
            document,
        }];

        while let Some(command) = stack.pop() {
            match command.document {
                Ir::Nil => {}
                Ir::Text(text) => writer.write_text(text),
                Ir::Verbatim(text) if self.flatten_forced_breaks => {
                    writer.write_flattened_verbatim(text)
                }
                Ir::Verbatim(text) => writer.write_verbatim(text),
                Ir::Concat(documents) => {
                    for document in documents.iter().rev() {
                        stack.push(Command {
                            document,
                            ..command
                        });
                    }
                }
                Ir::Line => match command.mode {
                    Mode::Flat => writer.write_text(" "),
                    Mode::Break => writer.newline(command.indent),
                },
                Ir::SoftLine => {
                    if command.mode == Mode::Break {
                        writer.newline(command.indent);
                    }
                }
                Ir::HardLine | Ir::EmptyLine if self.flatten_forced_breaks => {
                    writer.write_separating_space()
                }
                Ir::HardLine => writer.newline(command.indent),
                Ir::EmptyLine => writer.empty_line(command.indent),
                Ir::Indent(inner) => stack.push(Command {
                    indent: command.indent + self.indent_width,
                    document: inner,
                    ..command
                }),
                Ir::Align(width, inner) => stack.push(Command {
                    indent: command.indent + width,
                    document: inner,
                    ..command
                }),
                Ir::BoundedAlign { aligned, fallback } => {
                    let document = if command.mode == Mode::Flat
                        || self.bounded_align_fits(writer.current_column(), command.indent, aligned)
                    {
                        aligned
                    } else {
                        fallback
                    };
                    stack.push(Command {
                        document,
                        ..command
                    });
                }
                Ir::Group(inner) => {
                    let mode = if command.mode == Mode::Flat
                        || self.group_fits(writer.current_column(), inner, &stack)
                    {
                        Mode::Flat
                    } else {
                        Mode::Break
                    };
                    stack.push(Command {
                        mode,
                        document: inner,
                        ..command
                    });
                }
            }
        }

        writer.output
    }

    fn group_fits(&self, start_column: usize, inner: &Ir, rest: &[Command<'_>]) -> bool {
        self.flat_end(start_column, inner)
            .is_some_and(|end| self.rest_fits(end, rest))
    }

    fn flat_end(&self, start_column: usize, document: &Ir) -> Option<usize> {
        let mut column = start_column;
        let mut stack = vec![document];
        while let Some(document) = stack.pop() {
            match document {
                Ir::Nil | Ir::SoftLine => {}
                Ir::Text(text) => {
                    column = column.saturating_add(text.chars().count());
                }
                Ir::Verbatim(text) if !text.contains('\n') => {
                    column = column.saturating_add(text.chars().count());
                }
                Ir::Line => column = column.saturating_add(1),
                Ir::HardLine | Ir::EmptyLine | Ir::Verbatim(_) => return None,
                Ir::Concat(documents) => stack.extend(documents.iter().rev()),
                Ir::Indent(inner) | Ir::Align(_, inner) | Ir::Group(inner) => stack.push(inner),
                Ir::BoundedAlign { aligned, .. } => stack.push(aligned),
            }
            if column > self.line_width {
                return None;
            }
        }
        Some(column)
    }

    /// Whether what remains on the line after a group still fits.
    ///
    /// `rest` is the caller's pending stack, walked from the top down without
    /// copying it; only the documents this walk expands are buffered.
    fn rest_fits(&self, start_column: usize, rest: &[Command<'_>]) -> bool {
        let mut column = start_column;
        let mut work: Vec<Command<'_>> = Vec::new();
        let mut pending = rest.len();
        loop {
            let command = match work.pop() {
                Some(command) => command,
                None if pending > 0 => {
                    pending -= 1;
                    rest[pending]
                }
                None => return true,
            };
            match command.document {
                Ir::Nil => {}
                Ir::SoftLine if command.mode == Mode::Flat => {}
                Ir::SoftLine | Ir::HardLine | Ir::EmptyLine => return true,
                Ir::Text(text) => column = column.saturating_add(text.chars().count()),
                Ir::Verbatim(text) => {
                    let first = text
                        .split_once('\n')
                        .map_or(text.as_ref(), |(first, _)| first);
                    column = column.saturating_add(first.chars().count());
                    if text.contains('\n') {
                        return column <= self.line_width;
                    }
                }
                Ir::Line => {
                    if command.mode == Mode::Break {
                        return true;
                    }
                    column = column.saturating_add(1);
                }
                Ir::Concat(documents) => {
                    for document in documents.iter().rev() {
                        work.push(Command {
                            document,
                            ..command
                        });
                    }
                }
                Ir::Indent(inner) | Ir::Align(_, inner) => work.push(Command {
                    document: inner,
                    ..command
                }),
                Ir::BoundedAlign { aligned, fallback } => {
                    let document = if command.mode == Mode::Flat
                        || self.bounded_align_fits(column, command.indent, aligned)
                    {
                        aligned
                    } else {
                        fallback
                    };
                    work.push(Command {
                        document,
                        ..command
                    });
                }
                Ir::Group(inner) => {
                    let mode =
                        if command.mode == Mode::Flat || self.flat_end(column, inner).is_some() {
                            Mode::Flat
                        } else {
                            Mode::Break
                        };
                    work.push(Command {
                        mode,
                        document: inner,
                        ..command
                    });
                }
            }
            if column > self.line_width {
                return false;
            }
        }
    }

    fn bounded_align_fits(&self, start_column: usize, indent: usize, aligned: &Rc<Ir>) -> bool {
        let key = (Rc::as_ptr(aligned), start_column, indent);
        if let Some(&fits) = self.bounded_align_fits.borrow().get(&key) {
            return fits;
        }
        let rendered = self.run(aligned, indent, start_column, false, Mode::Break);
        let fits = rendered
            .split('\n')
            .skip(1)
            .all(|line| line.chars().count() <= self.line_width);
        self.bounded_align_fits.borrow_mut().insert(key, fits);
        fits
    }
}

struct Writer {
    output: String,
    column: usize,
    pending_indent: usize,
    needs_indent: bool,
    /// Flat printing joins logical lines with a single space. The lines it
    /// joins already carry their own layout indentation as literal text, so
    /// that indentation is dropped at every join.
    flatten: bool,
    drop_leading_space: bool,
}

impl Writer {
    fn new(column: usize, pending_indent: usize, needs_indent: bool, flatten: bool) -> Self {
        Self {
            output: String::new(),
            column,
            pending_indent,
            needs_indent,
            flatten,
            drop_leading_space: false,
        }
    }

    fn current_column(&self) -> usize {
        self.column
            + if self.needs_indent {
                self.pending_indent
            } else {
                0
            }
    }

    fn flush_indent(&mut self) {
        if self.needs_indent {
            self.output.push_str(&" ".repeat(self.pending_indent));
            self.column += self.pending_indent;
            self.needs_indent = false;
        }
    }

    fn write_text(&mut self, text: &str) {
        let text = if self.drop_leading_space {
            text.trim_start_matches([' ', '\t'])
        } else {
            text
        };
        if text.is_empty() {
            return;
        }
        self.drop_leading_space = false;
        self.flush_indent();
        self.output.push_str(text);
        self.column += text.chars().count();
    }

    /// Write one space, unless the line is still empty or already ends in one.
    fn write_separating_space(&mut self) {
        self.drop_leading_space = false;
        if !self.output.is_empty() && !self.output.ends_with(' ') {
            self.write_text(" ");
        }
        self.drop_leading_space = self.flatten;
    }

    /// Write opaque text with its newlines collapsed to single spaces.
    fn write_flattened_verbatim(&mut self, text: &str) {
        let mut first = true;
        for segment in text.split('\n') {
            if !first {
                self.write_separating_space();
            }
            first = false;
            self.write_text(segment.trim());
        }
    }

    fn write_verbatim(&mut self, text: &str) {
        let mut first = true;
        for segment in text.split('\n') {
            if first {
                self.flush_indent();
                first = false;
            } else {
                self.output.push('\n');
                self.column = 0;
                self.needs_indent = false;
            }
            self.output.push_str(segment);
            self.column += segment.chars().count();
        }
    }

    fn newline(&mut self, indent: usize) {
        self.output.push('\n');
        self.column = 0;
        self.pending_indent = indent;
        self.needs_indent = indent > 0;
    }

    fn empty_line(&mut self, indent: usize) {
        self.output.push_str("\n\n");
        self.column = 0;
        self.pending_indent = indent;
        self.needs_indent = indent > 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn printer(width: usize) -> Printer {
        Printer::new(width, 2)
    }

    #[test]
    fn group_stays_flat_when_it_fits() {
        let document = Ir::group(Ir::concat([
            Ir::text("f("),
            Ir::indent(Ir::concat([Ir::SoftLine, Ir::text("x")])),
            Ir::SoftLine,
            Ir::text(")"),
        ]));
        assert_eq!(printer(80).print(&document, 0), "f(x)");
    }

    #[test]
    fn hard_line_forces_enclosing_group_open() {
        let block = Ir::concat([
            Ir::text("\\begin{x}"),
            Ir::indent(Ir::concat([Ir::HardLine, Ir::text("a")])),
            Ir::HardLine,
            Ir::text("\\end{x}"),
        ]);
        let document = Ir::group(Ir::concat([
            Ir::text("f("),
            Ir::indent(Ir::concat([Ir::SoftLine, block])),
            Ir::SoftLine,
            Ir::text(")"),
        ]));
        assert_eq!(
            printer(80).print(&document, 0),
            "f(\n  \\begin{x}\n    a\n  \\end{x}\n)"
        );
    }

    #[test]
    fn align_hangs_block_and_keeps_punctuation_attached() {
        let block = Ir::align(
            4,
            Ir::concat([
                Ir::text("\\begin{x}"),
                Ir::indent(Ir::concat([Ir::HardLine, Ir::text("a")])),
                Ir::HardLine,
                Ir::text("\\end{x}"),
            ]),
        );
        let document = Ir::concat([Ir::text("v = "), block, Ir::text(",")]);
        assert_eq!(
            printer(80).print(&document, 2),
            "  v = \\begin{x}\n        a\n      \\end{x},"
        );
    }

    #[test]
    fn broken_group_uses_line_and_soft_line_distinctly() {
        let document = Ir::group(Ir::concat([
            Ir::text("f("),
            Ir::indent(Ir::concat([
                Ir::SoftLine,
                Ir::text("aaaaaaaa"),
                Ir::Line,
                Ir::text("bbbbbbbb"),
            ])),
            Ir::SoftLine,
            Ir::text(")"),
        ]));
        assert_eq!(
            printer(10).print(&document, 0),
            "f(\n  aaaaaaaa\n  bbbbbbbb\n)"
        );
    }

    #[test]
    fn join_does_not_separate_empty_documents() {
        let document = Ir::join(Ir::Line, [Ir::text("a,"), Ir::Nil]);
        assert_eq!(printer(80).print(&document, 0), "a,");
    }

    #[test]
    fn group_fit_accounts_for_trailing_same_line_content() {
        let document = Ir::concat([
            Ir::text("("),
            Ir::group(Ir::concat([Ir::text("aaaa"), Ir::Line, Ir::text("bbbb")])),
            Ir::text(")"),
        ]);
        assert_eq!(printer(10).print(&document, 0), "(aaaa\nbbbb)");
    }

    #[test]
    fn bounded_alignment_falls_back_when_a_continuation_would_overflow() {
        let aligned = Ir::align(8, Ir::concat([Ir::text("a"), Ir::Line, Ir::text("bbbb")]));
        let fallback = Ir::concat([Ir::text("a"), Ir::Line, Ir::text("bbbb")]);
        let document = Ir::group(Ir::concat([
            Ir::text("xxxxxxxx"),
            Ir::Line,
            Ir::bounded_align(aligned, fallback),
        ]));
        assert_eq!(printer(10).print(&document, 0), "xxxxxxxx\na\nbbbb");
    }

    #[test]
    fn empty_line_never_carries_indentation() {
        let document = Ir::indent(Ir::concat([Ir::text("a"), Ir::EmptyLine, Ir::text("b")]));
        assert_eq!(printer(80).print(&document, 0), "a\n\n  b");
    }

    #[test]
    fn multiline_verbatim_forces_an_enclosing_group_open() {
        let document = Ir::group(Ir::concat([
            Ir::text("{"),
            Ir::indent(Ir::concat([Ir::SoftLine, Ir::verbatim("a\nb")])),
            Ir::SoftLine,
            Ir::text("}"),
        ]));
        assert_eq!(printer(80).print(&document, 0), "{\n  a\nb\n}");
    }

    #[test]
    fn flat_print_ignores_width_and_forced_breaks() {
        let document = Ir::concat([
            Ir::text("aaaaaaaa"),
            Ir::Line,
            Ir::text("bbbbbbbb"),
            Ir::SoftLine,
            Ir::text("c"),
            Ir::HardLine,
            Ir::text("d"),
        ]);
        assert_eq!(printer(4).print_flat(&document), "aaaaaaaa bbbbbbbbc d");
        assert_eq!(printer(4).flat_width(&document), None);
    }

    #[test]
    fn flat_print_drops_the_layout_indent_of_joined_lines() {
        let document = Ir::join(
            Ir::HardLine,
            [
                Ir::text("\\begin{x}"),
                Ir::text("  a & b \\\\"),
                Ir::text("  c & d"),
                Ir::text("\\end{x}"),
            ],
        );
        assert_eq!(
            printer(4).print_flat(&document),
            "\\begin{x} a & b \\\\ c & d \\end{x}"
        );
    }

    #[test]
    fn flat_print_collapses_multiline_verbatim() {
        let document = Ir::concat([
            Ir::text("a"),
            Ir::HardLine,
            Ir::verbatim("b\nc"),
            Ir::HardLine,
        ]);
        assert_eq!(printer(4).print_flat(&document), "a b c ");
    }
}
