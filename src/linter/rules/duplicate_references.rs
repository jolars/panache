use crate::config::Config;
use crate::linter::diagnostics::{Diagnostic, Location};
use crate::linter::rules::Rule;
use crate::syntax::SyntaxNode;

pub struct DuplicateReferencesRule;

impl Rule for DuplicateReferencesRule {
    fn name(&self) -> &str {
        "duplicate-reference-labels"
    }

    fn check(
        &self,
        tree: &SyntaxNode,
        input: &str,
        _config: &Config,
        _metadata: Option<&crate::metadata::DocumentMetadata>,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // Check for duplicate reference definitions
        diagnostics.extend(check_duplicate_references(tree, input));
        diagnostics.extend(check_duplicate_crossref_labels(tree, input));

        // Check for duplicate footnote definitions
        diagnostics.extend(check_duplicate_footnotes(tree, input));

        diagnostics
    }
}

fn check_duplicate_references(tree: &SyntaxNode, input: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let db = crate::salsa::SalsaDb::default();
    let extensions = crate::config::Extensions::default();
    let index = crate::salsa::symbol_usage_index_from_tree(&db, tree, &extensions);

    for (label, ranges) in index.reference_definition_entries() {
        if ranges.len() < 2 {
            continue;
        }
        let first_location = Location::from_range(ranges[0], input);
        for range in ranges.iter().skip(1) {
            let display = extract_definition_label(input, *range).unwrap_or_else(|| label.clone());
            diagnostics.push(Diagnostic::warning(
                Location::from_range(*range, input),
                "duplicate-reference-labels",
                format!(
                    "Duplicate reference label '[{}]' (first defined at line {})",
                    display, first_location.line
                ),
            ));
        }
    }

    diagnostics
}

fn check_duplicate_footnotes(tree: &SyntaxNode, input: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let db = crate::salsa::SalsaDb::default();
    let extensions = crate::config::Extensions::default();
    let index = crate::salsa::symbol_usage_index_from_tree(&db, tree, &extensions);

    for (id, ranges) in index.footnote_definition_entries() {
        if ranges.len() < 2 {
            continue;
        }
        let first_location = Location::from_range(ranges[0], input);
        for range in ranges.iter().skip(1) {
            let display =
                extract_definition_label(input, *range).unwrap_or_else(|| format!("^{}", id));
            diagnostics.push(Diagnostic::warning(
                Location::from_range(*range, input),
                "duplicate-reference-labels",
                format!(
                    "Duplicate footnote ID '[^{}]' (first defined at line {})",
                    display.trim_start_matches('^'),
                    first_location.line
                ),
            ));
        }
    }

    diagnostics
}

fn extract_definition_label(input: &str, range: rowan::TextRange) -> Option<String> {
    let start: usize = range.start().into();
    let end: usize = range.end().into();
    let text = input.get(start..end)?;
    let open = text.find('[')?;
    let close = text[open + 1..].find(']')?;
    let label = &text[open + 1..open + 1 + close];
    if label.is_empty() {
        None
    } else {
        Some(label.to_string())
    }
}

fn check_duplicate_crossref_labels(tree: &SyntaxNode, input: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let db = crate::salsa::SalsaDb::default();
    let extensions = crate::config::Extensions::default();
    let index = crate::salsa::symbol_usage_index_from_tree(&db, tree, &extensions);

    for (label, ranges) in index.crossref_declaration_entries() {
        if ranges.len() < 2 {
            continue;
        }

        let declaration_value_ranges = index
            .crossref_declaration_value_ranges(label)
            .cloned()
            .unwrap_or_default();

        // Cross-reference labels are case-sensitive in Quarto/Bookdown contexts.
        // The symbol index groups by normalized label for lookup features, so this
        // lint rule must compare raw declaration values to avoid false positives.
        if declaration_value_ranges.len() == ranges.len() {
            use std::collections::HashMap;

            let mut declarations_by_raw_label: HashMap<&str, Vec<rowan::TextRange>> =
                HashMap::new();

            for (declaration_range, value_range) in
                ranges.iter().zip(declaration_value_ranges.iter())
            {
                let raw_label = range_text(input, *value_range);
                declarations_by_raw_label
                    .entry(raw_label)
                    .or_default()
                    .push(*declaration_range);
            }

            for (raw_label, declaration_ranges) in declarations_by_raw_label {
                if declaration_ranges.len() < 2 {
                    continue;
                }
                let first_location = Location::from_range(declaration_ranges[0], input);
                for range in declaration_ranges.iter().skip(1) {
                    diagnostics.push(Diagnostic::warning(
                        Location::from_range(*range, input),
                        "duplicate-reference-labels",
                        format!(
                            "Duplicate cross-reference label '[{}]' (first defined at line {})",
                            raw_label, first_location.line
                        ),
                    ));
                }
            }
            continue;
        }

        // Fallback if declaration/value range alignment is unavailable.
        let first_location = Location::from_range(ranges[0], input);
        for range in ranges.iter().skip(1) {
            diagnostics.push(Diagnostic::warning(
                Location::from_range(*range, input),
                "duplicate-reference-labels",
                format!(
                    "Duplicate cross-reference label '[{}]' (first defined at line {})",
                    label, first_location.line
                ),
            ));
        }
    }

    diagnostics
}

