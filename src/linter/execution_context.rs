//! Quarto execution units with occurrence-preserving source ownership.
//!
//! Linter adapters map into an arena of original document occurrences. Each
//! occurrence occupies a disjoint range, even when a partial is included twice.
//! Projection then restores file-local ranges and checks fix agreement before
//! hosts see diagnostics. This preserves the adapters' single-input contract.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use rowan::{TextRange, TextSize};

use crate::config::Flavor;
use crate::salsa::{Db, ExternalLintJob, FileConfig, FileText, SalsaDb};
use crate::utils::CodeSnippet;

use super::code_block_collector::concatenate_for_lint;
use super::diagnostics::{Diagnostic, DiagnosticNoteKind, Fix, Location};

#[derive(Clone, Copy)]
pub struct ExecutionRoot {
    pub file: FileText,
    pub config: FileConfig,
    /// A render target is also executed independently when another file includes it.
    pub render_target: bool,
}

#[derive(Debug, Clone)]
pub struct SourceOccurrence {
    pub path: PathBuf,
    pub text: Arc<str>,
    pub arena_range: Range<usize>,
    pub parent: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct SourceDiagnostic {
    pub path: PathBuf,
    pub diagnostic: Diagnostic,
    pub include_target: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct ExecutionPlan {
    pub root: PathBuf,
    pub sources: Vec<SourceOccurrence>,
    pub dependencies: BTreeSet<PathBuf>,
    pub diagnostics: Vec<SourceDiagnostic>,
    pub original_input: String,
    pub jobs: Vec<ExternalLintJob>,
    pub complete: bool,
    pub config_key: String,
}

/// Normalize `.` and `..` without resolving symlinks or reading the filesystem.
/// VFS identities retain the spelling used by the host, including symlink roots.
pub fn normalize_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir if result.file_name().is_some_and(|name| name != "..") => {
                result.pop();
            }
            Component::ParentDir if result.has_root() => {}
            part => result.push(part.as_os_str()),
        }
    }
    result
}

#[salsa::tracked(returns(ref), no_eq, unsafe(non_salsa_values), lru = 512)]
pub fn execution_plan(db: &dyn Db, file: FileText, config: FileConfig) -> ExecutionPlan {
    let _ = db.file_set().ids(db);
    let Some(path) = db.path_of(file) else {
        return ExecutionPlan::default();
    };
    let root = normalize_path(&path);
    let base = root.parent().unwrap_or(Path::new(".")).to_path_buf();
    let project = crate::includes::find_quarto_root(&root).unwrap_or_else(|| base.clone());
    let mut traversal = ExecutionSourceTraversal {
        db,
        config,
        base: &base,
        project: &project,
        plan: ExecutionPlan {
            root,
            complete: true,
            config_key: format!("{:?}", config.config(db)),
            ..Default::default()
        },
        snippets: BTreeMap::new(),
        active: BTreeSet::new(),
    };
    traversal.collect(file, None);
    let ExecutionSourceTraversal {
        mut plan, snippets, ..
    } = traversal;
    if plan.complete {
        let mut linters: Vec<_> = config.config(db).linters.iter().collect();
        linters.sort_by_key(|(language, _)| *language);
        for (language, linter) in linters {
            let Some(blocks) = snippets.get(language) else {
                continue;
            };
            for joined in concatenate_for_lint(blocks, config.config(db).flavor) {
                plan.jobs.push(ExternalLintJob {
                    document_path: Some(plan.root.clone()),
                    definition: config.config(db).linter_definitions.get(linter).cloned(),
                    language: language.clone(),
                    linter_name: linter.clone(),
                    content: joined.content,
                    mappings: joined.mappings,
                });
            }
        }
    }
    plan
}

struct ExecutionSourceTraversal<'a> {
    db: &'a dyn Db,
    config: FileConfig,
    base: &'a Path,
    project: &'a Path,
    plan: ExecutionPlan,
    snippets: BTreeMap<String, Vec<CodeSnippet>>,
    active: BTreeSet<PathBuf>,
}

