use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use wincode::{SchemaRead, SchemaWrite};

const CACHE_SCHEMA_VERSION: u32 = 1;
const CACHE_FILE_NAME: &str = "cli-cache-v1.bin";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatCacheMode {
    Check,
    Write,
}

#[derive(Debug, Clone, SchemaWrite, SchemaRead, PartialEq, Eq)]
pub struct CachedLintDocument {
    pub path: String,
    pub input: String,
    pub diagnostics: Vec<CachedDiagnostic>,
}

#[derive(Debug, Clone, SchemaWrite, SchemaRead, PartialEq, Eq)]
pub struct CachedDiagnostic {
    pub severity: CachedSeverity,
    pub location: CachedLocation,
    pub message: String,
    pub code: String,
    pub origin: CachedDiagnosticOrigin,
    pub notes: Vec<CachedDiagnosticNote>,
    pub fix: Option<CachedFix>,
}

#[derive(Debug, Clone, SchemaWrite, SchemaRead, PartialEq, Eq)]
pub enum CachedSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, SchemaWrite, SchemaRead, PartialEq, Eq)]
pub enum CachedDiagnosticOrigin {
    BuiltIn,
    External,
}

#[derive(Debug, Clone, SchemaWrite, SchemaRead, PartialEq, Eq)]
pub enum CachedDiagnosticNoteKind {
    Note,
    Help,
}

#[derive(Debug, Clone, SchemaWrite, SchemaRead, PartialEq, Eq)]
pub struct CachedLocation {
    pub line: usize,
    pub column: usize,
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, SchemaWrite, SchemaRead, PartialEq, Eq)]
pub struct CachedEdit {
    pub start: u32,
    pub end: u32,
    pub replacement: String,
}

#[derive(Debug, Clone, SchemaWrite, SchemaRead, PartialEq, Eq)]
pub struct CachedFix {
    pub message: String,
    pub edits: Vec<CachedEdit>,
}

#[derive(Debug, Clone, SchemaWrite, SchemaRead, PartialEq, Eq)]
pub struct CachedDiagnosticNote {
    pub kind: CachedDiagnosticNoteKind,
    pub message: String,
}

#[derive(Debug, Clone, SchemaWrite, SchemaRead)]
struct PersistentCache {
    schema_version: u32,
    lint: HashMap<String, CachedLintEntry>,
    format: HashMap<String, CachedFormatEntry>,
}

impl Default for PersistentCache {
    fn default() -> Self {
        Self {
            schema_version: CACHE_SCHEMA_VERSION,
            lint: HashMap::new(),
            format: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, SchemaWrite, SchemaRead)]
struct CachedLintEntry {
    file_fingerprint: String,
    config_fingerprint: String,
    tool_fingerprint: String,
    root_file: String,
    documents: Vec<CachedLintDocument>,
}

#[derive(Debug, Clone, SchemaWrite, SchemaRead)]
struct CachedFormatEntry {
    file_fingerprint: String,
    config_fingerprint: String,
    tool_fingerprint: String,
    mode: String,
    unchanged: bool,
    output: String,
}

pub struct FormatStoreArgs {
    pub file_fingerprint: String,
    pub config_fingerprint: String,
    pub tool_fingerprint: String,
    pub unchanged: bool,
    pub output: String,
}

pub struct CliCache {
    path: PathBuf,
    state: PersistentCache,
    dirty: bool,
}

impl CliCache {
    pub fn open(
        cfg: &panache::Config,
        explicit_config: Option<&Path>,
        start_dir: &Path,
    ) -> io::Result<Option<Self>> {
        let cache_dir = resolve_cache_dir(cfg, explicit_config, start_dir)?;
        fs::create_dir_all(&cache_dir)?;
        let cache_path = cache_dir.join(CACHE_FILE_NAME);

        let state = match fs::read(&cache_path) {
            Ok(raw) => match wincode::deserialize_exact::<PersistentCache>(&raw) {
                Ok(state) if state.schema_version == CACHE_SCHEMA_VERSION => state,
                Ok(_) => PersistentCache::default(),
                Err(err) => {
                    log::warn!(
                        "Ignoring unreadable cache at {}: {}",
                        cache_path.display(),
                        err
                    );
                    PersistentCache::default()
                }
            },
            Err(err) if err.kind() == io::ErrorKind::NotFound => PersistentCache::default(),
            Err(err) => return Err(err),
        };

        Ok(Some(Self {
            path: cache_path,
            state,
            dirty: false,
        }))
    }

    pub fn save_if_dirty(&mut self) -> io::Result<()> {
        if !self.dirty {
            return Ok(());
        }
        let raw = wincode::serialize(&self.state).map_err(io::Error::other)?;
        let tmp_path = self.path.with_extension(format!(
            "bin.tmp.{}.{}",
            std::process::id(),
            unique_suffix()
        ));
        fs::write(&tmp_path, raw)?;
        fs::rename(tmp_path, &self.path)?;
        self.dirty = false;
        Ok(())
    }

    pub fn supports_lint(&self, _cfg: &panache::Config) -> bool {
        true
    }

    pub fn supports_format_mode(&self, _cfg: &panache::Config, _mode: FormatCacheMode) -> bool {
        true
    }

    pub fn file_fingerprint(input: &str) -> String {
        format!("{:x}", stable_hash(input))
    }

    pub fn config_fingerprint(cfg: &panache::Config) -> String {
        format!("{:x}", stable_hash(&format!("{cfg:?}")))
    }

