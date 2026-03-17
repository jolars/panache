use crate::config::{Config, WrapMode};
use crate::syntax::YamlParseError;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn validate_yaml(input: &str) -> Result<(), YamlParseError> {
    crate::syntax::validate_yaml_text(input)
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn validate_yaml(_input: &str) -> Result<(), YamlParseError> {
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn format_yaml_with_config(input: &str, config: &Config) -> Result<String, String> {
    validate_yaml(input).map_err(|e| e.message().to_string())?;
    let mut options = pretty_yaml::config::FormatOptions::default();
    options.layout.print_width = config.line_width;
    options.language.prose_wrap = prose_wrap_for_config(config);
    pretty_yaml::format_text(input, &options).map_err(|e| e.to_string())
}

#[cfg(not(target_arch = "wasm32"))]
fn prose_wrap_for_config(config: &Config) -> pretty_yaml::config::ProseWrap {
    match config.wrap {
        Some(WrapMode::Preserve) => pretty_yaml::config::ProseWrap::Preserve,
        Some(WrapMode::Reflow) | Some(WrapMode::Sentence) | None => {
            pretty_yaml::config::ProseWrap::Always
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn format_yaml_with_config(input: &str, _config: &Config) -> Result<String, String> {
    Ok(input.to_string())
}

#[cfg(test)]
mod tests {
    use super::validate_yaml;
    #[cfg(not(target_arch = "wasm32"))]
    use super::{format_yaml_with_config, prose_wrap_for_config};
    use crate::config::{Config, WrapMode};

    #[test]
    fn preserves_block_scalar_styles() {
        let input = "fig-cap: >-\n  A folded caption\n  spanning some lines\n";
        let out = format_yaml_with_config(input, &Config::default()).expect("yaml should format");
        assert!(out.contains("fig-cap: >-"));
    }

    #[test]
    fn validate_yaml_reports_offset() {
        let err = validate_yaml("a: [\n").expect_err("should fail");
        assert!(err.offset() <= 4);
        assert!(!err.message().is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn prose_wrap_follows_panache_wrap_mode() {
        let preserve = Config {
            wrap: Some(WrapMode::Preserve),
            ..Default::default()
        };
        let reflow = Config {
            wrap: Some(WrapMode::Reflow),
            ..Default::default()
        };
        let sentence = Config {
            wrap: Some(WrapMode::Sentence),
            ..Default::default()
        };
        assert!(matches!(
            prose_wrap_for_config(&preserve),
            pretty_yaml::config::ProseWrap::Preserve
        ));
        assert!(matches!(
            prose_wrap_for_config(&reflow),
            pretty_yaml::config::ProseWrap::Always
        ));
        assert!(matches!(
            prose_wrap_for_config(&sentence),
            pretty_yaml::config::ProseWrap::Always
        ));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn line_width_follows_panache_config() {
        let long = "title: This is a very long yaml scalar that should not stay on one line when width is narrow.\n";
        let preserve = Config {
            line_width: 120,
            wrap: Some(WrapMode::Preserve),
            ..Default::default()
        };
        let reflow = Config {
            line_width: 30,
            wrap: Some(WrapMode::Reflow),
            ..Default::default()
        };
        let preserved = format_yaml_with_config(long, &preserve).expect("preserve should format");
        let reflowed = format_yaml_with_config(long, &reflow).expect("reflow should format");
        assert_ne!(preserved, reflowed);
    }
}