impl ExecutionSourceTraversal<'_> {
    fn collect(&mut self, file: FileText, parent: Option<usize>) {
        self.db.unwind_if_revision_cancelled();
        let path = normalize_path(&self.db.path_of(file).expect("execution sources have paths"));
        self.plan.dependencies.insert(path.clone());
        let Some(text) = file.text(self.db).clone() else {
            self.plan.complete = false;
            return;
        };
        self.active.insert(path.clone());
        let base_offset = self.plan.original_input.len();
        self.plan.original_input.push_str(&text);
        // Unmapped separators keep a boundary insertion owned by one occurrence.
        self.plan.original_input.push('\n');
        let occurrence = self.plan.sources.len();
        self.plan.sources.push(SourceOccurrence {
            path: path.clone(),
            text: text.clone(),
            arena_range: base_offset..base_offset + text.len(),
            parent,
        });
        let tree = crate::salsa::parsed_tree_root(self.db, file, self.config);
        enum Event {
            Snippet(CodeSnippet),
            Include(crate::includes::IncludeOccurrence),
        }
        let mut events: Vec<_> = crate::utils::collect_code_snippets(&tree, &text)
            .into_values()
            .flatten()
            .map(|s| (s.original_range.start, Event::Snippet(s)))
            .collect();
        if self.config.config(self.db).flavor == Flavor::Quarto {
            events.extend(
                crate::includes::include_occurrences(
                    &tree,
                    self.base,
                    Some(self.project),
                    self.config.config(self.db),
                )
                .into_iter()
                .map(|i| (usize::from(i.range.start()), Event::Include(i))),
            );
        }
        events.sort_by_key(|(offset, _)| *offset);
        for (_, event) in events {
            match event {
                Event::Snippet(mut snippet) => {
                    snippet.original_range.start += base_offset;
                    snippet.original_range.end += base_offset;
                    for offset in &mut snippet.line_starts {
                        *offset += base_offset;
                    }
                    self.snippets
                        .entry(snippet.language.clone())
                        .or_default()
                        .push(snippet);
                }
                Event::Include(include) => {
                    let target = normalize_path(&include.path);
                    self.plan.dependencies.insert(target.clone());
                    let child = self.db.file_text(target.clone());
                    let diagnostic = if self.active.contains(&target) {
                        Some(crate::includes::include_cycle_diagnostic(
                            &text,
                            include.range,
                            &target,
                        ))
                    } else if child.is_none_or(|child| child.text(self.db).is_none()) {
                        Some(crate::includes::include_not_found_diagnostic(
                            &text,
                            include.range,
                            &target,
                        ))
                    } else {
                        None
                    };
                    if let Some(diagnostic) = diagnostic {
                        self.plan.complete = false;
                        self.plan.diagnostics.push(SourceDiagnostic {
                            path: path.clone(),
                            diagnostic,
                            include_target: Some(target),
                        });
                    } else if let Some(child) = child {
                        self.collect(child, Some(occurrence));
                    }
                }
            }
        }
        self.active.remove(&path);
    }
}

/// Load execution dependencies on the writer, preserving already loaded buffers.
pub fn load_execution_sources(db: &mut SalsaDb, roots: &[ExecutionRoot]) -> LoadedExecutionSources {
    loop {
        let mut paths = BTreeSet::new();
        for root in roots {
            paths.extend(
                execution_plan(db, root.file, root.config)
                    .dependencies
                    .iter()
                    .cloned(),
            );
        }
        let mut changed = false;
        for path in &paths {
            let was_known = db.file_text_if_cached(path).is_some();
            let id = db.intern_file(Some(path.clone()));
            changed |= db.load_file_from_disk(id) || !was_known;
        }
        if !changed {
            let read_errors = paths
                .iter()
                .filter(|path| !db.file_text_is_loaded(path))
                .filter_map(|path| {
                    let error = std::fs::read_to_string(path).err()?;
                    (!matches!(
                        error.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::IsADirectory
                    ))
                    .then(|| (path.clone(), error.to_string()))
                })
                .collect();
            return LoadedExecutionSources { paths, read_errors };
        }
    }
}

