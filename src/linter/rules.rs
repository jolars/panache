use std::collections::HashSet;

use crate::config::Config;
use crate::linter::diagnostics::{Diagnostic, Severity};
use crate::linter::index::LintIndex;
use crate::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

pub mod adjacent_footnote_refs;
pub mod blank_line_in_display_math;
pub mod blank_line_in_inline_footnote;
pub mod chunk_label_spaces;
pub mod citation_keys;
pub mod citation_nonbreaking_space;
pub mod consumer_divergence;
pub mod crossref_as_link_target;
pub mod duplicate_references;
pub mod duplicate_yaml_anchor;
pub mod emoji_aliases;
pub mod empty_list_item;
pub mod empty_values;
pub mod figure_crossref_captions;
pub mod footnote_after_image;
pub mod footnote_ref_in_footnote_def;
pub mod footnote_swallowed_by_bracket;
pub mod heading_eaten_attrs;
pub mod heading_hierarchy;
pub mod heading_strip_comments_residue;
pub mod html_entities;
pub mod html_unclosed_attribute;
pub mod inline_math_line_break;
pub mod link_text_is_url;
pub mod malformed_citation;
pub mod math_content;
pub mod missing_chunk_labels;
pub mod quarto_schema;
pub mod reversed_footnote_marker;
pub mod space_before_punctuation;
pub mod stray_fenced_div_markers;
pub mod swallowed_list_marker;
pub mod table_column_count;
pub mod undefined_anchor;
pub mod undefined_references;
pub mod unspaced_citation;
pub mod unspaced_list_marker;
pub mod unsupported_metadata_key;
pub mod unused_definitions;
pub mod unused_yaml_anchor;

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
    /// Needs `extensions.inline-footnotes` (the `^[note]` form specifically;
    /// a flavor can enable reference footnotes without it).
    InlineFootnotes,
    /// Needs `extensions.citations`.
    Citations,
    /// Needs `extensions.fenced-code-attributes`.
    FencedCodeAttributes,
    /// Needs `extensions.fenced-divs`.
    FencedDivs,
    /// Needs `extensions.emoji`.
    Emoji,
    /// Needs `extensions.raw-html`.
    RawHtml,
    /// Needs any `tex-math-*` extension (dollars, gfm, jupyter, single/double backslash).
    TexMath,
    /// Needs `extensions.tex-math-dollars`.
    TexMathDollars,
    /// Needs a flavor with executable chunks (Quarto or R Markdown).
    ChunkFlavor,
    /// Needs the Quarto flavor specifically (e.g. Quarto schema validation).
    Quarto,
    /// Needs a flavor whose frontmatter reaches pandoc's metadata layer
    /// (Pandoc, Quarto, R Markdown) --- i.e. one whose frontmatter consumer set
    /// includes libyaml. See
    /// [`YamlValidationContext`](crate::parser::yaml::YamlValidationContext).
    PandocMetadata,
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
            Requirement::InlineFootnotes => ext.inline_footnotes,
            Requirement::Citations => ext.citations,
            Requirement::FencedCodeAttributes => ext.fenced_code_attributes,
            Requirement::FencedDivs => ext.fenced_divs,
            Requirement::Emoji => ext.emoji,
            Requirement::RawHtml => ext.raw_html,
            Requirement::TexMath => {
                ext.tex_math_dollars
                    || ext.tex_math_gfm
                    || ext.tex_math_single_backslash
                    || ext.tex_math_double_backslash
                    || ext.tex_math_jupyter
            }
            Requirement::TexMathDollars => ext.tex_math_dollars,
            Requirement::ChunkFlavor => {
                matches!(config.flavor, Flavor::Quarto | Flavor::RMarkdown)
            }
            Requirement::Quarto => matches!(config.flavor, Flavor::Quarto),
            Requirement::PandocMetadata => {
                use crate::parser::yaml::{YamlConsumer, YamlValidationContext};
                YamlValidationContext::frontmatter(config.flavor)
                    .consumers()
                    .contains(YamlConsumer::Libyaml)
            }
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
/// The reference generator reads these facts directly. The documentation tests
/// also check them against the diagnostics produced by executable examples.
#[derive(Debug, Clone)]
pub struct RuleMeta {
    /// Rule name, as used in `[lint]` selection lists and the docs' `### \`name\`` heading.
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

