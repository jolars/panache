//! `consumer-divergence`: flag a plain YAML scalar whose resolved type/value
//! differs across the document's active YAML consumers.
//!
//! This is the high-value core of yamllint's `truthy`/`octal-values` rules
//! *without* their version-agnostic "never write `no`" nag. Panache knows which
//! real parsers read each YAML region (the consumer profiles behind
//! [`YamlValidationContext`]), so it can flag only *genuine* ambiguity: a value
//! that two stages of the same toolchain resolve differently.
//!
//! In practice the only context with two distinct YAML versions is **Quarto
//! frontmatter**, read by both pandoc (libyaml ≈ YAML 1.1) and Quarto's js-yaml
//! (YAML 1.2 core). `country: no` is the boolean `false` to pandoc but the
//! string `"no"` to js-yaml; `mode: 0755` is octal `493` vs a string. Pandoc
//! frontmatter (libyaml only), RMarkdown frontmatter (both ≈ 1.1), and hashpipe
//! `#|` options (single consumer) carry only one version, so they never flag —
//! the `< 2 versions` guard skips them even though registration only gates on
//! the Quarto flavor.
//!
//! The auto-fix single-quotes the value, forcing a string under every consumer.
//! It is **unsafe** (applied only under `--unsafe-fixes`) because if the author
//! meant the boolean/integer, quoting changes the value for the 1.1 stage — the
//! resolution is an author-intent decision the rule can't make.

use crate::config::Flavor;
use crate::linter::diagnostics::{Diagnostic, DiagnosticNoteKind, Edit, Fix, Location};
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::linter::yaml_resolve::{Resolved, YamlVersion, resolve_plain, version_of};
use crate::parser::yaml::{YamlConsumer, YamlLocation, YamlValidationContext};
use crate::syntax::{
    AstNode, SyntaxKind, SyntaxNode, YamlBlockMapEntry, YamlScalar, YamlScalarStyle,
};
use rowan::{TextRange, TextSize};

pub const CONSUMER_DIVERGENCE: &str = "consumer-divergence";

pub struct ConsumerDivergenceRule;

impl Rule for ConsumerDivergenceRule {
    fn name(&self) -> &str {
        CONSUMER_DIVERGENCE
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: CONSUMER_DIVERGENCE,
            default_on: true,
            requires: Requirement::Quarto,
            auto_fix: true,
            codes: const { &[DiagnosticCode::warning(CONSUMER_DIVERGENCE)] },
        }
    }

    fn node_interests(&self) -> &'static [SyntaxKind] {
        &[SyntaxKind::YAML_BLOCK_MAP_ENTRY]
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for node in cx.nodes(SyntaxKind::YAML_BLOCK_MAP_ENTRY) {
            if let Some(diag) = classify(node, cx.config.flavor, cx.input) {
                diagnostics.push(diag);
            }
        }
        diagnostics
    }
}

fn classify(node: &SyntaxNode, flavor: Flavor, input: &str) -> Option<Diagnostic> {
    let entry = YamlBlockMapEntry::cast(node.clone())?;
    let scalar = entry.value()?.as_scalar()?;
    // Only plain scalars can diverge: quotes/block-headers pin the string tag.
    if scalar.style() != YamlScalarStyle::Plain {
        return None;
    }
    let raw = scalar.raw();
    let text = raw.trim();
    // Multi-line plain scalars are not the simple word/number tokens this rule
    // reasons about.
    if text.is_empty() || text.contains('\n') {
        return None;
    }

    // Which YAML versions actually read this region?
    let location = yaml_location(node)?;
    let versions = active_versions(flavor, location);
    if versions.len() < 2 {
        return None;
    }

    // Resolve under 1.1 (pandoc/libyaml) and 1.2 core (js-yaml) and compare.
    let r11 = resolve_plain(text, YamlVersion::V1_1);
    let r12 = resolve_plain(text, YamlVersion::V1_2Core);
    if r11 == r12 {
        return None;
    }

    let key = entry.key_text();
    let location_diag = Location::from_range(value_range(&scalar), input);

    let message = match &key {
        Some(k) => format!(
            "Key `{k}`: value `{text}` is {a} to pandoc (YAML 1.1) but {b} to Quarto's js-yaml (1.2)",
            a = describe(r11, text),
            b = describe(r12, text),
        ),
        None => format!(
            "Value `{text}` is {a} to pandoc (YAML 1.1) but {b} to Quarto's js-yaml (1.2)",
            a = describe(r11, text),
            b = describe(r12, text),
        ),
    };

    let fix = Fix::unsafe_fix(
        format!("Quote the value as `'{text}'`"),
        vec![Edit {
            range: value_range(&scalar),
            replacement: format!("'{text}'"),
        }],
    );

    Some(
        Diagnostic::warning(location_diag, CONSUMER_DIVERGENCE, message)
            .with_note(
                DiagnosticNoteKind::Help,
                "Quote the value to force a string under every consumer, or write the explicit `true`/`false` (or canonical integer) you mean",
            )
            .with_fix(fix),
    )
}

/// The distinct resolution behaviors among the consumers that read a (flavor,
/// location) region. Reuses the consumer profiles so the version set stays in
/// lockstep with the validator.
fn active_versions(flavor: Flavor, location: YamlLocation) -> Vec<YamlVersion> {
    let consumers = YamlValidationContext::new(flavor, location).consumers();
    let mut versions = Vec::new();
    for consumer in [
        YamlConsumer::Libyaml,
        YamlConsumer::Jsyaml,
        YamlConsumer::RYaml,
    ] {
        if consumers.contains(consumer) {
            let v = version_of(consumer);
            if !versions.contains(&v) {
                versions.push(v);
            }
        }
    }
    versions
}

