//! Emphasis resolution stage - convert delimiter runs to emphasis nodes

use super::elements::InlineElement;
use super::emphasis;

/// Resolve emphasis from collected inline elements.
///
/// This function:
/// 1. Extracts delimiter runs from the element list
/// 2. Uses the CommonMark delimiter stack algorithm to find matches
/// 3. Converts matched delimiters to Emphasis/Strong elements
/// 4. Removes consumed delimiter runs
///
/// Returns a new list of elements with emphasis resolved.
pub fn resolve_emphasis(elements: Vec<InlineElement>, text: &str) -> Vec<InlineElement> {
    log::trace!("Resolving emphasis from {} elements", elements.len());

    // Build a list of delimiters with their positions in the element list
    let mut delimiters = Vec::new();
    for (idx, element) in elements.iter().enumerate() {
        if let InlineElement::DelimiterRun {
            char: delim_char,
            count,
            can_open,
            can_close,
            start,
            ..
        } = element
        {
            delimiters.push(emphasis::Delimiter {
                char: *delim_char,
                count: *count,
                original_count: *count,
                start_pos: *start,
                can_open: *can_open,
                can_close: *can_close,
                active: true,
                element_index: idx,
            });
        }
    }

    log::trace!("Found {} delimiter runs", delimiters.len());

    // Use the existing emphasis processing algorithm
    let emphasis_matches = emphasis::process_emphasis(&mut delimiters);

    log::trace!("Found {} emphasis matches", emphasis_matches.len());

    // Now we need to convert the flat element list into a nested structure
    // based on the emphasis matches
    build_emphasis_tree(elements, emphasis_matches, text)
}

/// Build a nested tree of inline elements with emphasis wrapping.
///
/// This uses a position-based recursive approach:
/// 1. Track consumed byte ranges from emphasis matches
/// 2. Process elements in order by byte position
/// 3. Recursively build children for emphasis nodes
/// 4. Convert unconsumed delimiters to text
fn build_emphasis_tree(
    elements: Vec<InlineElement>,
    matches: Vec<emphasis::EmphasisMatch>,
    _text: &str,
) -> Vec<InlineElement> {
    if matches.is_empty() {
        log::trace!("No emphasis matches - converting delimiter runs to text");
        // No emphasis - just filter out delimiter runs and convert to text
        return elements
            .into_iter()
            .map(|elem| {
                if let InlineElement::DelimiterRun {
                    char: delim_char,
                    count,
                    start,
                    end,
                    ..
                } = elem
                {
                    // Unconsumed delimiter - emit as plain text
                    InlineElement::Text {
                        content: delim_char.to_string().repeat(count),
                        start,
                        end,
                    }
                } else {
                    elem
                }
            })
            .collect();
    }

    // Sort matches by span to ensure proper nesting:
    // 1. Larger spans first (outer matches)
    // 2. For equal spans, earlier start position first (outermost)
    let mut sorted_matches = matches;
    sorted_matches.sort_by_key(|m| (std::cmp::Reverse(m.end - m.start), m.start));

    log::debug!(
        "Building emphasis tree from {} matches and {} elements",
        sorted_matches.len(),
        elements.len()
    );
    for (i, m) in sorted_matches.iter().enumerate() {
        log::trace!(
            "  Match {}: level={}, char={}, start={}, end={}, content={}..{} (size {})",
            i,
            m.level,
            m.delim_char,
            m.start,
            m.end,
            m.content_start,
            m.content_end,
            m.content_end - m.content_start
        );
    }

    // Build tree recursively using byte position ranges
    build_tree_recursive(&elements, &sorted_matches, 0, usize::MAX)
}

