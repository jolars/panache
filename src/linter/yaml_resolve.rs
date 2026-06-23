//! Minimal YAML scalar *type resolution* for the [`consumer-divergence`] rule.
//!
//! [`consumer-divergence`]: crate::linter::rules::consumer_divergence
//!
//! This is not a general YAML resolver — it models only the handful of plain
//! scalars whose resolved tag/value genuinely *differs* between the two YAML
//! versions a Quarto document is read under: pandoc's libyaml (≈ YAML 1.1) and
//! Quarto's js-yaml (YAML 1.2 core). Everything else collapses to
//! [`Resolved::Str`] under *both* versions, so it can never produce a false
//! divergence.
//!
//! The modeled divergences (v1 scope):
//!
//! - **Booleans (the "Norway problem").** YAML 1.1 resolves a wide set of
//!   words to booleans (`y/n`, `yes/no`, `on/off`, `true/false` and their case
//!   variants — see the YAML 1.1 *Timestamp/Bool* type repository,
//!   <https://yaml.org/type/bool.html>). YAML 1.2 core (and js-yaml v4, whose
//!   `type/bool.js` only matches `true|True|TRUE|false|False|FALSE`) resolves
//!   only `true`/`false`. So `country: no` is the boolean `false` to pandoc but
//!   the string `"no"` to Quarto.
//! - **Leading-zero integers.** YAML 1.1 reads `0[0-7]+` as octal (`0755` →
//!   493). js-yaml core (`type/int.js`) accepts only `0o…` octal; a leading-zero
//!   token like `0755`/`010` is not a valid int and stays a string. So the two
//!   stages disagree (int vs string).
//!
//! Sexagesimal (`1:30`) and the dotted/scientific float corners are deliberately
//! left as `Str`-under-both for now: modeling js-yaml's exact float regex
//! reliably needs an empirical resolved-value oracle, and getting it wrong would
//! violate the rule's "never false-positive" contract.

use crate::parser::yaml::YamlConsumer;

/// The two scalar-resolution behaviors among the real consumers. libyaml and
/// R's `yaml` are both libyaml-based (≈ 1.1); js-yaml is 1.2 core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum YamlVersion {
    V1_1,
    V1_2Core,
}

/// The resolution behavior a given consumer applies.
pub fn version_of(consumer: YamlConsumer) -> YamlVersion {
    match consumer {
        YamlConsumer::Libyaml | YamlConsumer::RYaml => YamlVersion::V1_1,
        YamlConsumer::Jsyaml => YamlVersion::V1_2Core,
    }
}

/// A resolved scalar, reduced to the facts that matter for cross-version
/// divergence. Two `Resolved` that compare unequal are a divergence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolved {
    Bool(bool),
    Int(i64),
    /// Any value both versions agree is a string, plus anything outside the
    /// modeled set (floats, words, dates, ...).
    Str,
}

/// Resolve a single-line **plain** scalar's text under `version`. Callers must
/// only pass plain scalars (quoted/literal/folded are always strings).
pub fn resolve_plain(text: &str, version: YamlVersion) -> Resolved {
    let t = text.trim();
    if t.is_empty() {
        return Resolved::Str;
    }
    if let Some(b) = resolve_bool(t, version) {
        return Resolved::Bool(b);
    }
    if let Some(i) = resolve_int(t, version) {
        return Resolved::Int(i);
    }
    Resolved::Str
}

fn resolve_bool(t: &str, version: YamlVersion) -> Option<bool> {
    match version {
        YamlVersion::V1_1 => match t {
            "y" | "Y" | "yes" | "Yes" | "YES" | "true" | "True" | "TRUE" | "on" | "On" | "ON" => {
                Some(true)
            }
            "n" | "N" | "no" | "No" | "NO" | "false" | "False" | "FALSE" | "off" | "Off"
            | "OFF" => Some(false),
            _ => None,
        },
        YamlVersion::V1_2Core => match t {
            "true" | "True" | "TRUE" => Some(true),
            "false" | "False" | "FALSE" => Some(false),
            _ => None,
        },
    }
}