#[derive(Default)]
pub struct LoadedExecutionSources {
    pub paths: BTreeSet<PathBuf>,
    pub read_errors: BTreeMap<PathBuf, String>,
}

#[derive(Debug, Clone, Default)]
pub struct ExecutionBatch {
    pub plans: Vec<ExecutionPlan>,
    /// Source closures for explicitly selected documents, including partials.
    pub scopes: BTreeMap<PathBuf, BTreeSet<PathBuf>>,
}

impl ExecutionBatch {
    /// Attach writer-observed I/O failures without reading files on workers.
    pub fn with_read_errors(mut self, errors: &BTreeMap<PathBuf, String>) -> Self {
        for plan in &mut self.plans {
            for issue in &mut plan.diagnostics {
                if issue.diagnostic.code != "include-not-found" {
                    continue;
                }
                let Some(target) = &issue.include_target else {
                    continue;
                };
                let Some(error) = errors.get(target) else {
                    continue;
                };
                let Some(source) = plan.sources.iter().find(|source| source.path == issue.path)
                else {
                    continue;
                };
                issue.diagnostic = crate::includes::include_read_error_diagnostic(
                    &source.text,
                    issue.diagnostic.location.range,
                    target,
                    error,
                );
            }
        }
        self
    }

    pub fn new(db: &dyn Db, roots: &[ExecutionRoot], targets: &[PathBuf]) -> Self {
        let candidates: Vec<_> = roots
            .iter()
            .map(|root| (root, execution_plan(db, root.file, root.config)))
            .collect();
        let mut plans: Vec<ExecutionPlan> = candidates
            .iter()
            .filter(|(root, plan)| {
                root.render_target
                    || !candidates.iter().any(|(_, other)| {
                        other.root != plan.root
                            && other.sources.iter().skip(1).any(|s| s.path == plan.root)
                    })
            })
            .map(|(_, plan)| (*plan).clone())
            .collect();
        // A cyclic component has no parentless candidate, even when unrelated
        // roots survive selection. Retain a selected entry into each such
        // component so its include-cycle diagnostic cannot disappear.
        for target in targets {
            let target = normalize_path(target);
            if !plans
                .iter()
                .any(|plan| plan.sources.iter().any(|source| source.path == target))
                && let Some((_, plan)) = candidates.iter().find(|(_, plan)| plan.root == target)
            {
                plans.push((*plan).clone());
            }
        }
        plans.sort_by(|a, b| a.root.cmp(&b.root));
        plans.dedup_by(|a, b| a.root == b.root && a.config_key == b.config_key);
        let mut scopes = BTreeMap::new();
        for target in targets {
            let target = normalize_path(target);
            let mut scope = BTreeSet::from([target.clone()]);
            for plan in &plans {
                let mut inside = Vec::new();
                for source in &plan.sources {
                    let selected =
                        source.path == target || source.parent.is_some_and(|p| inside[p]);
                    inside.push(selected);
                    if selected {
                        scope.insert(source.path.clone());
                    }
                }
            }
            scopes.insert(target, scope);
        }
        let visible: BTreeSet<_> = scopes.values().flatten().cloned().collect();
        plans.retain(|plan| {
            plan.sources
                .iter()
                .any(|source| visible.contains(&source.path))
        });
        Self { plans, scopes }
    }

    pub fn fingerprint(&self) -> String {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        for plan in &self.plans {
            plan.root.hash(&mut hash);
            plan.config_key.hash(&mut hash);
            plan.complete.hash(&mut hash);
            plan.dependencies.hash(&mut hash);
            plan.original_input.hash(&mut hash);
            for issue in &plan.diagnostics {
                issue.diagnostic.message.hash(&mut hash);
            }
        }
        self.scopes.hash(&mut hash);
        format!("{:016x}", hash.finish())
    }

