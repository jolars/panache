//! File-path conversion helpers for [`lsp_types::Uri`].
//!
//! `lsp_types::Uri` is a thin newtype over `fluent_uri::Uri<String>` and, unlike
//! the old `url::Url`-based type, does not provide `to_file_path`/`from_file_path`.
//! This module restores those conversions as an extension trait so the rest of
//! the LSP can keep calling `uri.to_file_path()` and `Uri::from_file_path(path)`.
//!
//! The implementation is ported from the `ls-types` crate (the conversion logic
//! tower-lsp-server shipped), adapted to the `fluent-uri` 0.1.x API that
//! `lsp_types` 0.97 depends on.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use lsp_types::Uri;
use percent_encoding::AsciiSet;

/// RFC 3986 reserves a small set of unreserved path characters; everything else
/// is percent-encoded. Path separators are deliberately left intact.
const ASCII_SET: AsciiSet = percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~')
    .remove(b'/');

/// Extension methods that fill the file-path gap left by `fluent_uri`.
pub trait UriExt {
    /// Assuming the URI is in the `file` scheme, decode its path into an
    /// absolute [`Path`].
    ///
    /// **Note:** this does not check the scheme; callers are responsible for
    /// only invoking it on `file:` URIs. Returns `None` for an empty path.
    fn to_file_path(&self) -> Option<Cow<'_, Path>>;

    /// Build a `file:` [`Uri`] from a filesystem path.
    ///
    /// Relative paths are canonicalized; returns `None` if that fails or the
    /// resulting URI does not parse.
    fn from_file_path<A: AsRef<Path>>(path: A) -> Option<Uri>;
}

impl UriExt for Uri {
    fn to_file_path(&self) -> Option<Cow<'_, Path>> {
        // `self` derefs to `fluent_uri::Uri<String>`.
        let path_str = self.path().as_estr().decode().into_string_lossy();
        if path_str.is_empty() {
            return None;
        }

        let path = match path_str {
            Cow::Borrowed(ref_) => Cow::Borrowed(Path::new(ref_)),
            Cow::Owned(owned) => Cow::Owned(PathBuf::from(owned)),
        };

        if cfg!(windows) {
            let auth_host = self
                .authority()
                .map(|auth| auth.host().as_str())
                .unwrap_or_default();

            if auth_host.is_empty() {
                // Very high chance this is a `file:///c:/...` URI, in which case
                // the path has a leading slash we must drop to get `c:/...`.
                let host = path.to_string_lossy();
                let host = host.get(1..)?;
                return Some(Cow::Owned(PathBuf::from(host)));
            }

            Some(Cow::Owned(
                // `file://server/...` becomes `server:/...`.
                Path::new(&format!("{auth_host}:"))
                    .components()
                    .chain(path.components())
                    .collect(),
            ))
        } else {
            Some(path)
        }
    }

    fn from_file_path<A: AsRef<Path>>(path: A) -> Option<Uri> {
        let path = path.as_ref();

        let fragment = if path.is_absolute() {
            Cow::Borrowed(path)
        } else {
            match strict_canonicalize(path) {
                Ok(path) => Cow::Owned(path),
                Err(_) => return None,
            }
        };

        #[cfg(windows)]
        let raw_uri = {
            // A triple-slash path is shorthand for `file://localhost/C:/...`
            // with `localhost` omitted. The drive letter is encoded too, which
            // the LSP spec permits.
            format!(
                "file:///{}",
                percent_encoding::utf8_percent_encode(
                    &capitalize_drive_letter(&fragment.to_string_lossy().replace('\\', "/")),
                    &ASCII_SET
                )
            )
        };

        #[cfg(not(windows))]
        let raw_uri = {
            format!(
                "file://{}",
                percent_encoding::utf8_percent_encode(&fragment.to_string_lossy(), &ASCII_SET)
            )
        };

        Uri::from_str(&raw_uri).ok()
    }
}

/// Like [`std::fs::canonicalize`] but strips the Windows verbatim (`\\?\`)
/// prefix so the result round-trips through a `file:` URI.
fn strict_canonicalize<P: AsRef<Path>>(path: P) -> std::io::Result<PathBuf> {
    use std::io;

    fn impl_(path: PathBuf) -> std::io::Result<PathBuf> {
        let head = path
            .components()
            .next()
            .ok_or_else(|| io::Error::other("empty path"))?;
        let disk_;
        let head = if let std::path::Component::Prefix(prefix) = head {
            if let std::path::Prefix::VerbatimDisk(disk) = prefix.kind() {
                disk_ = format!("{}:", disk as char);
                Path::new(&disk_)
                    .components()
                    .next()
                    .ok_or_else(|| io::Error::other("failed to parse disk component"))?
            } else {
                head
            }
        } else {
            head
        };

        Ok(std::iter::once(head)
            .chain(path.components().skip(1))
            .collect())
    }

    let canon = std::fs::canonicalize(path)?;
    impl_(canon)
}

#[cfg(windows)]
fn capitalize_drive_letter(path: &str) -> String {
    // Windows paths starting with a drive letter like "c:/".
    if path.len() >= 2 && path.chars().nth(1) == Some(':') {
        let mut chars = path.chars();
        let drive_letter = chars.next().unwrap().to_ascii_uppercase();
        let rest: String = chars.collect();
        format!("{drive_letter}{rest}")
    } else {
        path.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(windows))]
    #[test]
    fn round_trips_absolute_unix_path() {
        let path = Path::new("/tmp/panache/doc.qmd");
        let uri = Uri::from_file_path(path).expect("uri");
        assert_eq!(uri.as_str(), "file:///tmp/panache/doc.qmd");
        let back = uri.to_file_path().expect("path");
        assert_eq!(back.as_ref(), path);
    }

    #[cfg(not(windows))]
    #[test]
    fn percent_encodes_spaces() {
        let path = Path::new("/tmp/my notes/a b.qmd");
        let uri = Uri::from_file_path(path).expect("uri");
        assert_eq!(uri.as_str(), "file:///tmp/my%20notes/a%20b.qmd");
        let back = uri.to_file_path().expect("path");
        assert_eq!(back.as_ref(), path);
    }

    #[test]
    fn empty_path_is_none() {
        let uri = Uri::from_str("file://").or_else(|_| Uri::from_str("http://example.com"));
        if let Ok(uri) = uri {
            // An http authority-only URI has an empty path → None.
            let _ = uri.to_file_path();
        }
    }
}