    /// Explain what the rule catches and how the author can resolve it.
    fn description(&self) -> &'static str {
        ""
    }

    /// Examples used to generate the reference and verify its diagnostic codes.
    #[cfg(not(target_arch = "wasm32"))]
    fn examples(&self) -> &'static [crate::linter::docs::Example] {
        &[]
    }

    /// Explain individual codes when a rule emits more than one diagnostic.
    fn code_description(&self, _code: &str) -> Option<&'static str> {
        None
    }

    /// Declarative metadata describing the rule's gating, severity, auto-fix
    /// support, and the set of diagnostic codes it can emit. Keep this in sync
    /// with the rule body; `tests/linter_rules_docs.rs` checks the examples
    /// against it and verifies the generated reference.
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
            symbols: None,
            project_symbols: None,
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
    /// A salsa-memoized [`SymbolUsageIndex`](crate::salsa::SymbolUsageIndex) for
    /// `tree`, when the caller has one (the `built_in_lint_plan` query does).
    /// Rules read it via [`LintContext::symbol_index`] instead of rebuilding it
    /// per rule off a throwaway database, so unchanged blocks stay memoized
    /// across edits.
    pub symbols: Option<&'a crate::salsa::SymbolUsageIndex>,
    /// The salsa-memoized project-wide symbol aggregate for cross-file rules,
    /// when the document participates in a Quarto/bookdown project and the
    /// caller has one (the `built_in_lint_plan` query does). Rules read it via
    /// [`LintContext::project_symbol_index`]; standalone callers get a
    /// filesystem-built fallback instead.
    pub project_symbols: Option<&'a crate::linter::project_index::ProjectSymbolIndex>,
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

    /// The document's [`SymbolUsageIndex`](crate::salsa::SymbolUsageIndex).
    ///
    /// Returns the caller-provided memoized index when present, else builds one
    /// from `tree` on a throwaway database (the path used by standalone
    /// `check_tree` callers and tests). The buckets every rule consumes are
    /// either extension-independent or the caller's memoized index was built
    /// with the real config extensions, so the two paths are equivalent.
    pub fn symbol_index(&self) -> std::borrow::Cow<'_, crate::salsa::SymbolUsageIndex> {
        match self.symbols {
            Some(symbols) => std::borrow::Cow::Borrowed(symbols),
            None => {
                let db = crate::salsa::SalsaDb::default();
                std::borrow::Cow::Owned(crate::salsa::symbol_usage_index_from_tree(
                    &db,
                    self.tree,
                    &self.config.extensions,
                ))
            }
        }
    }

    /// The project-wide symbol aggregate for cross-file resolution, or `None`
    /// when the document is standalone (not part of a Quarto/bookdown project).
    ///
    /// Returns the caller-provided salsa-memoized aggregate when present (the
    /// `built_in_lint_plan` path). Otherwise, for standalone `check_tree`
    /// callers and tests, it resolves the project from the document's metadata
    /// and folds the sibling documents off disk — the same fold as the salsa
    /// path, just without cross-file memoization.
    pub fn project_symbol_index(
        &self,
    ) -> Option<std::borrow::Cow<'_, crate::linter::project_index::ProjectSymbolIndex>> {
        if let Some(project_symbols) = self.project_symbols {
            return Some(std::borrow::Cow::Borrowed(project_symbols));
        }
        let metadata = self.metadata?;
        let doc_path = metadata
            .source_path
            .canonicalize()
            .unwrap_or_else(|_| metadata.source_path.clone());
        let roots = crate::includes::find_project_roots(&doc_path);
        let project_root = roots.bookdown_first()?;
        let is_bookdown = roots.bookdown.is_some();
        Some(std::borrow::Cow::Owned(
            crate::linter::project_index::build_from_fs(
                &project_root,
                &doc_path,
                self.config,
                is_bookdown,
            ),
        ))
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
