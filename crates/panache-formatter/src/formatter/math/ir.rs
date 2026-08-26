//! Document algebra for math formatting.
//!
//! This is the math-focused subset of Badness's formatter IR. It stays
//! Panache-owned and contains every primitive needed by the planned typed math
//! lowering, without importing the prose- and general-LaTeX-only machinery.

// Several primitives are intentionally staged for the typed lowering slices
// that follow this engine-only change.
#![allow(dead_code)]

use std::rc::Rc;

/// A document whose final layout is selected by [`super::printer::Printer`].
#[derive(Clone, Debug)]
pub(super) enum Ir {
    /// Literal single-line text.
    Text(Rc<str>),
    /// Documents printed back-to-back.
    Concat(Rc<[Ir]>),
    /// A space when flat, or a newline when broken.
    Line,
    /// Nothing when flat, or a newline when broken.
    SoftLine,
    /// An unconditional newline.
    HardLine,
    /// An unconditional blank line.
    EmptyLine,
    /// One configured indentation step around the inner document.
    Indent(Rc<Ir>),
    /// An explicit hanging indentation in source columns.
    Align(usize, Rc<Ir>),
    /// Prefer the aligned layout unless one of its continuation lines overflows.
    BoundedAlign { aligned: Rc<Ir>, fallback: Rc<Ir> },
    /// A flat-or-broken layout decision.
    Group(Rc<Ir>),
    /// Opaque source text. Multiline text forces enclosing groups to break.
    Verbatim(Rc<str>),
    /// No output.
    Nil,
}

impl Ir {
    pub(super) fn text(text: impl Into<Rc<str>>) -> Self {
        let text = text.into();
        debug_assert!(!text.contains('\n'));
        if text.is_empty() {
            Self::Nil
        } else {
            Self::Text(text)
        }
    }

    pub(super) fn verbatim(text: impl Into<Rc<str>>) -> Self {
        let text = text.into();
        if text.is_empty() {
            Self::Nil
        } else {
            Self::Verbatim(text)
        }
    }

    pub(super) fn concat(documents: impl IntoIterator<Item = Ir>) -> Self {
        let documents: Vec<_> = documents
            .into_iter()
            .filter(|document| !matches!(document, Self::Nil))
            .collect();
        match documents.len() {
            0 => Self::Nil,
            1 => documents.into_iter().next().expect("one document"),
            _ => Self::Concat(documents.into()),
        }
    }

    pub(super) fn join(separator: Ir, documents: impl IntoIterator<Item = Ir>) -> Self {
        let mut output = Vec::new();
        for document in documents
            .into_iter()
            .filter(|document| !matches!(document, Self::Nil))
        {
            if !output.is_empty() {
                output.push(separator.clone());
            }
            output.push(document);
        }
        Self::concat(output)
    }

    pub(super) fn indent(inner: Ir) -> Self {
        Self::Indent(Rc::new(inner))
    }

    pub(super) fn align(width: usize, inner: Ir) -> Self {
        if width == 0 || matches!(inner, Self::Nil) {
            inner
        } else {
            Self::Align(width, Rc::new(inner))
        }
    }

    pub(super) fn bounded_align(aligned: Ir, fallback: Ir) -> Self {
        Self::BoundedAlign {
            aligned: Rc::new(aligned),
            fallback: Rc::new(fallback),
        }
    }

    pub(super) fn group(inner: Ir) -> Self {
        Self::Group(Rc::new(inner))
    }

    pub(super) fn contains_forced_break(&self) -> bool {
        match self {
            Self::HardLine | Self::EmptyLine => true,
            Self::Verbatim(text) => text.contains('\n'),
            Self::Concat(documents) => documents.iter().any(Self::contains_forced_break),
            Self::Indent(inner) | Self::Align(_, inner) | Self::Group(inner) => {
                inner.contains_forced_break()
            }
            Self::BoundedAlign { aligned, .. } => aligned.contains_forced_break(),
            Self::Text(_) | Self::Line | Self::SoftLine | Self::Nil => false,
        }
    }

    /// Return the width of the flat form, or `None` when the document must
    /// break. Environment rows use this to align authored `\\` markers.
    pub(super) fn flat_width(&self) -> Option<usize> {
        match self {
            Self::Text(text) => Some(text.chars().count()),
            Self::Verbatim(text) => (!text.contains('\n')).then(|| text.chars().count()),
            Self::Concat(documents) => documents.iter().try_fold(0usize, |width, document| {
                width.checked_add(document.flat_width()?)
            }),
            Self::Line => Some(1),
            Self::SoftLine | Self::Nil => Some(0),
            Self::Indent(inner) | Self::Align(_, inner) | Self::Group(inner) => inner.flat_width(),
            Self::BoundedAlign { aligned, .. } => aligned.flat_width(),
            Self::HardLine | Self::EmptyLine => None,
        }
    }
}
