use std::collections::HashSet;

use crate::config::Config;
use crate::linter::diagnostics::{Diagnostic, Severity};
use crate::linter::index::LintIndex;
use crate::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

pub mod adjacent_footnote_refs;
pub mod chunk_label_spaces;
pub mod citation_keys;
pub mod consumer_divergence;
pub mod crossref_as_link_target;
pub mod duplicate_references;
pub mod emoji_aliases;
pub mod empty_list_item;
pub mod empty_values;
pub mod figure_crossref_captions;
pub mod footnote_ref_in_footnote_def;
pub mod heading_eaten_attrs;
pub mod heading_hierarchy;
pub mod heading_strip_comments_residue;
pub mod html_entities;
pub mod link_text_is_url;
pub mod math_content;
pub mod missing_chunk_labels;
pub mod quarto_schema;
pub mod stray_fenced_div_markers;
pub mod undefined_anchor;
pub mod undefined_references;
pub mod unused_definitions;

/// Config precondition for a rule's diagnostics to be able to fire.
///
/// This is the single source of truth for both registration gating (see
/// `default_registry`) and the "Requirements" field documented in
/// `docs/reference/linter-rules.qmd`. A rule whose preconditions are not met is
/// never registered, so its diagnostics cannot fire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Requirement {
    /// Eligible on every flavor (subject only to the rule being enabled).
    Always,
    /// Needs `extensions.header-attributes`.
    HeaderAttributes,
    /// Needs `extensions.footnotes`.
    Footnotes,
    /// Needs `extensions.citations`.
    Citations,
    /// Needs `extensions.fenced-code-attributes`.
    FencedCodeAttributes,
    /// Needs `extensions.fenced-divs`.
    FencedDivs,
    /// Needs `extensions.emoji`.
    Emoji,
    /// Needs any `tex-math-*` extension (dollars, gfm, single/double backslash).
    TexMath,
    /// Needs a flavor with executable chunks (Quarto or R Markdown).
    ChunkFlavor,
    /// Needs the Quarto flavor specifically (e.g. Quarto schema validation).
    Quarto,
}

impl Requirement {
    /// Whether `config` satisfies this requirement.
    pub fn is_satisfied(self, config: &Config) -> bool {
        use crate::config::Flavor;
        let ext = &config.extensions;
        match self {
            Requirement::Always => true,
            Requirement::HeaderAttributes => ext.header_attributes,
            Requirement::Footnotes => ext.footnotes,
            Requirement::Citations => ext.citations,
            Requirement::FencedCodeAttributes => ext.fenced_code_attributes,
            Requirement::FencedDivs => ext.fenced_divs,
            Requirement::Emoji => ext.emoji,
            Requirement::TexMath => {
                ext.tex_math_dollars
                    || ext.tex_math_gfm
                    || ext.tex_math_single_backslash
                    || ext.tex_math_double_backslash
            }
            Requirement::ChunkFlavor => {
                matches!(config.flavor, Flavor::Quarto | Flavor::RMarkdown)
            }
            Requirement::Quarto => matches!(config.flavor, Flavor::Quarto),
        }
    }
}

/// One diagnostic code a rule may emit, paired with the severity it rides at.
/// A single rule can emit several distinct codes at different severities (e.g.
/// `citation-keys`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticCode {
    pub code: &'static str,
    pub severity: Severity,
}

impl DiagnosticCode {
    pub const fn warning(code: &'static str) -> Self {
        Self {
            code,
            severity: Severity::Warning,
        }
    }

    pub const fn error(code: &'static str) -> Self {
        Self {
            code,
            severity: Severity::Error,
        }
    }

    pub const fn info(code: &'static str) -> Self {
        Self {
            code,
            severity: Severity::Info,
        }
    }
}