    /// Whether any contributing linter reads configuration outside this batch.
    pub fn uses_project_config(&self) -> bool {
        self.plans.iter().flat_map(|plan| &plan.jobs).any(|job| {
            crate::config::linter_uses_project_config(&job.linter_name, job.definition.as_ref())
        })
    }

    pub fn source_text(&self, path: &Path) -> Option<&str> {
        self.plans
            .iter()
            .flat_map(|p| &p.sources)
            .find(|s| s.path == path)
            .map(|s| s.text.as_ref())
    }

    pub fn run(&self) -> ExecutionResults {
        let mut results = Vec::new();
        for plan in &self.plans {
            let definitions = plan
                .jobs
                .iter()
                .filter_map(|job| job.definition.clone().map(|d| (job.linter_name.clone(), d)))
                .collect();
            let registry = super::external_linters::ExternalLinterRegistry::for_document(
                &definitions,
                Some(&plan.root),
            );
            let missing = super::external_linters::find_missing_linter_commands(
                plan.jobs.iter().map(|job| job.linter_name.as_str()),
                &registry,
            );
            super::external_linters::log_missing_linter_commands(&missing);
            let mut diagnostics = Vec::new();
            let mut complete = plan.complete;
            for job in &plan.jobs {
                if registry
                    .get(&job.linter_name)
                    .is_some_and(|tool| missing.contains(tool.command.as_ref()))
                {
                    complete = false;
                    continue;
                }
                let _permit = crate::external_tools_common::acquire_external_tool_permit();
                match super::external_linters_sync::run_linter_job_sync(job, &plan.original_input) {
                    Ok(items) => diagnostics.extend(
                        items
                            .into_iter()
                            .filter_map(|d| project_diagnostic(plan, d)),
                    ),
                    Err(error) => {
                        complete = false;
                        log::warn!("External linter '{}' failed: {error}", job.linter_name);
                    }
                }
            }
            results.push((complete, diagnostics));
        }
        self.merge(results)
    }

    fn merge(&self, results: Vec<(bool, Vec<(usize, Diagnostic)>)>) -> ExecutionResults {
        let mut out = ExecutionResults {
            successful: results.iter().all(|(complete, _)| *complete),
            ..Default::default()
        };
        let visible: BTreeSet<_> = self.scopes.values().flatten().collect();
        for (plan_index, plan) in self.plans.iter().enumerate() {
            for issue in &plan.diagnostics {
                if visible.contains(&issue.path) {
                    let items = out.diagnostics.entry(issue.path.clone()).or_default();
                    if !items.contains(&issue.diagnostic) {
                        items.push(issue.diagnostic.clone());
                    }
                }
            }
            for (occurrence, original) in &results[plan_index].1 {
                let source = &plan.sources[*occurrence];
                if !visible.contains(&source.path) {
                    continue;
                }
                let mut diagnostic = original.clone();
                if let Some(fix) = &diagnostic.fix {
                    let agreed = self.plans.iter().enumerate().all(|(idx, other)| {
                        other
                            .sources
                            .iter()
                            .enumerate()
                            .filter(|(_, s)| s.path == source.path)
                            .all(|(occ, _)| {
                                results[idx].0
                                    && results[idx].1.iter().any(|(candidate_occ, candidate)| {
                                        *candidate_occ == occ
                                            && candidate
                                                .fix
                                                .as_ref()
                                                .is_some_and(|candidate| same_edits(fix, candidate))
                                    })
                            })
                    });
                    if !agreed {
                        diagnostic.fix = None;
                    }
                }
                let items = out.diagnostics.entry(source.path.clone()).or_default();
                let matching = items.iter().position(|d| {
                    d.code == diagnostic.code
                        && d.severity == diagnostic.severity
                        && d.location == diagnostic.location
                        && d.message == diagnostic.message
                });
                let idx = matching.unwrap_or_else(|| {
                    items.push(diagnostic);
                    items.len() - 1
                });
                if plan.root != source.path {
                    let note = super::diagnostics::DiagnosticNote {
                        kind: DiagnosticNoteKind::Note,
                        message: format!("In execution context: {}", plan.root.display()),
                    };
                    if !items[idx].notes.contains(&note) {
                        items[idx].notes.push(note);
                    }
                }
            }
        }
        for items in out.diagnostics.values_mut() {
            items.sort_by(|a, b| {
                (a.location.range.start(), &a.code, &a.message).cmp(&(
                    b.location.range.start(),
                    &b.code,
                    &b.message,
                ))
            });
        }
        out
    }
}

