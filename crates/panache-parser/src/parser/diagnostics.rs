//! Parser syntax-error channel.
//!
//! Markdown itself is never syntactically invalid — every byte sequence is some
//! lossless CST — so the block/inline parsers emit no diagnostics. Embedded
//! *sublanguages* are different: hashpipe and frontmatter YAML can be malformed,
//! and (later) LaTeX math or raw HTML may be validated too. When the parser
//! validates such a region it already knows the verdict and offset; rather than
//! discard it and force a downstream re-parse, it records a host-ranged
//! [`SyntaxError`] here, mirroring rust-analyzer's `Parse { green, errors }`.
//!
//! The CST is unchanged — invalid YAML still becomes opaque tokens. This channel
//! is purely the *diagnostic* the parser already computed, surfaced instead of
//! thrown away. It is empty for pure Markdown.

use std::cell::RefCell;
use std::rc::Rc;

use rowan::TextRange;

/// Which sublanguage validation produced a [`SyntaxError`]. Lets downstream
/// consumers (the linter) map to the right diagnostic code without the parser
/// knowing linter codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxErrorSource {
    /// Embedded YAML — frontmatter metadata or a hashpipe option preamble.
    Yaml,
}

/// A syntax error the parser found in an embedded sublanguage, with a
/// **host-aligned** byte range (ready to turn into a diagnostic without any
/// offset remapping).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxError {
    pub range: TextRange,
    pub message: String,
    pub source: SyntaxErrorSource,
}

/// Interior-mutable sink the single-pass parser pushes into while building.
///
/// Cloning shares the same backing store (it is an `Rc`), so it threads through
/// the block dispatcher (on `BlockContext`) as an owned value — sidestepping any
/// `&self` borrow that would clash with the `&mut GreenNodeBuilder` held during
/// emission. The handful of clones per parse are cheap pointer bumps.
#[derive(Debug, Clone, Default)]
pub struct Diagnostics {
    errors: Rc<RefCell<Vec<SyntaxError>>>,
}

impl Diagnostics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a syntax error.
    pub fn push(&self, error: SyntaxError) {
        self.errors.borrow_mut().push(error);
    }

    /// Drain the recorded errors. Called once after the parse completes.
    pub fn take(&self) -> Vec<SyntaxError> {
        std::mem::take(&mut self.errors.borrow_mut())
    }
}