fn resolve_int(t: &str, version: YamlVersion) -> Option<i64> {
    let (sign, rest) = match t.strip_prefix('-') {
        Some(r) => (-1i64, r),
        None => (1i64, t.strip_prefix('+').unwrap_or(t)),
    };
    if rest.is_empty() {
        return None;
    }

    // Hexadecimal (`0x…`) is an integer under both versions.
    if let Some(hex) = rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")) {
        if hex.is_empty() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        return i64::from_str_radix(hex, 16).ok().map(|v| sign * v);
    }

    match version {
        YamlVersion::V1_1 => {
            // A leading zero means octal (`0[0-7]+`); a leading-zero token with a
            // non-octal digit (e.g. `0789`) is *not* a 1.1 decimal int — it is a
            // string, same as 1.2. Returning `None` here keeps those non-diverging.
            if rest.len() >= 2 && rest.starts_with('0') {
                if rest.bytes().all(|b| b.is_ascii_digit() && b <= b'7') {
                    return i64::from_str_radix(rest, 8).ok().map(|v| sign * v);
                }
                return None;
            }
            if rest.bytes().all(|b| b.is_ascii_digit()) {
                return rest.parse::<i64>().ok().map(|v| sign * v);
            }
            None
        }
        YamlVersion::V1_2Core => {
            if let Some(oct) = rest.strip_prefix("0o") {
                if oct.is_empty() || !oct.bytes().all(|b| b.is_ascii_digit() && b <= b'7') {
                    return None;
                }
                return i64::from_str_radix(oct, 8).ok().map(|v| sign * v);
            }
            // Decimal only, and no leading zero (`0755` is a string here).
            if rest == "0" {
                return Some(0);
            }
            if rest.starts_with('0') || !rest.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            rest.parse::<i64>().ok().map(|v| sign * v)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r11(t: &str) -> Resolved {
        resolve_plain(t, YamlVersion::V1_1)
    }
    fn r12(t: &str) -> Resolved {
        resolve_plain(t, YamlVersion::V1_2Core)
    }

    #[test]
    fn version_mapping() {
        assert_eq!(version_of(YamlConsumer::Libyaml), YamlVersion::V1_1);
        assert_eq!(version_of(YamlConsumer::RYaml), YamlVersion::V1_1);
        assert_eq!(version_of(YamlConsumer::Jsyaml), YamlVersion::V1_2Core);
    }

    #[test]
    fn norway_booleans_diverge() {
        for s in ["no", "No", "NO", "yes", "on", "off", "y", "n", "Y", "N"] {
            assert!(matches!(r11(s), Resolved::Bool(_)), "1.1 {s}");
            assert_eq!(r12(s), Resolved::Str, "1.2 {s}");
        }
        assert_eq!(r11("no"), Resolved::Bool(false));
        assert_eq!(r11("yes"), Resolved::Bool(true));
    }

    #[test]
    fn canonical_booleans_agree() {
        for s in ["true", "True", "TRUE", "false", "False", "FALSE"] {
            assert_eq!(r11(s), r12(s), "{s}");
            assert!(matches!(r11(s), Resolved::Bool(_)));
        }
    }

    #[test]
    fn leading_zero_octal_diverges() {
        assert_eq!(r11("0755"), Resolved::Int(0o755));
        assert_eq!(r12("0755"), Resolved::Str);
        assert_eq!(r11("010"), Resolved::Int(8));
        assert_eq!(r12("010"), Resolved::Str);
    }

    #[test]
    fn leading_zero_non_octal_agrees_as_string() {
        // `0789` is octal-invalid: a string under *both* versions, so no
        // divergence and no false positive.
        assert_eq!(r11("0789"), Resolved::Str);
        assert_eq!(r12("0789"), Resolved::Str);
    }

    #[test]
    fn plain_decimals_and_hex_agree() {
        for s in ["0", "42", "-7", "+3", "0x1f", "-0x10"] {
            assert_eq!(r11(s), r12(s), "{s}");
            assert!(matches!(r11(s), Resolved::Int(_)), "{s}");
        }
        assert_eq!(r11("0x1f"), Resolved::Int(31));
    }

    #[test]
    fn floats_and_words_are_strings_both() {
        for s in [
            "3.14",
            ".inf",
            ".nan",
            "-.inf",
            "1e3",
            "hello",
            "2024-01-01",
            "",
        ] {
            assert_eq!(r11(s), Resolved::Str, "1.1 {s}");
            assert_eq!(r12(s), Resolved::Str, "1.2 {s}");
        }
    }
}
