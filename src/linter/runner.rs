use std::collections::HashSet;

use crate::config::Config;
#[cfg(not(target_arch = "wasm32"))]
use crate::linter::code_block_collector::concatenate_with_blanks_and_mapping;
use crate::linter::diagnostics::Diagnostic;
#[cfg(not(target_arch = "wasm32"))]
use crate::linter::external_linters::ExternalLinterRegistry;
#[cfg(not(target_arch = "wasm32"))]
use crate::linter::external_linters::{find_missing_linter_commands, log_missing_linter_commands};
use crate::linter::index::LintIndex;
use crate::linter::rules::LintContext;
use crate::linter::rules::RuleRegistry;
use crate::syntax::{SyntaxKind, SyntaxNode};
#[cfg(not(target_arch = "wasm32"))]
use crate::utils::collect_code_blocks;

pub struct LintRunner {
    registry: RuleRegistry,
    #[cfg(not(target_arch = "wasm32"))]
    external_linters: ExternalLinterRegistry,
}

impl LintRunner {
    pub fn new(registry: RuleRegistry) -> Self {
        Self {
            registry,
            #[cfg(not(target_arch = "wasm32"))]
            external_linters: ExternalLinterRegistry::new(),
        }
    }

    pub fn run(&self, tree: &SyntaxNode, input: &str, config: &Config) -> Vec<Diagnostic> {
        self.run_with_metadata(tree, input, config, None)
    }

    pub fn run_with_metadata(
        &self,
        tree: &SyntaxNode,
        input: &str,
        config: &Config,
        metadata: Option<&crate::metadata::DocumentMetadata>,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // One shared walk for every registered rule: bucket the nodes/tokens
        // they collectively care about (and compute ignore regions in the same
        // pass) instead of each rule re-walking the whole tree.
        let mut want_kinds: HashSet<SyntaxKind> = HashSet::new();
        let mut want_tokens = false;
        for rule in self.registry.rules() {
            want_kinds.extend(rule.node_interests().iter().copied());
            want_tokens |= rule.wants_text_tokens();
        }
        let index = LintIndex::build(tree, &want_kinds, want_tokens);
        let ignored_ranges = index.ignored_ranges();

        let cx = LintContext {
            tree,
            input,
            config,
            metadata,
            index: &index,
        };

        // Run built-in rules
        for rule in self.registry.rules() {
            log::debug!("Running lint rule: {}", rule.name());
            let rule_diagnostics = rule.check(&cx);
            log::debug!(
                "Rule {} found {} diagnostic(s)",
                rule.name(),
                rule_diagnostics.len()
            );

            // Filter out diagnostics in ignored ranges
            for diagnostic in rule_diagnostics {
                let byte_offset: usize = diagnostic.location.range.start().into();
                let is_ignored = ignored_ranges
                    .iter()
                    .any(|(start, end)| byte_offset >= *start && byte_offset < *end);

                log::debug!(
                    "Diagnostic at byte {}: is_ignored={}",
                    byte_offset,
                    is_ignored
                );

                if !is_ignored {
                    diagnostics.push(diagnostic);
                }
            }
        }

        diagnostics.sort_by_key(|d| (d.location.line, d.location.column));
        diagnostics
    }

    /// Run external linters on code blocks (sync version for CLI).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn run_with_external_linters_sync(
        &self,
        tree: &SyntaxNode,
        input: &str,
        config: &Config,
        metadata: Option<&crate::metadata::DocumentMetadata>,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = self.run_with_metadata(tree, input, config, metadata);

        // If no external linters configured, return early
        if config.linters.is_empty() {
            return diagnostics;
        }

        let missing_linter_commands = find_missing_linter_commands(
            config.linters.values().map(String::as_str),
            &self.external_linters,
        );
        log_missing_linter_commands(&missing_linter_commands);

        // Collect code blocks by language
        let code_blocks = collect_code_blocks(tree, input);

        // Resolve which (language, linter) pairs are actually runnable. This
        // pre-pass stays sequential: the skip/warn logging is cheap and its
        // order should be stable. The expensive subprocess work happens below.
        let mut jobs: Vec<(&str, &str, &[crate::utils::CodeBlock])> = Vec::new();
        for (language, linter_name) in &config.linters {
            let Some(linter_info) = self.external_linters.get(linter_name) else {
                log::warn!(
                    "Skipping unknown external linter '{}' configured for language '{}'",
                    linter_name,
                    language
                );
                continue;
            };
            if missing_linter_commands.contains(linter_info.command) {
                continue;
            }

            if !self
                .external_linters
                .supports_language(linter_name, language)
                .unwrap_or(false)
            {
                log::warn!(
                    "Skipping external linter '{}' for unsupported language '{}'; supported languages: {}",
                    linter_name,
                    language,
                    linter_info.supported_languages.join(", ")
                );
                continue;
            }

            let Some(blocks) = code_blocks.get(language) else {
                continue;
            };
            if blocks.is_empty() {
                continue;
            }

            jobs.push((language.as_str(), linter_name.as_str(), blocks));
        }

        // Each job is an independent, subprocess-bound external linter run; the
        // diagnostics are merged and re-sorted below, so completion order
        // doesn't matter. Run them concurrently (capped) instead of
        // back-to-back so a polyglot document doesn't pay the sum of every
        // linter's latency.
        //
        // Borrow the registry alone rather than all of `self`: `LintRunner`
        // holds non-`Sync` `dyn Rule`s, but `ExternalLinterRegistry` is
        // shareable, so this keeps the closure `Sync` for rayon.
        let external_linters = &self.external_linters;
        let run_one = |(language, linter_name, blocks): (
            &str,
            &str,
            &[crate::utils::CodeBlock],
        )|
         -> Vec<Diagnostic> {
            log::debug!(
                "Running external linter '{}' for {} code blocks in language '{}'",
                linter_name,
                blocks.len(),
                language
            );

            let concatenated_result = concatenate_with_blanks_and_mapping(blocks);
            match crate::linter::external_linters_sync::run_linter_sync(
                linter_name,
                language,
                &concatenated_result.content,
                input,
                external_linters,
                Some(&concatenated_result.mappings),
            ) {
                Ok(external_diagnostics) => {
                    log::debug!(
                        "External linter '{}' found {} diagnostic(s)",
                        linter_name,
                        external_diagnostics.len()
                    );
                    external_diagnostics
                }
                Err(e) => {
                    log::warn!("External linter '{}' failed: {}", linter_name, e);
                    // Continue with other linters — don't fail the whole pass.
                    Vec::new()
                }
            }
        };

        let external_diagnostics: Vec<Diagnostic> = if jobs.len() <= 1 {
            // Common single-language case: skip the pool-build overhead.
            jobs.into_iter().flat_map(run_one).collect()
        } else {
            use rayon::prelude::*;
            // TODO(external-tool-budget): formatters (`run_formatters_parallel`)
            // and linters each build their own pool capped at
            // `external_max_parallel`, so a concurrent format + lint can spin up
            // to 2× that many subprocess threads. Coordinate a single shared
            // external-tool thread budget across both paths at a later stage.
            let cap = config.external_max_parallel.max(1).min(jobs.len());
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(cap)
                .build()
                .expect("failed to build external linter thread pool");
            pool.install(|| jobs.into_par_iter().flat_map(run_one).collect())
        };
        diagnostics.extend(external_diagnostics);

        diagnostics.sort_by_key(|d| (d.location.line, d.location.column));
        diagnostics
    }
}
