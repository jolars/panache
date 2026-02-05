pub(crate) fn strip_leading_spaces(line: &str) -> &str {
    line.strip_prefix("   ")
        .or_else(|| line.strip_prefix("  "))
        .or_else(|| line.strip_prefix(" "))
        .unwrap_or(line)
}

pub(crate) fn get_fence_count(line: &str, fence_char: char) -> Option<usize> {
    if !line.starts_with(fence_char) {
        return None;
    }

    let count = line.chars().take_while(|&c| c == fence_char).count();
    Some(count)
}
