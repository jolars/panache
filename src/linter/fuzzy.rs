//! Shared fuzzy-matching helpers for "did you mean...?" lint suggestions.
//!
//! Two rules want the same primitive: given an unknown token and a set of known
//! values, find the closest known value within a small edit-distance budget.
//! `html-entities` suggests the nearest named entity; the Quarto schema rule
//! suggests the nearest known key. Keeping the distance cap and the
//! deterministic tie-break in one place stops the two callers from drifting
//! apart (they previously carried separate, subtly different copies).

/// Levenshtein edit distance between `a` and `b`, capped at `max`.
///
/// Returns [`usize::MAX`] as soon as the distance provably exceeds `max`, so
/// callers can cheaply reject distant candidates without paying the full
/// O(n·m) fill. Counts Unicode scalar values, not bytes, so multibyte text is
/// measured by character.
pub(crate) fn levenshtein(a: &str, b: &str, max: usize) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > max {
        return usize::MAX;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        let mut row_min = curr[0];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            curr[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(curr[j] + 1);
            row_min = row_min.min(curr[j + 1]);
        }
        if row_min > max {
            return usize::MAX;
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    // The per-row bail above only fires when an *entire* row exceeds `max`; the
    // diagonal can keep one cell within budget while the true distance still
    // ends up over it. Clamp here so the cap is actually enforced.
    let dist = prev[b.len()];
    if dist > max { usize::MAX } else { dist }
}

/// Find the candidate closest to `target` within `max_distance` edits.
///
/// Exact matches (distance 0) are skipped: this powers "did you mean...?"
/// suggestions, where `target` is by definition not a known value. Ties are
/// broken alphabetically (smallest wins) so the suggestion is deterministic
/// regardless of candidate iteration order — important when candidates come
/// from a `HashSet`.
pub(crate) fn nearest_match<'a>(
    target: &str,
    candidates: impl IntoIterator<Item = &'a str>,
    max_distance: usize,
) -> Option<&'a str> {
    let mut best: Option<(usize, &'a str)> = None;
    for cand in candidates {
        let d = levenshtein(target, cand, max_distance);
        if d == 0 || d == usize::MAX {
            continue;
        }
        let better = match best {
            None => true,
            Some((bd, bc)) => d < bd || (d == bd && cand < bc),
        };
        if better {
            best = Some((d, cand));
        }
    }
    best.map(|(_, c)| c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levenshtein_caps() {
        assert_eq!(levenshtein("format", "format", 2), 0);
        assert_eq!(levenshtein("forrmat", "format", 2), 1);
        assert_eq!(levenshtein("abcdefg", "xyz", 2), usize::MAX);
        // A true distance of 2 must report as over-budget when the cap is 1,
        // even though no single DP row ever fully exceeds the cap. Otherwise
        // the cap is unenforced and open objects nag legitimate custom keys
        // (e.g. `cran` is distance 2 from `brand`).
        assert_eq!(levenshtein("cran", "brand", 1), usize::MAX);
    }

    #[test]
    fn nearest_match_skips_exact_and_over_budget() {
        // Exact matches are not suggestions.
        assert_eq!(nearest_match("pdf", ["pdf", "pdfa"], 1), Some("pdfa"));
        // Nothing within budget → no suggestion.
        assert_eq!(nearest_match("cran", ["brand"], 1), None);
    }

    #[test]
    fn nearest_match_breaks_ties_alphabetically() {
        // Both candidates sit at distance 2 from "ellips"; the alphabetically
        // earliest must win so output is deterministic across iteration orders.
        assert_eq!(
            nearest_match("ellips", ["vellip", "hellip"], 2),
            Some("hellip")
        );
    }
}