fn range_text(input: &str, range: rowan::TextRange) -> &str {
    let start: usize = range.start().into();
    let end: usize = range.end().into();
    input.get(start..end).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Flavor};

    fn parse_and_lint(input: &str) -> Vec<Diagnostic> {
        let config = Config::default();
        // Use main parse function which now includes inline parsing
        let tree = crate::parser::parse(input, Some(config.clone()));

        let rule = DuplicateReferencesRule;
        rule.check(&tree, input, &config, None)
    }

    #[test]
    fn test_no_duplicates() {
        let input = r#"[ref1]: https://example.com
[ref2]: https://another.com
[ref3]: https://third.com
"#;
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_duplicate_reference_labels() {
        let input = r#"[ref1]: https://example.com
[ref1]: https://duplicate.com
"#;
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "duplicate-reference-labels");
        assert!(diagnostics[0].message.contains("[ref1]"));
        assert!(diagnostics[0].message.contains("line 1"));
    }

    #[test]
    fn test_multiple_duplicates() {
        let input = r#"[ref1]: https://example.com
[ref1]: https://duplicate1.com
[ref2]: https://another.com
[ref1]: https://duplicate2.com
[ref2]: https://duplicate3.com
"#;
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 3); // 2 duplicates of ref1, 1 duplicate of ref2
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code == "duplicate-reference-labels")
        );
    }

    #[test]
    fn test_case_insensitive_matching() {
        let input = r#"[MyRef]: https://example.com
[myref]: https://duplicate.com
[MYREF]: https://another-duplicate.com
"#;
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 2); // Second and third are duplicates of first
        let joined = diagnostics
            .iter()
            .map(|d| d.message.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("[myref]"));
        assert!(joined.contains("[MYREF]"));
    }

    #[test]
    fn test_whitespace_normalization() {
        let input = r#"[my ref]: https://example.com
[my  ref]: https://duplicate.com
[my   ref]: https://another-duplicate.com
"#;
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 2); // Extra whitespace should be normalized
    }

    #[test]
    fn test_duplicate_footnotes() {
        let input = r#"[^1]: First footnote
[^1]: Duplicate footnote
"#;
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "duplicate-reference-labels");
        assert!(diagnostics[0].message.contains("[^1]"));
    }

    #[test]
    fn test_footnote_case_insensitive() {
        let input = r#"[^Note]: First footnote
[^note]: Duplicate footnote
[^NOTE]: Another duplicate
"#;
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 2);
        let joined = diagnostics
            .iter()
            .map(|d| d.message.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("[^note]"));
        assert!(joined.contains("[^NOTE]"));
    }

    #[test]
    fn test_mixed_references_and_footnotes() {
        let input = r#"[ref]: https://example.com
[ref]: https://duplicate.com

[^1]: Footnote content
[^1]: Duplicate footnote
"#;
        let diagnostics = parse_and_lint(input);
        assert_eq!(diagnostics.len(), 2); // One duplicate ref, one duplicate footnote
    }

    #[test]
    fn test_first_definition_not_flagged() {
        let input = r#"[ref]: https://example.com
[ref]: https://duplicate.com
"#;
        let diagnostics = parse_and_lint(input);
        // Only the second occurrence should be flagged
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].location.line, 2);
    }

    #[test]
    fn test_duplicate_chunk_label_and_attribute_id() {
        let input = r#"See @fig-plot.

```{r}
#| label: fig-plot
plot(1:10)
```

```{r}
#| label: fig-plot
plot(1:10)
```
"#;
        let config = Config {
            flavor: Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        };
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = DuplicateReferencesRule;
        let diagnostics = rule.check(&tree, input, &config, None);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "duplicate-reference-labels");
        assert!(diagnostics[0].message.contains("fig-plot"));
    }

    #[test]
    fn test_crossref_chunk_labels_are_case_sensitive() {
        let input = r#"See @fig-foo and @fig-FOO.

```{r}
#| label: fig-foo
plot(1:10)
```

```{r}
#| label: fig-FOO
plot(1:10)
```
"#;
        let config = Config {
            flavor: Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        };
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = DuplicateReferencesRule;
        let diagnostics = rule.check(&tree, input, &config, None);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_crossref_heading_ids_are_case_sensitive() {
        let input = r#"# Heading {#em}

A reference to [Heading](#em).

# Heading {#EM}

A reference to [Heading](#EM).
"#;
        let config = Config {
            flavor: Flavor::Pandoc,
            extensions: crate::config::Extensions::for_flavor(Flavor::Pandoc),
            ..Default::default()
        };
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = DuplicateReferencesRule;
        let diagnostics = rule.check(&tree, input, &config, None);
        assert_eq!(diagnostics.len(), 0);
    }
}
