//! This file explores the FUTURE architecture needs when we implement links and emphasis.
//!
//! The question: Will our current linear parsing approach handle nesting properly?
//!
//! Current approach: Single-pass left-to-right scan that eagerly matches delimiters.
//!
//! Example concerns:
//! - `[**bold link text**](url)` - Link containing bold
//! - `*emphasis with `code` inside*` - Emphasis containing code
//! - `[outer [inner] text](url)` - Nested brackets in links
//!
//! The CommonMark spec uses a delimiter stack approach for emphasis/links to handle
//! these cases correctly. We may need to refactor before implementing those features.

#[cfg(test)]
mod future_architecture_tests {
    // These tests document what WILL need to work when we implement
    // emphasis and links. They're disabled for now.

    #[test]
    #[ignore = "Links not yet implemented"]
    fn link_with_code_inside() {
        // [click `here`](url) should parse as:
        // - Link node containing:
        //   - TEXT "click "
        //   - CodeSpan "`here`"
        //   - Destination "(url)"
        //
        // Current approach: Would need to parse link structure first,
        // then recursively parse inline elements in link text.
    }

    #[test]
    #[ignore = "Emphasis not yet implemented"]
    fn emphasis_with_code_inside() {
        // *emphasis with `code` inside* should parse as:
        // - Emphasis node containing:
        //   - TEXT "emphasis with "
        //   - CodeSpan "`code`"
        //   - TEXT " inside"
        //
        // Current approach: Would need to match emphasis delimiters,
        // then recursively parse content between them.
    }

    #[test]
    #[ignore = "Links not yet implemented"]
    fn nested_brackets_in_link() {
        // [outer [inner] text](url)
        // CommonMark: Outer brackets win, inner become literal
        //
        // This requires proper bracket matching with escape handling.
    }

    #[test]
    #[ignore = "Complex nesting not yet designed"]
    fn emphasis_link_code_nesting() {
        // *emphasis [with [link](url)] and `code`*
        //
        // This requires careful precedence:
        // 1. Code spans are atomic (highest precedence)
        // 2. Links next
        // 3. Emphasis last (lowest precedence)
        //
        // May need multi-pass or delimiter stack approach.
    }
}

/// Documentation of architectural approaches for nested inline elements:
///
/// ## Approach 1: Current (Linear Eager Matching)
/// - Single pass left-to-right
/// - Match delimiters eagerly
/// - Works for: code spans, inline math (atomic elements)
/// - Issues: Can't handle complex nesting, emphasis precedence
///
/// ## Approach 2: Delimiter Stack (CommonMark)
/// - Track opening delimiters in a stack
/// - Match closing delimiters with stack entries
/// - Handle precedence by delimiter type
/// - Works for: emphasis, links, all nesting
/// - Complexity: More complex state management
///
/// ## Approach 3: Multi-Pass
/// - Pass 1: Parse atomic elements (code, math)
/// - Pass 2: Parse links
/// - Pass 3: Parse emphasis
/// - Works for: All cases with clear precedence
/// - Complexity: Multiple tree traversals
///
/// ## Approach 4: Recursive Descent
/// - Parse container elements (links, emphasis)
/// - Recursively parse their content
/// - Works for: Clean nesting
/// - Complexity: Need to mark which TEXT ranges are "consumed"
///
/// ## Recommendation:
/// For panache, a **hybrid approach** makes sense:
/// - Atomic elements (code, math): Current eager matching ✓
/// - Emphasis: Delimiter stack for proper nesting
/// - Links: Special parsing with recursive content handling
///
/// This matches how CommonMark and Pandoc work.
#[allow(dead_code)]
const ARCHITECTURE_NOTES: () = ();