    pub fn tool_fingerprint() -> String {
        format!("panache@{}", env!("CARGO_PKG_VERSION"))
    }

    pub fn get_lint(
        &self,
        root_file: &Path,
        file_fingerprint: &str,
        config_fingerprint: &str,
        tool_fingerprint: &str,
    ) -> Option<Vec<CachedLintDocument>> {
        let key = root_file.to_string_lossy().to_string();
        let entry = self.state.lint.get(&key)?;
        if entry.file_fingerprint != file_fingerprint
            || entry.config_fingerprint != config_fingerprint
            || entry.tool_fingerprint != tool_fingerprint
        {
            return None;
        }
        Some(entry.documents.clone())
    }

    pub fn put_lint(
        &mut self,
        root_file: &Path,
        file_fingerprint: String,
        config_fingerprint: String,
        tool_fingerprint: String,
        documents: Vec<CachedLintDocument>,
    ) {
        let key = root_file.to_string_lossy().to_string();
        self.state.lint.insert(
            key.clone(),
            CachedLintEntry {
                file_fingerprint,
                config_fingerprint,
                tool_fingerprint,
                root_file: key,
                documents,
            },
        );
        self.dirty = true;
    }

    pub fn get_format(
        &self,
        file_path: &Path,
        mode: FormatCacheMode,
        file_fingerprint: &str,
        config_fingerprint: &str,
        tool_fingerprint: &str,
    ) -> Option<(bool, String)> {
        let key = file_path.to_string_lossy().to_string();
        let entry = self.state.format.get(&key)?;
        if entry.mode != mode_to_str(mode)
            || entry.file_fingerprint != file_fingerprint
            || entry.config_fingerprint != config_fingerprint
            || entry.tool_fingerprint != tool_fingerprint
        {
            return None;
        }
        Some((entry.unchanged, entry.output.clone()))
    }

    pub fn put_format(&mut self, file_path: &Path, mode: FormatCacheMode, args: FormatStoreArgs) {
        let key = file_path.to_string_lossy().to_string();
        self.state.format.insert(
            key,
            CachedFormatEntry {
                file_fingerprint: args.file_fingerprint,
                config_fingerprint: args.config_fingerprint,
                tool_fingerprint: args.tool_fingerprint,
                mode: mode_to_str(mode).to_string(),
                unchanged: args.unchanged,
                output: args.output,
            },
        );
        self.dirty = true;
    }
}

fn mode_to_str(mode: FormatCacheMode) -> &'static str {
    match mode {
        FormatCacheMode::Check => "check",
        FormatCacheMode::Write => "write",
    }
}

fn resolve_cache_dir(
    cfg: &panache::Config,
    explicit_config: Option<&Path>,
    start_dir: &Path,
) -> io::Result<PathBuf> {
    if let Some(dir) = &cfg.cache_dir {
        let candidate = PathBuf::from(dir);
        if candidate.is_absolute() {
            return Ok(candidate);
        }
        return Ok(start_dir.join(candidate));
    }

    if let Some(path) = explicit_config
        && let Some(parent) = path.parent()
    {
        return Ok(parent.join(".panache-cache"));
    }

    let cwd = std::env::current_dir()?;
    Ok(cwd.join(".panache-cache"))
}

fn stable_hash(value: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn unique_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |dur| dur.as_nanos() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache_enabled_config(cache_dir: &Path) -> panache::Config {
        panache::Config {
            cache_dir: Some(cache_dir.to_string_lossy().to_string()),
            ..panache::Config::default()
        }
    }

    #[test]
    fn lint_entry_round_trips() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cfg = cache_enabled_config(tmp.path());
        let mut cache = CliCache::open(&cfg, None, tmp.path())
            .expect("open cache")
            .expect("cache enabled");

        let root = tmp.path().join("doc.qmd");
        let docs = vec![CachedLintDocument {
            path: root.to_string_lossy().to_string(),
            input: "# Title\n".to_string(),
            diagnostics: vec![],
        }];
        cache.put_lint(
            &root,
            "file-hash".to_string(),
            "cfg-hash".to_string(),
            "tool-hash".to_string(),
            docs.clone(),
        );
        cache.save_if_dirty().expect("save");

        let cache = CliCache::open(&cfg, None, tmp.path())
            .expect("open cache")
            .expect("cache enabled");
        let got = cache
            .get_lint(&root, "file-hash", "cfg-hash", "tool-hash")
            .expect("lint cache hit");
        assert_eq!(got, docs);
    }

    #[test]
    fn format_entry_miss_on_mode_mismatch() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cfg = cache_enabled_config(tmp.path());
        let mut cache = CliCache::open(&cfg, None, tmp.path())
            .expect("open cache")
            .expect("cache enabled");

        let path = tmp.path().join("doc.md");
        cache.put_format(
            &path,
            FormatCacheMode::Check,
            FormatStoreArgs {
                file_fingerprint: "file".to_string(),
                config_fingerprint: "cfg".to_string(),
                tool_fingerprint: "tool".to_string(),
                unchanged: true,
                output: "same".to_string(),
            },
        );

        assert!(
            cache
                .get_format(&path, FormatCacheMode::Write, "file", "cfg", "tool")
                .is_none()
        );
        assert!(
            cache
                .get_format(&path, FormatCacheMode::Check, "file", "cfg", "tool")
                .is_some()
        );
    }
}
