use panache::format;

#[test]
fn hyphen_marker_stays_hyphen() {
    let input = "- Item 1\n- Item 2\n- Item 3\n";
    let expected = "- Item 1\n- Item 2\n- Item 3\n";
    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn asterisk_marker_converts_to_hyphen() {
    let input = "* Item 1\n* Item 2\n* Item 3\n";
    let expected = "- Item 1\n- Item 2\n- Item 3\n";
    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn plus_marker_converts_to_hyphen() {
    let input = "+ Item 1\n+ Item 2\n+ Item 3\n";
    let expected = "- Item 1\n- Item 2\n- Item 3\n";
    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn mixed_bullet_markers_all_convert_to_hyphen() {
    let input = "- Item 1\n* Item 2\n+ Item 3\n";
    // Different bullet markers merge into one tight list (Pandoc behavior)
    let expected = "- Item 1\n- Item 2\n- Item 3\n";
    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn nested_bullet_lists_all_use_hyphen() {
    let input = "* Level 1\n  + Level 2\n    - Level 3\n";
    let expected = "- Level 1\n  - Level 2\n    - Level 3\n";
    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn bullet_and_ordered_lists_mixed() {
    let input = "* Bullet item\n\n1. Ordered item\n2. Another ordered\n\n+ Another bullet\n";
    // Bullet items with blank lines become one loose list; ordered list separate
    let expected = "- Bullet item\n\n1. Ordered item\n2. Another ordered\n\n- Another bullet\n";
    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn task_lists_use_hyphen() {
    let input = "* [ ] Unchecked task\n* [x] Checked task\n* [X] Also checked\n";
    let expected = "- [ ] Unchecked task\n- [x] Checked task\n- [X] Also checked\n";
    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn nested_with_task_lists() {
    let input = "* Parent item\n  + [ ] Subtask 1\n  + [x] Subtask 2\n";
    let expected = "- Parent item\n  - [ ] Subtask 1\n  - [x] Subtask 2\n";
    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn bullet_list_with_multiline_items() {
    let input = "* First item\n  with continuation\n\n* Second item\n";
    let expected = "- First item with continuation\n\n- Second item\n";
    let result = format(input, None, None);
    assert_eq!(result, expected);
}

#[test]
fn formatting_is_idempotent() {
    let input = "* Item 1\n+ Item 2\n- Item 3\n";

    let first_pass = format(input, None, None);
    let second_pass = format(&first_pass, None, None);

    assert_eq!(first_pass, second_pass);
}