/// Declarative, machine-readable facts about a built-in rule.
///
/// This is the source of truth that the reference docs are checked against (see
/// `tests/linter_rules_docs.rs`): every fact here must be reflected in
/// `docs/reference/linter-rules.qmd`, and the docs may not list a rule or code
/// that does not appear here.
#[derive(Debug, Clone)]
pub struct RuleMeta {
    /// Rule name, as used in `[lint.rules]` and the docs' `### \`name\`` heading.
    pub name: &'static str,
    /// `true` when the rule runs unless explicitly disabled; `false` when it is
    /// opt-in (must be explicitly enabled). Documented as "Default: Off" when
    /// `false`.
    pub default_on: bool,
    /// Config precondition for the rule's diagnostics; also gates registration.
    pub requires: Requirement,
    /// Whether the rule offers any auto-fix.
    pub auto_fix: bool,
    /// Every diagnostic code the rule may emit, with each code's severity.
    pub codes: &'static [DiagnosticCode],
}

pub trait Rule {
    fn name(&self) -> &str;

    /// Declarative metadata describing the rule's gating, severity, auto-fix
    /// support, and the set of diagnostic codes it can emit. Keep this in sync
    /// with the rule body; `tests/linter_rules_docs.rs` checks the reference
    /// docs against it.
    fn metadata(&self) -> RuleMeta;

    /// Node kinds this rule wants collected for it by the shared walk. The
    /// runner builds one [`LintIndex`] over the union of every registered
    /// rule's interests; the rule then reads its bucket via
    /// [`LintContext::nodes`] instead of walking the tree itself. Default is
    /// empty (e.g. salsa-index-backed rules that don't bucket-walk at all).
    fn node_interests(&self) -> &'static [SyntaxKind] {
        &[]
    }

    /// Whether this rule scans `TEXT` tokens (read via
    /// [`LintContext::text_tokens`]). Default `false` keeps the token list out
    /// of the shared walk unless some rule needs it.
    fn wants_text_tokens(&self) -> bool {
        false
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic>;

    /// Build a one-off [`LintIndex`] for just this rule's interests and run it.
    /// The runner uses a single shared index instead; this is the convenient
    /// entry point for unit tests and any single-rule caller.
    fn check_tree(
        &self,
        tree: &SyntaxNode,
        input: &str,
        config: &Config,
        metadata: Option<&crate::metadata::DocumentMetadata>,
    ) -> Vec<Diagnostic> {
        let want_kinds: HashSet<SyntaxKind> = self.node_interests().iter().copied().collect();
        let index = LintIndex::build(tree, &want_kinds, self.wants_text_tokens());
        let cx = LintContext {
            tree,
            input,
            config,
            metadata,
            index: &index,
        };
        self.check(&cx)
    }
}

/// Per-lint-pass inputs handed to every rule's [`Rule::check`]. Bundles the
/// document, its source/config/metadata, and the shared [`LintIndex`] so rules
/// read pre-bucketed nodes instead of re-walking the tree.
pub struct LintContext<'a> {
    pub tree: &'a SyntaxNode,
    pub input: &'a str,
    pub config: &'a Config,
    pub metadata: Option<&'a crate::metadata::DocumentMetadata>,
    pub index: &'a LintIndex,
}

impl LintContext<'_> {
    /// Nodes of `kind` from the shared walk, in document order.
    pub fn nodes(&self, kind: SyntaxKind) -> &[SyntaxNode] {
        self.index.nodes(kind)
    }

    /// All `TEXT` tokens from the shared walk (requires
    /// [`Rule::wants_text_tokens`]).
    pub fn text_tokens(&self) -> &[SyntaxToken] {
        self.index.text_tokens()
    }
}

pub struct RuleRegistry {
    rules: Vec<Box<dyn Rule>>,
}

impl RuleRegistry {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    pub fn register(&mut self, rule: Box<dyn Rule>) {
        self.rules.push(rule);
    }

    pub fn rules(&self) -> &[Box<dyn Rule>] {
        &self.rules
    }
}

impl Default for RuleRegistry {
    fn default() -> Self {
        Self::new()
    }
}
