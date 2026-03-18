use crate::parser::blocks::code_blocks::{CodeBlockType, InfoString};
use crate::parser::utils::chunk_options::hashpipe_comment_prefix;

/// Resolve executable chunk language + hashpipe prefix from a code info string.
pub fn hashpipe_language_and_prefix(info_text: &str) -> Option<(String, &'static str)> {
    let info = InfoString::parse(info_text);
    let language = match info.block_type {
        CodeBlockType::Executable { language } => language,
        _ => return None,
    };
    let prefix = hashpipe_comment_prefix(&language)?;
    Some((language, prefix))
}