fn same_edits(a: &Fix, b: &Fix) -> bool {
    a.safety == b.safety && a.edits == b.edits
}

fn local_range(range: TextRange, source: &SourceOccurrence) -> Option<TextRange> {
    let start = usize::from(range.start());
    let end = usize::from(range.end());
    if start < source.arena_range.start || end > source.arena_range.end {
        return None;
    }
    let start = start - source.arena_range.start;
    let end = end - source.arena_range.start;
    if !source.text.is_char_boundary(start) || !source.text.is_char_boundary(end) {
        return None;
    }
    Some(TextRange::new(
        TextSize::try_from(start).ok()?,
        TextSize::try_from(end).ok()?,
    ))
}

fn project_diagnostic(
    plan: &ExecutionPlan,
    mut diagnostic: Diagnostic,
) -> Option<(usize, Diagnostic)> {
    let (index, source, range) = plan.sources.iter().enumerate().find_map(|(idx, source)| {
        Some((idx, source, local_range(diagnostic.location.range, source)?))
    })?;
    diagnostic.location = Location::from_range(range, &source.text);
    diagnostic.fix = diagnostic.fix.and_then(|mut fix| {
        for edit in &mut fix.edits {
            edit.range = local_range(edit.range, source)?;
        }
        fix.edits.sort_by_key(|edit| {
            (
                edit.range.start(),
                edit.range.end(),
                edit.replacement.clone(),
            )
        });
        fix.edits.dedup();
        Some(fix)
    });
    Some((index, diagnostic))
}

#[derive(Debug, Clone, Default)]
pub struct ExecutionResults {
    pub diagnostics: BTreeMap<PathBuf, Vec<Diagnostic>>,
    pub successful: bool,
}

/// Host-owned subprocess cache. Salsa only memoizes pure execution plans.
#[derive(Default)]
pub struct ExecutionCache(Mutex<HashMap<String, Arc<OnceLock<Arc<ExecutionResults>>>>>);

impl ExecutionCache {
    /// Refresh tool-owned configuration before collecting a pass's diagnostics.
    pub fn invalidate_project_config(&self, batch: &ExecutionBatch) {
        if batch.uses_project_config() {
            self.0
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(&batch.fingerprint());
        }
    }

