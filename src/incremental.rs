//! The incremental-reparse side channel that sits beside salsa.
//!
//! [`crate::salsa::parsed_document`] is a pure function of a document's text
//! and config, and it stays one: a successful reparse is byte-identical to a
//! full parse of the same text (the governing invariant, asserted by the
//! parser crate's debug oracle on every splice). This cache therefore only
//! changes how *fast* the query computes, never what it returns -- which is
//! what makes reading and writing it from inside an otherwise-pure tracked
//! query sound.
//!
//! Membership is the admission gate. Only the LSP admits a document, and only
//! while incremental parsing is enabled; every other parse of the same file
//! (the CLI, a project-graph sweep, a sibling config) finds no entry, stores
//! nothing, and full-parses exactly as before. That is why "the workspace is
//! green with the flag off" is a meaningful statement about this module: with
//! nothing admitted, none of it runs.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use crate::config::Config;
use crate::parser::{Edit, RefdefMap, SyntaxError};
use crate::salsa::{FileConfig, FileText};

/// A base is keyed on `(file, config)`, not on the file alone: project queries
/// parse the same file under other configs, and their result must never become
/// the open document's splice base.
pub type ReparseKey = (FileText, FileConfig);

/// The previous parse of a `(file, config)` pair, kept outside salsa as the
/// incremental-reparse base.
///
/// `refdefs` and `config` are reuse *keys*, not payload: retained blocks keep
/// the reference resolution and the parser options they were parsed with, so a
/// base recorded under a different set or a different config cannot be spliced
/// against.
#[derive(Debug, Clone)]
pub struct PrevParse {
    /// Shares salsa's `Arc`, so the base costs no second copy of the text.
    pub text: Arc<str>,
    pub green: rowan::GreenNode,
    pub errors: Vec<SyntaxError>,
    pub refdefs: RefdefMap,
    pub config: Config,
}

/// What the side channel has to say about a `(file, config)` pair.
pub enum ReparseAdmission {
    /// Not admitted: full-parse and store nothing.
    Refused,
    /// Admitted, carrying the base to splice against (`None` before the first
    /// parse, or after an eviction).
    Admitted {
        prev: Option<Arc<PrevParse>>,
        edit: Option<SuppliedEdit>,
    },
}

#[derive(Clone)]
pub struct SuppliedEdit {
    pub target: Arc<str>,
    pub span: EditSpan,
}