/// Recursively build emphasis tree within a byte range.
///
/// This processes elements and matches that fall within [range_start, range_end).
/// For each emphasis match in this range:
/// - Recursively build children from the match's content range
/// - Create Emphasis/Strong node wrapping the children
/// - Add elements before/after matches as-is
fn build_tree_recursive(
    elements: &[InlineElement],
    matches: &[emphasis::EmphasisMatch],
    range_start: usize,
    range_end: usize,
) -> Vec<InlineElement> {
    log::trace!(
        "build_tree_recursive: range {}..{}, {} elements, {} matches",
        range_start,
        range_end,
        elements.len(),
        matches.len()
    );
    eprintln!(
        "DEBUG build_tree_recursive: range {}..{}, {} elements, {} matches",
        range_start,
        range_end,
        elements.len(),
        matches.len()
    );

    let mut result = Vec::new();
    let mut pos = range_start;

    // Find matches whose CONTENT falls within this range
    // (the match's delimiters may extend outside the range)
    let matches_in_range: Vec<&emphasis::EmphasisMatch> = matches
        .iter()
        .filter(|m| {
            // Match's content should overlap with this range
            m.content_start >= range_start
                && m.content_start < range_end
                && m.content_end > range_start
                && m.content_end <= range_end
        })
        .collect();

    log::trace!("  {} matches in range", matches_in_range.len());
    for (i, m) in matches_in_range.iter().enumerate() {
        log::trace!(
            "    Match {} in range: level={}, content={}..{}",
            i,
            m.level,
            m.content_start,
            m.content_end
        );
        eprintln!(
            "DEBUG   Match {} in range: level={}, content={}..{}",
            i, m.level, m.content_start, m.content_end
        );
    }

    // Track which byte positions are consumed by emphasis delimiters
    let mut consumed_ranges: Vec<(usize, usize)> = Vec::new();
    for m in &matches_in_range {
        // Opening delimiter: start..content_start
        consumed_ranges.push((m.start, m.content_start));
        // Closing delimiter: content_end..end
        consumed_ranges.push((m.content_end, m.end));
    }

    // Process all matches in this range first (they take precedence)
    for m in &matches_in_range {
        // Check if this match's content starts at or after current position
        // (the match's delimiters might be before pos, but content is what matters)
        if m.content_start >= pos {
            eprintln!(
                "DEBUG   Processing match at content_start={}, level={}",
                m.content_start, m.level
            );

            // IMPORTANT: Handle partially consumed opener delimiters
            // If m.start > range_start, there may be unconsumed opener delimiters BEFORE m.start
            // Example: ***foo** has delimiter at 0-3, match.start=1, so byte 0 is unconsumed
            if m.start > pos {
                // Check if there's a delimiter run that contains unconsumed prefix
                for elem in elements {
                    if let InlineElement::DelimiterRun {
                        char: delim_char,
                        start,
                        end,
                        ..
                    } = elem
                    {
                        // If this delimiter run starts before m.start and extends into/past it
                        if *start < m.start && *end > m.start {
                            // Emit text for the unconsumed prefix (start..m.start)
                            let unconsumed_start = (*start).max(pos);
                            let unconsumed_end = m.start;
                            let unconsumed_count = (unconsumed_end - unconsumed_start) as usize;
                            if unconsumed_count > 0 {
                                log::trace!(
                                    "  Emitting {} unconsumed opener delimiters at {}..{}",
                                    unconsumed_count,
                                    unconsumed_start,
                                    unconsumed_end
                                );
                                result.push(InlineElement::Text {
                                    content: delim_char.to_string().repeat(unconsumed_count),
                                    start: unconsumed_start,
                                    end: unconsumed_end,
                                });
                            }
                        }
                    }
                }
            }

            // Add any non-delimiter elements before this match's content
            for elem in elements {
                let elem_start = elem.start();
                let elem_end = elem.end();

                // Only process elements in the gap before this match's content
                if elem_start >= pos
                    && elem_end <= m.content_start
                    && !matches!(elem, InlineElement::DelimiterRun { .. })
                {
                    log::trace!("  Adding element at {} before match", elem_start);
                    result.push(elem.clone());
                }
            }

            // Recursively build children from the content range
            // IMPORTANT: Exclude this match from the recursive call to avoid infinite loops
            let child_matches: Vec<emphasis::EmphasisMatch> = matches
                .iter()
                .filter(|other| {
                    // Don't include this match itself
                    other.start != m.start || other.end != m.end
                })
                .cloned()
                .collect();

            let children =
                build_tree_recursive(elements, &child_matches, m.content_start, m.content_end);

            // Create the emphasis node
            let emphasis_node = if m.level == 1 {
                InlineElement::Emphasis {
                    delim_char: m.delim_char,
                    children,
                    start: m.start,
                    end: m.end,
                }
            } else {
                InlineElement::Strong {
                    delim_char: m.delim_char,
                    children,
                    start: m.start,
                    end: m.end,
                }
            };

            result.push(emphasis_node);

            // IMPORTANT: Handle partially consumed closer delimiters
            // If m.content_end < m.end, the closer consumed some but not all delimiters
            // Example: **foo*** has closer at 5-8, match uses 5-7, leaving 7-8 unconsumed
            if m.content_end < m.end {
                // Check if there's a delimiter run that was partially consumed
                for elem in elements {
                    if let InlineElement::DelimiterRun {
                        char: delim_char,
                        start,
                        end,
                        ..
                    } = elem
                    {
                        // If this delimiter run overlaps with the consumed closer range
                        if *start < m.end && *end > m.end {
                            // Emit text for the unconsumed portion
                            let unconsumed_count = (*end - m.end) as usize;
                            if unconsumed_count > 0 {
                                log::trace!(
                                    "  Emitting {} unconsumed closer delimiters at {}",
                                    unconsumed_count,
                                    m.end
                                );
                                result.push(InlineElement::Text {
                                    content: delim_char.to_string().repeat(unconsumed_count),
                                    start: m.end,
                                    end: *end,
                                });
                            }
                        }
                    }
                }
            }

            pos = m.content_end; // Move past this match's content
        }
    }

    // Add any remaining non-delimiter elements after all matches
    for elem in elements {
        let elem_start = elem.start();
        let elem_end = elem.end();

        // Only process elements after the last match
        if elem_start >= pos && elem_end <= range_end {
            // Check if this delimiter run is consumed
            let is_consumed = consumed_ranges
                .iter()
                .any(|(start, end)| elem_start >= *start && elem_start < *end);

            if let InlineElement::DelimiterRun {
                char: delim_char,
                count,
                start,
                end,
                ..
            } = elem
            {
                if !is_consumed {
                    log::trace!(
                        "  Unconsumed delimiter at {}: {}x{}",
                        start,
                        count,
                        delim_char
                    );
                    // Unconsumed delimiter - emit as plain text
                    result.push(InlineElement::Text {
                        content: delim_char.to_string().repeat(*count),
                        start: *start,
                        end: *end,
                    });
                } else {
                    log::trace!("  Skipping consumed delimiter at {}", start);
                }
            } else {
                // Regular element (code, link, text, etc.) - keep as-is
                log::trace!("  Adding remaining element at {}", elem_start);
                result.push(elem.clone());
            }
        }
    }

    log::trace!("  Returning {} elements from recursive call", result.len());
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_no_emphasis() {
        let elements = vec![InlineElement::Text {
            content: "hello world".to_string(),
            start: 0,
            end: 11,
        }];
        let result = resolve_emphasis(elements.clone(), "hello world");
        assert_eq!(result.len(), 1);
        assert_eq!(result, elements);
    }

    #[test]
    fn test_resolve_simple_emphasis() {
        // *foo* -> opening delimiter, "foo", closing delimiter
        let elements = vec![
            InlineElement::DelimiterRun {
                char: '*',
                count: 1,
                can_open: true,
                can_close: false,
                start: 0,
                end: 1,
            },
            InlineElement::Text {
                content: "foo".to_string(),
                start: 1,
                end: 4,
            },
            InlineElement::DelimiterRun {
                char: '*',
                count: 1,
                can_open: false,
                can_close: true,
                start: 4,
                end: 5,
            },
        ];

        let result = resolve_emphasis(elements, "*foo*");

        // Should have 1 emphasis node
        assert_eq!(result.len(), 1);
        match &result[0] {
            InlineElement::Emphasis {
                delim_char,
                children,
                ..
            } => {
                assert_eq!(*delim_char, '*');
                assert_eq!(children.len(), 1);
                match &children[0] {
                    InlineElement::Text { content, .. } => {
                        assert_eq!(content, "foo");
                    }
                    _ => panic!("Expected Text child"),
                }
            }
            _ => panic!("Expected Emphasis node"),
        }
    }

    #[test]
    fn test_resolve_unconsumed_delimiters() {
        // *foo - no closing delimiter
        let elements = vec![
            InlineElement::DelimiterRun {
                char: '*',
                count: 1,
                can_open: true,
                can_close: false,
                start: 0,
                end: 1,
            },
            InlineElement::Text {
                content: "foo".to_string(),
                start: 1,
                end: 4,
            },
        ];

        let result = resolve_emphasis(elements, "*foo");

        // Should have 2 elements: "*" as text, then "foo"
        assert_eq!(result.len(), 2);
        match &result[0] {
            InlineElement::Text { content, .. } => {
                assert_eq!(content, "*");
            }
            _ => panic!("Expected Text for unconsumed delimiter"),
        }
    }

    #[test]
    fn test_resolve_triple_emphasis() {
        // ***foo*** -> nested strong and emphasis
        let elements = vec![
            InlineElement::DelimiterRun {
                char: '*',
                count: 3,
                can_open: true,
                can_close: true,
                start: 0,
                end: 3,
            },
            InlineElement::Text {
                content: "foo".to_string(),
                start: 3,
                end: 6,
            },
            InlineElement::DelimiterRun {
                char: '*',
                count: 3,
                can_open: true,
                can_close: true,
                start: 6,
                end: 9,
            },
        ];

        // First, let's see what matches are produced
        let mut delimiters = vec![
            emphasis::Delimiter {
                char: '*',
                count: 3,
                original_count: 3,
                start_pos: 0,
                can_open: true,
                can_close: true,
                active: true,
                element_index: 0,
            },
            emphasis::Delimiter {
                char: '*',
                count: 3,
                original_count: 3,
                start_pos: 6,
                can_open: true,
                can_close: true,
                active: true,
                element_index: 2,
            },
        ];

        let matches = emphasis::process_emphasis(&mut delimiters);
        println!("Triple emphasis matches: {}", matches.len());
        for (i, m) in matches.iter().enumerate() {
            println!(
                "  Match {}: level={}, start={}, end={}, content={}..{}",
                i, m.level, m.start, m.end, m.content_start, m.content_end
            );
        }

        let result = resolve_emphasis(elements, "***foo***");
        println!("Result elements: {}", result.len());
        for (i, elem) in result.iter().enumerate() {
            println!("  Element {}: {:?}", i, elem);
        }

        // Should have nested Strong containing Emphasis
        assert_eq!(result.len(), 1, "Should have one top-level element");

        // The outer element should be Strong or Emphasis
        // (CommonMark processes 2+2 first, then 1+1, so it should be Strong(Emphasis(text)))
        match &result[0] {
            InlineElement::Strong { children, .. } => {
                assert_eq!(children.len(), 1, "Strong should have one child");
                match &children[0] {
                    InlineElement::Emphasis {
                        children: em_children,
                        ..
                    } => {
                        assert_eq!(em_children.len(), 1, "Emphasis should have one child");
                    }
                    other => panic!("Expected Emphasis inside Strong, got {:?}", other),
                }
            }
            other => panic!("Expected Strong as outer element, got {:?}", other),
        }
    }
}
