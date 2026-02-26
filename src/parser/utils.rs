//! Shared utilities for parser implementation.
//!
//! This module contains utilities used by both block and inline parsers,
//! including attribute parsing, text buffering, container management, etc.

#[path = "utils/attributes.rs"]
pub mod attributes; // Public for use in inline parser and formatter
#[path = "utils/chunk_options.rs"]
pub mod chunk_options; // Public for hashpipe formatter
#[path = "utils/container_stack.rs"]
pub mod container_stack;
#[path = "utils/helpers.rs"]
pub mod helpers;
#[path = "utils/inline_emission.rs"]
pub mod inline_emission;
#[path = "utils/list_item_buffer.rs"]
pub mod list_item_buffer;
#[path = "utils/marker_utils.rs"]
pub mod marker_utils;
#[path = "utils/text_buffer.rs"]
pub mod text_buffer;