impl SuppliedEdit {
    pub fn materialize(&self) -> Option<Edit> {
        Some(Edit {
            range: self.span.old.clone(),
            insert: self.target.get(self.span.new.clone())?.to_owned(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditSpan {
    pub old: Range<usize>,
    pub new: Range<usize>,
}

#[derive(Debug, Clone)]
struct StagedEdit {
    source: Arc<str>,
    target: Arc<str>,
    span: EditSpan,
}

#[derive(Debug, Default)]
struct FileReparseState {
    prev: Option<Arc<PrevParse>>,
    staged: Option<StagedEdit>,
    /// When this entry was last touched, for eviction. See [`ReparseCache`].
    used: u64,
}

/// How many `(file, config)` pairs keep a reparse base. Admission already
/// bounds this to open documents, so the cap is a backstop against a client
/// that opens documents without ever closing them, not a working limit.
const MAX_REPARSE_BASES: usize = 64;

/// The reparse side channel for every admitted document.
///
/// Presence in `files` *is* admission: [`ReparseCache::admit`] is the only way
/// an entry appears, and [`ReparseCache::store`] no-ops on a key it does not
/// find. Keeping one map rather than a map plus an admitted-set means the two
/// cannot drift apart.
///
/// Eviction is least-recently-used, approximated by a monotone counter stamped
/// on each entry as it is read or written. Dropping an entry only costs its
/// document a full parse, so the policy needs no more precision than that.
///
/// No `Debug`: salsa only formats an input with its database in hand, and
/// [`crate::salsa::SalsaDb`] elides this cache from its own `Debug` anyway.
#[derive(Default)]
pub struct ReparseCache {
    files: HashMap<ReparseKey, FileReparseState>,
    clock: u64,
}

impl ReparseCache {
    /// The entry for `key` if it is admitted, stamped as just used.
    fn touch(&mut self, key: ReparseKey) -> Option<&mut FileReparseState> {
        self.clock += 1;
        let clock = self.clock;
        let state = self.files.get_mut(&key)?;
        state.used = clock;
        Some(state)
    }

    /// The side channel's view of `key`, stamping it as used.
    ///
    /// Read through `touch` so a document the editor keeps hitting stays recent
    /// even across a long run of parses of other documents.
    pub fn base(&mut self, key: ReparseKey) -> ReparseAdmission {
        match self.touch(key) {
            Some(state) => ReparseAdmission::Admitted {
                prev: state.prev.clone(),
                edit: state.staged.as_ref().map(|staged| SuppliedEdit {
                    target: Arc::clone(&staged.target),
                    span: staged.span.clone(),
                }),
            },
            None => ReparseAdmission::Refused,
        }
    }

    /// Record `prev` as the base for `key`. Silently does nothing when `key` is
    /// not admitted.
    pub fn store(&mut self, key: ReparseKey, prev: PrevParse) {
        let Some(state) = self.touch(key) else {
            return;
        };
        if let Some(staged) = &state.staged {
            if !Arc::ptr_eq(&staged.target, &prev.text) {
                return;
            }
            state.staged = None;
        }
        state.prev = Some(Arc::new(prev));
        self.evict_over_budget();
    }

    /// Stage an LSP edit against the exact text allocation it transformed.
    pub fn stage(&mut self, key: ReparseKey, source: Arc<str>, target: Arc<str>, span: EditSpan) {
        let Some(state) = self.touch(key) else {
            return;
        };
        let Some(prev) = state.prev.as_ref() else {
            state.staged = None;
            return;
        };

        debug_assert_eq!(
            Edit {
                range: span.old.clone(),
                insert: target[span.new.clone()].to_owned(),
            }
            .apply(&source),
            &*target
        );
        let staged = if Arc::ptr_eq(&prev.text, &source) {
            Some(StagedEdit {
                source,
                target,
                span,
            })
        } else if let Some(prior) = state.staged.take()
            && Arc::ptr_eq(&prior.source, &prev.text)
            && Arc::ptr_eq(&prior.target, &source)
        {
            compose_spans(&prior.span, &span, source.len(), target.len()).map(|span| StagedEdit {
                source: prior.source,
                target,
                span,
            })
        } else {
            None
        };
        state.staged = staged;
    }

    /// Forget staged edit metadata for `file` after an unstaged text write.
    pub fn invalidate_file_edits(&mut self, file: FileText) {
        for ((entry, _), state) in &mut self.files {
            if *entry == file {
                state.staged = None;
            }
        }
    }

    /// Admit `key`, so parses of it start keeping a base.
    ///
    /// Drops any entry for the same file under a *different* config: a config
    /// reload re-points the document at a new [`FileConfig`] handle, and the
    /// base recorded under the old one can never be hit again.
    pub fn admit(&mut self, key: ReparseKey) {
        self.files
            .retain(|(file, config), _| *file != key.0 || *config == key.1);
        self.clock += 1;
        let clock = self.clock;
        self.files.entry(key).or_default().used = clock;
        self.evict_over_budget();
    }

    /// Forget every base for `file`, whatever config it was parsed under.
    /// Called when a document closes.
    pub fn retire_file(&mut self, file: FileText) {
        self.files.retain(|(entry, _), _| *entry != file);
    }

    /// Forget everything. Called when incremental parsing is switched off, so
    /// the flag-off path is not merely unused but empty.
    pub fn clear(&mut self) {
        self.files.clear();
    }

    /// Drop the least recently used entries until the cache is within budget.
    /// Stamps are unique -- every touch bumps a monotone clock -- so
    /// "everything at or below the `n`th smallest" drops exactly `n` entries.
    fn evict_over_budget(&mut self) {
        if self.files.len() <= MAX_REPARSE_BASES {
            return;
        }
        let over = self.files.len() - MAX_REPARSE_BASES;
        let mut stamps: Vec<u64> = self.files.values().map(|state| state.used).collect();
        stamps.select_nth_unstable(over - 1);
        let threshold = stamps[over - 1];
        self.files.retain(|_, state| state.used > threshold);
    }
}

/// Compose two sequential edits without comparing their surrounding texts.
pub(crate) fn compose_spans(
    first: &EditSpan,
    second: &EditSpan,
    middle_len: usize,
    final_len: usize,
) -> Option<EditSpan> {
    if first.old.start != first.new.start
        || second.old.start != second.new.start
        || first.new.end > middle_len
        || second.old.end > middle_len
    {
        return None;
    }
    let delta = first.new.len() as isize - first.old.len() as isize;
    let old_len = middle_len.checked_add_signed(-delta)?;
    if first.old.end > old_len {
        return None;
    }

    let map_start = |offset: usize| {
        if offset <= first.old.start {
            Some(offset)
        } else if offset >= first.new.end {
            offset.checked_add_signed(-delta)
        } else {
            Some(first.old.start)
        }
    };
    let map_end = |offset: usize| {
        if offset < first.old.start {
            Some(offset)
        } else if offset >= first.new.end {
            offset.checked_add_signed(-delta)
        } else {
            Some(first.old.end)
        }
    };

    let start = first.old.start.min(map_start(second.old.start)?);
    let end = first.old.end.max(map_end(second.old.end)?);
    let insert_len = final_len.checked_sub(old_len.checked_sub(end - start)?)?;
    let insert_end = start.checked_add(insert_len)?;
    (insert_end <= final_len).then_some(EditSpan {
        old: start..end,
        new: start..insert_end,
    })
}

/// Host-level oracle: an incrementally reused parse must equal a full parse of
/// the same text under the same config and a freshly scanned refdef set, tree
/// and errors.
///
/// The parser crate already asserts this on every splice in debug builds; this
/// second layer runs where the reuse *keys* live, so a base recorded under a
/// stale config or refdef set is caught too -- and it runs before the result is
/// stored, so a divergent splice can never become the next keystroke's base.
///
/// Off by default even in debug builds (it costs a full parse per reuse, which
/// defeats the point of the feature); set `PANACHE_REPARSE_ORACLE=1` to arm it
/// for a test run or a dogfooding session. Compiled out of release builds.
#[cfg(debug_assertions)]
pub fn assert_reuse_matches_full_parse(
    reused: &crate::salsa::ParsedDocument,
    text: &str,
    config: &Config,
    refdefs: &RefdefMap,
) {
    use std::sync::OnceLock;

    static ARMED: OnceLock<bool> = OnceLock::new();
    if !*ARMED.get_or_init(|| std::env::var("PANACHE_REPARSE_ORACLE").as_deref() == Ok("1")) {
        return;
    }

    let fresh_refdefs =
        crate::parser::collect_refdef_labels_with_options(text, &config.parser_options());
    assert_eq!(
        refdefs, &fresh_refdefs,
        "reused parse carried a stale reference-definition set",
    );
    let (full, full_errors) =
        crate::parser::parse_with_refdefs_and_errors(text, Some(config.clone()), fresh_refdefs);
    let reused_root = crate::syntax::SyntaxNode::new_root(reused.green.clone());
    assert_eq!(
        panache_parser::parser::fingerprint(&reused_root),
        panache_parser::parser::fingerprint(&full),
        "reused parse diverged from a full parse of the same text",
    );
    assert_eq!(
        reused.errors, full_errors,
        "reused parse diverged from a full parse on syntax errors",
    );
}

#[cfg(not(debug_assertions))]
#[inline]
pub fn assert_reuse_matches_full_parse(
    _reused: &crate::salsa::ParsedDocument,
    _text: &str,
    _config: &Config,
    _refdefs: &RefdefMap,
) {
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::salsa::SalsaDb;

    fn base(text: &str) -> PrevParse {
        let tree = crate::parser::parse(text, None);
        PrevParse {
            text: Arc::from(text),
            green: tree.green().to_owned(),
            errors: Vec::new(),
            refdefs: crate::parser::collect_refdef_labels_with_options(
                text,
                &Config::default().parser_options(),
            ),
            config: Config::default(),
        }
    }

    fn key(db: &SalsaDb, text: &str) -> ReparseKey {
        (
            FileText::from_str(db, text),
            FileConfig::new(db, Config::default()),
        )
    }

    #[test]
    fn a_key_that_was_never_admitted_is_refused_and_stores_nothing() {
        let db = SalsaDb::default();
        let mut cache = ReparseCache::default();
        let k = key(&db, "# Title\n");

        assert!(matches!(cache.base(k), ReparseAdmission::Refused));
        cache.store(k, base("# Title\n"));
        assert!(matches!(cache.base(k), ReparseAdmission::Refused));
    }

    #[test]
    fn admission_enables_storing_and_reading_a_base() {
        let db = SalsaDb::default();
        let mut cache = ReparseCache::default();
        let k = key(&db, "# Title\n");

        cache.admit(k);
        assert!(matches!(
            cache.base(k),
            ReparseAdmission::Admitted {
                prev: None,
                edit: None
            }
        ));

        cache.store(k, base("# Title\n"));
        let ReparseAdmission::Admitted {
            prev: Some(prev), ..
        } = cache.base(k)
        else {
            panic!("a stored base must come back");
        };
        assert_eq!(&*prev.text, "# Title\n");
    }

    #[test]
    fn admitting_a_new_config_drops_the_same_file_under_the_old_one() {
        let db = SalsaDb::default();
        let mut cache = ReparseCache::default();
        let file = FileText::from_str(&db, "# Title\n");
        let old_config = FileConfig::new(&db, Config::default());
        let new_config = FileConfig::new(&db, Config::default());

        cache.admit((file, old_config));
        cache.store((file, old_config), base("# Title\n"));
        cache.admit((file, new_config));

        assert!(matches!(
            cache.base((file, old_config)),
            ReparseAdmission::Refused
        ));
        assert!(matches!(
            cache.base((file, new_config)),
            ReparseAdmission::Admitted { prev: None, .. }
        ));
    }

    #[test]
    fn retiring_a_file_forgets_every_config_it_was_parsed_under() {
        let db = SalsaDb::default();
        let mut cache = ReparseCache::default();
        let file = FileText::from_str(&db, "# Title\n");
        let other = FileText::from_str(&db, "# Other\n");
        let config = FileConfig::new(&db, Config::default());

        cache.admit((file, config));
        cache.admit((other, config));
        cache.retire_file(file);

        assert!(matches!(
            cache.base((file, config)),
            ReparseAdmission::Refused
        ));
        assert!(matches!(
            cache.base((other, config)),
            ReparseAdmission::Admitted { prev: None, .. }
        ));
    }

    #[test]
    fn eviction_keeps_the_budget_and_spares_the_most_recently_used() {
        let db = SalsaDb::default();
        let mut cache = ReparseCache::default();
        let config = FileConfig::new(&db, Config::default());
        let keys: Vec<ReparseKey> = (0..MAX_REPARSE_BASES + 8)
            .map(|index| (FileText::from_str(&db, format!("# {index}\n")), config))
            .collect();

        for key in &keys {
            cache.admit(*key);
        }
        // Admission alone is enough to overflow; the hot key is the last one
        // touched, so it must survive.
        assert!(cache.files.len() <= MAX_REPARSE_BASES);
        assert!(matches!(
            cache.base(*keys.last().unwrap()),
            ReparseAdmission::Admitted { .. }
        ));
    }

    #[test]
    fn clearing_empties_the_channel() {
        let db = SalsaDb::default();
        let mut cache = ReparseCache::default();
        let k = key(&db, "# Title\n");
        cache.admit(k);
        cache.store(k, base("# Title\n"));

        cache.clear();

        assert!(matches!(cache.base(k), ReparseAdmission::Refused));
    }

    fn assert_composes(old: &str, first: Edit, second: Edit) {
        let middle = first.apply(old);
        let final_text = second.apply(&middle);
        let first_span = EditSpan {
            old: first.range.clone(),
            new: first.range.start..first.range.start + first.insert.len(),
        };
        let second_span = EditSpan {
            old: second.range.clone(),
            new: second.range.start..second.range.start + second.insert.len(),
        };
        let composed = compose_spans(&first_span, &second_span, middle.len(), final_text.len())
            .expect("valid edits must compose");
        let edit = Edit {
            range: composed.old,
            insert: final_text[composed.new].to_owned(),
        };
        assert_eq!(edit.apply(old), final_text);
    }

    #[test]
    fn sequential_edits_compose_without_scanning_surrounding_text() {
        assert_composes(
            "alpha beta gamma",
            Edit {
                range: 6..10,
                insert: "BETA".to_owned(),
            },
            Edit {
                range: 0..5,
                insert: "ALPHA".to_owned(),
            },
        );
        assert_composes(
            "alpha beta gamma",
            Edit {
                range: 6..10,
                insert: "long middle".to_owned(),
            },
            Edit {
                range: 11..17,
                insert: "center".to_owned(),
            },
        );
        assert_composes(
            "alpha beta gamma",
            Edit {
                range: 6..10,
                insert: String::new(),
            },
            Edit {
                range: 6..6,
                insert: "new ".to_owned(),
            },
        );
        assert_composes(
            "café and tea",
            Edit {
                range: 0..5,
                insert: "茶".to_owned(),
            },
            Edit {
                range: 4..7,
                insert: "&".to_owned(),
            },
        );
    }

    #[test]
    fn staged_notifications_compose_against_the_stored_base() {
        let db = SalsaDb::default();
        let mut cache = ReparseCache::default();
        let k = key(&db, "alpha beta gamma");
        cache.admit(k);
        cache.store(k, base("alpha beta gamma"));
        let ReparseAdmission::Admitted {
            prev: Some(prev), ..
        } = cache.base(k)
        else {
            panic!("stored base");
        };

        let middle: Arc<str> = Arc::from("alpha BETA gamma");
        cache.stage(
            k,
            Arc::clone(&prev.text),
            Arc::clone(&middle),
            EditSpan {
                old: 6..10,
                new: 6..10,
            },
        );
        let target: Arc<str> = Arc::from("ALPHA BETA gamma");
        cache.stage(
            k,
            middle,
            Arc::clone(&target),
            EditSpan {
                old: 0..5,
                new: 0..5,
            },
        );

        let ReparseAdmission::Admitted {
            edit: Some(staged), ..
        } = cache.base(k)
        else {
            panic!("composed edit");
        };
        assert!(Arc::ptr_eq(&staged.target, &target));
        assert_eq!(staged.materialize().unwrap().apply(&prev.text), &*target);
    }

    #[test]
    fn an_older_parse_cannot_overwrite_a_staged_transition() {
        let db = SalsaDb::default();
        let mut cache = ReparseCache::default();
        let k = key(&db, "before");
        cache.admit(k);
        cache.store(k, base("before"));
        let ReparseAdmission::Admitted {
            prev: Some(prev), ..
        } = cache.base(k)
        else {
            panic!("stored base");
        };
        let target: Arc<str> = Arc::from("after");
        cache.stage(
            k,
            Arc::clone(&prev.text),
            Arc::clone(&target),
            EditSpan {
                old: 0..6,
                new: 0..5,
            },
        );

        cache.store(k, base("stale"));
        let ReparseAdmission::Admitted {
            prev: Some(still_prev),
            edit: Some(staged),
        } = cache.base(k)
        else {
            panic!("staged transition must survive");
        };
        assert!(Arc::ptr_eq(&still_prev.text, &prev.text));
        assert!(Arc::ptr_eq(&staged.target, &target));
    }
}
