pub(super) fn count_unescaped_single_dollars(text: &str) -> usize {
    let mut singles = 0usize;
    let mut backslashes = 0usize;
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\\' {
            backslashes += 1;
            i += 1;
            continue;
        }
        let escaped = backslashes % 2 == 1;
        backslashes = 0;
        if ch != '$' || escaped {
            i += 1;
            continue;
        }
        let mut run = 1usize;
        while i + run < chars.len() && chars[i + run] == '$' {
            run += 1;
        }
        singles += run % 2;
        i += run;
    }
    singles
}

pub(super) fn has_ambiguous_dollar_delimiters(text: &str) -> bool {
    let mut singles = 0usize;
    let mut doubles = 0usize;
    let mut backslashes = 0usize;
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\\' {
            backslashes += 1;
            i += 1;
            continue;
        }
        let escaped = backslashes % 2 == 1;
        backslashes = 0;
        if ch != '$' || escaped {
            i += 1;
            continue;
        }
        let mut run = 1usize;
        while i + run < chars.len() && chars[i + run] == '$' {
            run += 1;
        }
        doubles += run / 2;
        singles += run % 2;
        i += run;
    }
    (doubles > 0 && singles > 0) || singles % 2 == 1
}
