//! Blockquote parsing utilities.
//!
//! Re-exports marker parsing functions from marker_utils for backward compatibility.

pub(crate) use super::marker_utils::{count_blockquote_markers, try_parse_blockquote_marker};
