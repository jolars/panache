//! Shared LaTeX environment parsing utilities.

/// Information about a detected LaTeX environment opening.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LatexEnvInfo {
    pub env_name: String,
}