    pub fn get_or_run(&self, batch: &ExecutionBatch, run: bool) -> Arc<ExecutionResults> {
        let key = batch.fingerprint();
        let slot = {
            let mut entries = self.0.lock().unwrap_or_else(|error| error.into_inner());
            // Bound retained source histories in long-running language servers.
            if entries.len() >= 64 && !entries.contains_key(&key) {
                entries.clear();
            }
            entries.entry(key.clone()).or_default().clone()
        };
        if !run {
            return slot.get().cloned().unwrap_or_else(|| {
                Arc::new(batch.merge(batch.plans.iter().map(|_| (false, Vec::new())).collect()))
            });
        }
        let result = slot.get_or_init(|| Arc::new(batch.run())).clone();
        if !result.successful {
            self.0
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(&key);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::super::diagnostics::Edit;
    use super::*;
    use crate::config::{Config, Extensions};

    fn config(db: &SalsaDb) -> FileConfig {
        FileConfig::new(
            db,
            Config {
                flavor: Flavor::Quarto,
                extensions: Extensions::for_flavor(Flavor::Quarto),
                linters: HashMap::from([("r".into(), "arity".into())]),
                ..Default::default()
            },
        )
    }

    fn root(db: &mut SalsaDb, config: FileConfig, path: &str, text: &str) -> ExecutionRoot {
        let file = db.update_file_text(PathBuf::from(path), text.to_owned());
        ExecutionRoot {
            file,
            config,
            render_target: false,
        }
    }

    #[test]
    fn include_order_nested_paths_and_repeated_occurrences() {
        let mut db = SalsaDb::default();
        let config = config(&db);
        let parent = root(
            &mut db,
            config,
            "/execution/main.qmd",
            "`r before`\n\n{{< include parts/_one.qmd >}}\n\n`r between`\n\n{{< include parts/_one.qmd >}}\n\n`r after`\n",
        );
        root(
            &mut db,
            config,
            "/execution/parts/_one.qmd",
            "`r first`\n\n{{< include \"_two words.qmd\" >}}\n",
        );
        root(&mut db, config, "/execution/_two words.qmd", "`r second`\n");
        let plan = execution_plan(&db, parent.file, config);
        assert!(plan.complete);
        assert_eq!(plan.sources.len(), 5);
        let job = &plan.jobs[0];
        let mut cursor = 0;
        for word in [
            "before", "first", "second", "between", "first", "second", "after",
        ] {
            cursor += job.content[cursor..].find(word).unwrap() + word.len();
        }
        assert_ne!(plan.sources[1].arena_range, plan.sources[3].arena_range);
    }

    #[test]
    fn quarto_displayed_examples_are_separate_in_both_lint_plans() {
        let mut db = SalsaDb::default();
        let config = config(&db);
        let parent = root(
            &mut db,
            config,
            "/execution/main.qmd",
            "```r\ndisplayed\n```\n\n```{r}\n#| eval: false\ndisabled\n```\n\n```{r}\nactive\n```\n\n`r use`\n",
        );
        let plan = execution_plan(&db, parent.file, config);
        let local = crate::salsa::built_in_lint_plan(&db, parent.file, config);
        for jobs in [&plan.jobs, &local.external_jobs] {
            assert_eq!(jobs.len(), 3);
            assert!(jobs[0].content.contains("active"));
            assert!(jobs[0].content.contains("use"));
            assert!(!jobs[0].content.contains("displayed"));
            assert!(!jobs[0].content.contains("disabled"));
            assert_eq!(jobs[1].content.trim(), "displayed");
            assert_eq!(jobs[2].content.trim(), "disabled");
        }
    }

    #[test]
    fn missing_include_invalidates_when_loaded_and_cycles_stop_jobs() {
        let mut db = SalsaDb::default();
        let config = config(&db);
        let parent = root(
            &mut db,
            config,
            "/execution/main.qmd",
            "{{< include _child.qmd >}}\n\n`r value`\n",
        );
        let missing = execution_plan(&db, parent.file, config);
        assert!(!missing.complete);
        assert!(missing.jobs.is_empty());
        assert_eq!(missing.diagnostics[0].diagnostic.code, "include-not-found");
        root(&mut db, config, "/execution/_child.qmd", "`r binding`\n");
        assert!(execution_plan(&db, parent.file, config).complete);
        root(
            &mut db,
            config,
            "/execution/_child.qmd",
            "{{< include ./main.qmd >}}\n",
        );
        let cycle = execution_plan(&db, parent.file, config);
        assert!(!cycle.complete);
        assert!(cycle.jobs.is_empty());
        assert_eq!(cycle.diagnostics[0].diagnostic.code, "include-cycle");
    }

    #[test]
    fn cyclic_roots_remain_reportable_beside_unrelated_documents() {
        let mut db = SalsaDb::default();
        let config = config(&db);
        let a = root(
            &mut db,
            config,
            "/execution/a.qmd",
            "{{< include b.qmd >}}\n",
        );
        let b = root(
            &mut db,
            config,
            "/execution/b.qmd",
            "{{< include a.qmd >}}\n",
        );
        let unrelated = root(&mut db, config, "/execution/other.qmd", "Text\n");
        let targets = vec![
            PathBuf::from("/execution/a.qmd"),
            PathBuf::from("/execution/other.qmd"),
        ];
        let batch = ExecutionBatch::new(&db, &[a, b, unrelated], &targets);
        assert!(
            batch
                .plans
                .iter()
                .flat_map(|plan| &plan.diagnostics)
                .any(|issue| issue.diagnostic.code == "include-cycle")
        );
    }

    #[test]
    fn escaped_and_verbatim_shortcodes_do_not_load_files() {
        let mut db = SalsaDb::default();
        let config = config(&db);
        let parent = root(
            &mut db,
            config,
            "/execution/main.qmd",
            "{{{< include absent.qmd >}}}\n\n`{{< include absent.qmd >}}`\n",
        );
        let plan = execution_plan(&db, parent.file, config);
        assert!(plan.complete);
        assert_eq!(plan.dependencies.len(), 1);
    }

    #[test]
    fn tool_ranges_in_generated_padding_are_not_attributed_to_source() {
        let mut db = SalsaDb::default();
        let config = config(&db);
        let parent = root(
            &mut db,
            config,
            "/execution/main.qmd",
            "Prose\n\n```{r}\nvalue\n```\n",
        );
        let plan = execution_plan(&db, parent.file, config);
        let job = &plan.jobs[0];
        let outputs = [
            (
                "eslint",
                r#"[{"messages":[{"ruleId":"test","severity":2,"message":"padding","line":1,"column":1,"endLine":1,"endColumn":2}]}]"#,
            ),
            (
                "shellcheck",
                r#"[{"code":1,"level":"warning","message":"padding","line":1,"column":1,"endLine":1,"endColumn":2}]"#,
            ),
            (
                "staticcheck",
                r#"[{"code":"test","location":{"file":"test.go","line":1,"column":1},"message":"padding"}]"#,
            ),
        ];
        for (tool, output) in outputs {
            let diagnostics = super::super::external_linters::parse_linter_output(
                tool,
                output,
                &job.content,
                &plan.original_input,
                Some(&job.mappings),
            )
            .unwrap();
            assert!(diagnostics.is_empty(), "{tool}: {diagnostics:?}");
        }
    }

    #[test]
    fn cross_source_diagnostic_stays_in_its_starting_include() {
        let mut db = SalsaDb::default();
        let config = config(&db);
        let parent = root(
            &mut db,
            config,
            "/execution/main.qmd",
            "{{< include _child.qmd >}}\n\n`r last`\n",
        );
        let text = "> ```{r}\r\n> first()\r\n> ```\r\n";
        root(&mut db, config, "/execution/_child.qmd", text);
        let plan = execution_plan(&db, parent.file, config);
        let job = &plan.jobs[0];
        let start = job.content.find("first()").unwrap();
        let end = job.content.find("last").unwrap() + 4;
        let output = serde_json::json!([{
            "rule": "test", "severity": "Warning",
            "range": {"start": start, "end": end},
            "message": {"name": "test", "body": "Spans included sources"},
            "fix": {"start": start, "end": end, "content": "", "applicability": "safe", "description": "Delete"}
        }]);
        let diagnostics = super::super::external_linters::parse_linter_output(
            "arity",
            &output.to_string(),
            &job.content,
            &plan.original_input,
            Some(&job.mappings),
        )
        .unwrap();
        assert_eq!(diagnostics.len(), 1);
        let (occurrence, diagnostic) =
            project_diagnostic(plan, diagnostics.into_iter().next().unwrap()).unwrap();
        assert_eq!(
            plan.sources[occurrence].path,
            Path::new("/execution/_child.qmd")
        );
        assert_eq!(
            &text[usize::from(diagnostic.location.range.start())
                ..usize::from(diagnostic.location.range.end())],
            "first()"
        );
        assert!(diagnostic.fix.is_none());
    }

    #[test]
    fn mapping_restores_prefixed_crlf_and_unicode_source_and_drops_cross_source_fix() {
        let mut db = SalsaDb::default();
        let config = config(&db);
        let parent = root(
            &mut db,
            config,
            "/execution/main.qmd",
            "{{< include _child.qmd >}}\n\n`r last`\n",
        );
        let text = "> ```{r child}\r\n> value <- 1\r\n> ```\r\n\r\n🦀 `r value`\r\n";
        root(&mut db, config, "/execution/_child.qmd", text);
        let plan = execution_plan(&db, parent.file, config);
        let job = &plan.jobs[0];
        let start = job.content.find("value").unwrap();
        let output = serde_json::json!([{
            "rule": "test", "severity": "Warning", "range": { "start": start, "end": start + 5 },
            "message": {"name": "test", "body": "test"},
            "fix": {"start": start, "end": start + 5, "content": "other", "applicability": "safe", "description": "Rename"}
        }]);
        let raw = super::super::external_linters::parse_linter_output(
            "arity",
            &output.to_string(),
            &job.content,
            &plan.original_input,
            Some(&job.mappings),
        )
        .unwrap()
        .remove(0);
        let (occurrence, diagnostic) = project_diagnostic(plan, raw.clone()).unwrap();
        assert_eq!(
            plan.sources[occurrence].path,
            Path::new("/execution/_child.qmd")
        );
        assert_eq!(
            &text[usize::from(diagnostic.location.range.start())
                ..usize::from(diagnostic.location.range.end())],
            "value"
        );
        assert_eq!(
            (diagnostic.location.line, diagnostic.location.column),
            (2, 3)
        );
        assert_eq!(
            diagnostic.fix.unwrap().edits[0].range.start(),
            TextSize::from(text.find("value").unwrap() as u32)
        );
        let mut raw = raw;
        raw.fix.as_mut().unwrap().edits.push(Edit {
            range: TextRange::empty(0.into()),
            replacement: "bad".into(),
        });
        assert!(project_diagnostic(plan, raw).unwrap().1.fix.is_none());
    }

    fn proposal(plan: &ExecutionPlan, occurrence: usize) -> (usize, Diagnostic) {
        let source = &plan.sources[occurrence];
        let start = source.text.find("value").unwrap();
        let range = TextRange::new((start as u32).into(), ((start + 5) as u32).into());
        (
            occurrence,
            Diagnostic::warning(
                Location::from_range(range, &source.text),
                "unused-binding",
                "unused value",
            )
            .with_fix(Fix::unsafe_fix(
                "Remove",
                vec![Edit {
                    range,
                    replacement: String::new(),
                }],
            )),
        )
    }

    #[test]
    fn shared_fixes_require_all_parent_contexts_and_occurrences() {
        let mut db = SalsaDb::default();
        let config = config(&db);
        let a = root(
            &mut db,
            config,
            "/execution/a.qmd",
            "{{< include _child.qmd >}}\n",
        );
        let b = root(
            &mut db,
            config,
            "/execution/b.qmd",
            "{{< include _child.qmd >}}\n\n{{< include _child.qmd >}}\n",
        );
        let child = root(&mut db, config, "/execution/_child.qmd", "`r value`\n");
        let path = PathBuf::from("/execution/_child.qmd");
        let batch = ExecutionBatch::new(&db, &[a, b, child], std::slice::from_ref(&path));
        assert_eq!(batch.plans.len(), 2);
        let a_fix = proposal(&batch.plans[0], 1);
        let b_fix = proposal(&batch.plans[1], 1);
        let b_repeat = proposal(&batch.plans[1], 2);
        let partial = batch.merge(vec![(true, vec![a_fix.clone()]), (true, vec![])]);
        assert!(partial.diagnostics[&path][0].fix.is_none());
        assert_eq!(partial.diagnostics[&path][0].notes.len(), 1);
        let missing_occurrence = batch.merge(vec![
            (true, vec![a_fix.clone()]),
            (true, vec![b_fix.clone()]),
        ]);
        assert!(missing_occurrence.diagnostics[&path][0].fix.is_none());
        let agreed = batch.merge(vec![(true, vec![a_fix]), (true, vec![b_fix, b_repeat])]);
        assert_eq!(agreed.diagnostics[&path].len(), 1);
        assert!(agreed.diagnostics[&path][0].fix.is_some());
        assert_eq!(agreed.diagnostics[&path][0].notes.len(), 2);
    }
}