/// Whether the entry sits in document frontmatter or in a hashpipe `#|` block,
/// from its nearest embedding wrapper. `None` if it is neither (defensive).
fn yaml_location(node: &SyntaxNode) -> Option<YamlLocation> {
    node.ancestors().find_map(|anc| match anc.kind() {
        SyntaxKind::YAML_METADATA_CONTENT => Some(YamlLocation::Frontmatter),
        SyntaxKind::HASHPIPE_YAML_CONTENT => Some(YamlLocation::Hashpipe),
        _ => None,
    })
}

fn describe(resolved: Resolved, text: &str) -> String {
    match resolved {
        Resolved::Bool(true) => "the boolean `true`".to_string(),
        Resolved::Bool(false) => "the boolean `false`".to_string(),
        Resolved::Int(i) => format!("the integer `{i}`"),
        Resolved::Str => format!("the string `\"{text}\"`"),
    }
}

/// The byte range of the scalar's trimmed token, excluding any surrounding
/// trivia the node range may include. Quoting exactly this token leaves
/// indentation and line breaks untouched.
fn value_range(scalar: &YamlScalar) -> TextRange {
    let raw = scalar.raw();
    let node_start: usize = scalar.text_range().start().into();
    let leading = raw.len() - raw.trim_start().len();
    let trimmed_len = raw.trim().len();
    let start = node_start + leading;
    TextRange::new(
        TextSize::new(start as u32),
        TextSize::new((start + trimmed_len) as u32),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::linter::FixSafety;
    use crate::linter::diagnostics::Edit;

    fn lint_with_flavor(input: &str, flavor: Flavor) -> Vec<Diagnostic> {
        let config = Config {
            flavor,
            ..Config::default()
        };
        let tree = crate::parser::parse(input, Some(config.clone()));
        ConsumerDivergenceRule.check_tree(&tree, input, &config, None)
    }

    fn lint(input: &str) -> Vec<Diagnostic> {
        lint_with_flavor(input, Flavor::Quarto)
    }

    fn apply_fix(d: &Diagnostic, input: &str) -> String {
        let fix = d.fix.as_ref().expect("fix present");
        let mut edits: Vec<&Edit> = fix.edits.iter().collect();
        edits.sort_by_key(|e| e.range.start());
        let mut out = String::new();
        let mut last = 0;
        for edit in edits {
            let start: usize = edit.range.start().into();
            let end: usize = edit.range.end().into();
            out.push_str(&input[last..start]);
            out.push_str(&edit.replacement);
            last = end;
        }
        out.push_str(&input[last..]);
        out
    }

    #[test]
    fn flags_norway_problem() {
        let diags = lint("---\ncountry: no\n---\n# x\n");
        assert_eq!(diags.len(), 1, "got: {diags:?}");
        assert_eq!(diags[0].code, CONSUMER_DIVERGENCE);
        assert!(diags[0].message.contains("country"));
        assert!(diags[0].message.contains("boolean `false`"));
        assert!(diags[0].message.contains("string"));
    }

    #[test]
    fn flags_yes_and_off() {
        assert_eq!(lint("---\nflag: yes\n---\n# x\n").len(), 1);
        assert_eq!(lint("---\nflag: off\n---\n# x\n").len(), 1);
    }

    #[test]
    fn flags_leading_zero_octal() {
        let diags = lint("---\nmode: 0755\n---\n# x\n");
        assert_eq!(diags.len(), 1, "got: {diags:?}");
        assert!(diags[0].message.contains("integer `493`"));
    }

    #[test]
    fn does_not_flag_quoted_value() {
        assert!(lint("---\ncountry: \"no\"\n---\n# x\n").is_empty());
        assert!(lint("---\ncountry: 'no'\n---\n# x\n").is_empty());
    }

    #[test]
    fn does_not_flag_unambiguous_values() {
        assert!(lint("---\nenabled: true\n---\n# x\n").is_empty());
        assert!(lint("---\ncount: 42\n---\n# x\n").is_empty());
        assert!(lint("---\npi: 3.14\n---\n# x\n").is_empty());
        assert!(lint("---\nx: .inf\n---\n# x\n").is_empty());
        assert!(lint("---\nname: Norway\n---\n# x\n").is_empty());
    }

    #[test]
    fn only_fires_for_quarto() {
        // Pandoc frontmatter is libyaml-only; CommonMark has no asserted
        // consumer; RMarkdown frontmatter is libyaml + R-yaml (both ≈ 1.1).
        assert!(lint_with_flavor("---\ncountry: no\n---\n# x\n", Flavor::Pandoc).is_empty());
        assert!(lint_with_flavor("---\ncountry: no\n---\n# x\n", Flavor::CommonMark).is_empty());
        assert!(lint_with_flavor("---\ncountry: no\n---\n# x\n", Flavor::RMarkdown).is_empty());
    }

    #[test]
    fn offers_unsafe_quoting_fix() {
        let input = "---\ncountry: no\n---\n# x\n";
        let diags = lint(input);
        assert_eq!(diags.len(), 1);
        let fix = diags[0].fix.as_ref().expect("fix");
        assert_eq!(fix.safety, FixSafety::Unsafe);
        assert_eq!(
            apply_fix(&diags[0], input),
            "---\ncountry: 'no'\n---\n# x\n"
        );
    }

    #[test]
    fn caret_points_at_value() {
        let input = "---\ncountry: no\n---\n# x\n";
        let diags = lint(input);
        let r = diags[0].location.range;
        let start: usize = r.start().into();
        let end: usize = r.end().into();
        assert_eq!(&input[start..end], "no");
    }
}
