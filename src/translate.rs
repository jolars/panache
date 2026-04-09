use crate::config::{Config, TranslateProvider, TranslateSettings};
use crate::parser::parse;
use crate::syntax::{SyntaxKind, SyntaxNode};
use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde_json::{Value, json};
use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone)]
pub struct TranslateOverrides {
    pub provider: Option<TranslateProvider>,
    pub source_lang: Option<String>,
    pub target_lang: Option<String>,
    pub api_key: Option<String>,
    pub endpoint: Option<String>,
}

#[derive(Debug, Clone)]
struct ResolvedTranslateSettings {
    provider: TranslateProvider,
    source_lang: Option<String>,
    target_lang: String,
    api_key: Option<String>,
    endpoint: Option<String>,
}

#[derive(Debug, Clone)]
struct SpanReplacement {
    start: usize,
    end: usize,
    text: String,
    continuation_indent: Option<String>,
}

#[derive(Debug)]
pub enum TranslateError {
    MissingProvider,
    MissingTargetLanguage,
    MissingApiKey(&'static str),
    Http(String),
    InvalidResponse(String),
    Internal(String),
}

impl Display for TranslateError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingProvider => write!(
                f,
                "Translation provider not configured. Set [translate].provider or pass --provider."
            ),
            Self::MissingTargetLanguage => write!(
                f,
                "Target language not configured. Set [translate].target-lang or pass --target-lang."
            ),
            Self::MissingApiKey(provider) => write!(
                f,
                "Missing API key for {provider}. Set [translate].api-key or pass --api-key."
            ),
            Self::Http(msg) => write!(f, "{msg}"),
            Self::InvalidResponse(msg) => write!(f, "{msg}"),
            Self::Internal(msg) => write!(f, "{msg}"),
        }
    }
}

impl Error for TranslateError {}

pub fn translate_document(
    input: &str,
    cfg: &Config,
    overrides: &TranslateOverrides,
) -> Result<String, TranslateError> {
    let settings = resolve_settings(&cfg.translate, overrides)?;
    let tree = parse(input, Some(cfg.clone()));
    let spans = collect_translatable_spans(input, &tree);
    if spans.is_empty() {
        return Ok(input.to_string());
    }

    let source_texts = spans.iter().map(|s| s.text.clone()).collect::<Vec<_>>();
    let translated = match settings.provider {
        TranslateProvider::Deepl => translate_deepl(&settings, &source_texts)?,
        TranslateProvider::Libretranslate => translate_libretranslate(&settings, &source_texts)?,
    };

    if translated.len() != spans.len() {
        return Err(TranslateError::Internal(format!(
            "translation result count mismatch: got {}, expected {}",
            translated.len(),
            spans.len()
        )));
    }

    let mut output = input.to_string();
    apply_replacements(&mut output, &spans, &translated)?;
    Ok(output)
}

fn resolve_settings(
    cfg: &TranslateSettings,
    overrides: &TranslateOverrides,
) -> Result<ResolvedTranslateSettings, TranslateError> {
    let provider = overrides
        .provider
        .or(cfg.provider)
        .ok_or(TranslateError::MissingProvider)?;
    let target_lang = overrides
        .target_lang
        .clone()
        .or_else(|| cfg.target_lang.clone())
        .ok_or(TranslateError::MissingTargetLanguage)?;
    let source_lang = overrides
        .source_lang
        .clone()
        .or_else(|| cfg.source_lang.clone());
    let api_key = overrides.api_key.clone().or_else(|| cfg.api_key.clone());
    let endpoint = overrides.endpoint.clone().or_else(|| cfg.endpoint.clone());
    Ok(ResolvedTranslateSettings {
        provider,
        source_lang,
        target_lang,
        api_key,
        endpoint,
    })
}

fn collect_translatable_spans(input: &str, tree: &SyntaxNode) -> Vec<SpanReplacement> {
    let mut spans = Vec::new();
    let mut in_yaml_block_scalar = false;
    for token in tree
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
    {
        let kind = token.kind();
        if !matches!(kind, SyntaxKind::TEXT | SyntaxKind::YAML_SCALAR) {
            continue;
        }
        if is_excluded_token(&token) {
            continue;
        }

        if is_yaml_metadata_token(&token) {
            collect_yaml_metadata_spans(input, &token, &mut spans, &mut in_yaml_block_scalar);
            continue;
        }

        if kind == SyntaxKind::YAML_SCALAR {
            collect_yaml_scalar_spans(input, &token, &mut spans);
            continue;
        }

        let range = token.text_range();
        let start = u32::from(range.start()) as usize;
        let end = u32::from(range.end()) as usize;
        maybe_push_span(input, start, end, None, &mut spans);
    }
    spans
}

fn contains_translatable_chars(s: &str) -> bool {
    s.chars().any(|c| c.is_alphabetic()) && !looks_like_non_natural_language(s)
}

fn looks_like_non_natural_language(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return true;
    }
    if trimmed.contains("://") {
        return true;
    }

    let no_spaces = !trimmed.chars().any(char::is_whitespace);
    let pathish_chars_only = trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '/' | '\\' | '_' | '-' | '@'));

    if no_spaces && pathish_chars_only {
        if trimmed.contains('/') || trimmed.contains('\\') {
            return true;
        }
        if trimmed.starts_with('@') && trimmed.contains('.') {
            return true;
        }
        let is_single_word = !trimmed.contains([' ', '\t', '\n']);
        if is_single_word
            && trimmed.contains('.')
            && !trimmed.ends_with('.')
            && !trimmed.starts_with('.')
            && trimmed.split('.').all(|part| !part.is_empty())
        {
            return true;
        }
    }
    false
}

fn maybe_push_span(
    input: &str,
    start: usize,
    end: usize,
    continuation_indent: Option<String>,
    spans: &mut Vec<SpanReplacement>,
) {
    if start >= end || end > input.len() {
        return;
    }
    let text = &input[start..end];
    if !contains_translatable_chars(text) {
        return;
    }
    spans.push(SpanReplacement {
        start,
        end,
        text: text.to_string(),
        continuation_indent,
    });
}

fn collect_yaml_scalar_spans(
    input: &str,
    token: &crate::syntax::SyntaxToken,
    spans: &mut Vec<SpanReplacement>,
) {
    let range = token.text_range();
    let base_start = u32::from(range.start()) as usize;
    let base_end = u32::from(range.end()) as usize;
    if base_start >= base_end || base_end > input.len() {
        return;
    }

    let scalar = &input[base_start..base_end];
    let mut line_offset = 0usize;
    for line in scalar.split_inclusive('\n') {
        let line_body = line.strip_suffix('\n').unwrap_or(line);
        let leading_ws = line_body.len() - line_body.trim_start().len();
        let trailing_trimmed_len = line_body.trim_end().len();
        if leading_ws < trailing_trimmed_len {
            let start = base_start + line_offset + leading_ws;
            let end = base_start + line_offset + trailing_trimmed_len;
            maybe_push_span(input, start, end, None, spans);
        }
        line_offset += line.len();
    }
}

fn is_yaml_metadata_token(token: &crate::syntax::SyntaxToken) -> bool {
    token
        .parent_ancestors()
        .any(|node| node.kind() == SyntaxKind::YAML_METADATA_CONTENT)
}

fn collect_yaml_metadata_spans(
    input: &str,
    token: &crate::syntax::SyntaxToken,
    spans: &mut Vec<SpanReplacement>,
    in_block_scalar: &mut bool,
) {
    let range = token.text_range();
    let start = u32::from(range.start()) as usize;
    let end = u32::from(range.end()) as usize;
    if start >= end || end > input.len() {
        return;
    }

    let line = &input[start..end];
    let trimmed_end_len = line.trim_end().len();
    if trimmed_end_len == 0 {
        return;
    }
    let line_no_trailing = &line[..trimmed_end_len];
    let leading_ws = line_no_trailing.len() - line_no_trailing.trim_start().len();

    if *in_block_scalar {
        if leading_ws > 0 {
            let scalar_start = start + leading_ws;
            let scalar_end = start + line_no_trailing.len();
            let indent = Some(input[start..start + leading_ws].to_string());
            maybe_push_span(input, scalar_start, scalar_end, indent, spans);
            return;
        }
        *in_block_scalar = false;
    }

    if let Some(colon_idx) = line_no_trailing.find(':') {
        let key = line_no_trailing[..colon_idx].trim();
        if !looks_like_yaml_key(key) {
            return;
        }
        let after_colon = &line_no_trailing[colon_idx + 1..];
        let value_trimmed = after_colon.trim_start();
        if value_trimmed.is_empty() {
            return;
        }

        if is_yaml_block_scalar_indicator(value_trimmed) {
            *in_block_scalar = true;
            return;
        }

        let value_leading_ws = after_colon.len() - value_trimmed.len();
        let value_start = start + colon_idx + 1 + value_leading_ws;
        let value_end = start + line_no_trailing.len();
        maybe_push_span(input, value_start, value_end, None, spans);
    }
}

fn looks_like_yaml_key(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

fn is_yaml_block_scalar_indicator(s: &str) -> bool {
    if let Some(first) = s.chars().next() {
        first == '>' || first == '|'
    } else {
        false
    }
}

fn apply_replacements(
    output: &mut String,
    spans: &[SpanReplacement],
    translated: &[String],
) -> Result<(), TranslateError> {
    if spans.len() != translated.len() {
        return Err(TranslateError::Internal(format!(
            "replacement count mismatch: got {}, expected {}",
            translated.len(),
            spans.len()
        )));
    }
    for (span, replacement) in spans.iter().zip(translated.iter()).rev() {
        let replacement = if let Some(indent) = &span.continuation_indent {
            apply_yaml_continuation_indent(replacement, indent)
        } else {
            replacement.to_string()
        };
        output.replace_range(span.start..span.end, &replacement);
    }
    Ok(())
}

fn apply_yaml_continuation_indent(text: &str, indent: &str) -> String {
    if !text.contains('\n') {
        return text.to_string();
    }
    let mut lines = text.split('\n');
    let first = lines.next().unwrap_or_default();
    let mut out = String::from(first);
    for line in lines {
        out.push('\n');
        if !line.is_empty() {
            out.push_str(indent);
        }
        out.push_str(line);
    }
    out
}

fn is_excluded_token(token: &crate::syntax::SyntaxToken) -> bool {
    token.parent_ancestors().any(|node| {
        matches!(
            node.kind(),
            SyntaxKind::CODE_SPAN
                | SyntaxKind::INLINE_EXECUTABLE_CODE
                | SyntaxKind::CODE_BLOCK
                | SyntaxKind::CODE_CONTENT
                | SyntaxKind::INLINE_MATH
                | SyntaxKind::DISPLAY_MATH
                | SyntaxKind::MATH_CONTENT
                | SyntaxKind::RAW_INLINE
                | SyntaxKind::RAW_INLINE_CONTENT
                | SyntaxKind::TEX_BLOCK
                | SyntaxKind::HTML_BLOCK
                | SyntaxKind::HTML_BLOCK_CONTENT
                | SyntaxKind::SHORTCODE
                | SyntaxKind::SHORTCODE_CONTENT
        )
    })
}

fn translate_deepl(
    settings: &ResolvedTranslateSettings,
    texts: &[String],
) -> Result<Vec<String>, TranslateError> {
    let api_key = settings
        .api_key
        .clone()
        .ok_or(TranslateError::MissingApiKey("DeepL"))?;
    let endpoint = settings
        .endpoint
        .clone()
        .unwrap_or_else(|| "https://api-free.deepl.com/v2/translate".to_string());
    let target_lang = settings.target_lang.to_uppercase();
    let source_lang = settings.source_lang.as_ref().map(|s| s.to_uppercase());

    let mut params = vec![("target_lang".to_string(), target_lang)];
    if let Some(src) = source_lang {
        params.push(("source_lang".to_string(), src));
    }
    for text in texts {
        params.push(("text".to_string(), text.clone()));
    }

    let client = Client::new();
    let response = client
        .post(endpoint)
        .header(AUTHORIZATION, format!("DeepL-Auth-Key {api_key}"))
        .form(&params)
        .send()
        .map_err(|e| TranslateError::Http(format!("DeepL request failed: {e}")))?;

    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .unwrap_or_else(|_| "<unable to read response body>".to_string());
        return Err(TranslateError::Http(format!(
            "DeepL request failed with status {status}: {body}"
        )));
    }

    let value: Value = response
        .json()
        .map_err(|e| TranslateError::InvalidResponse(format!("DeepL JSON parse failed: {e}")))?;
    parse_deepl_translations(&value)
}

fn parse_deepl_translations(value: &Value) -> Result<Vec<String>, TranslateError> {
    let translations = value
        .get("translations")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            TranslateError::InvalidResponse("DeepL response missing `translations` array".into())
        })?;

    let mut out = Vec::with_capacity(translations.len());
    for item in translations {
        let text = item.get("text").and_then(Value::as_str).ok_or_else(|| {
            TranslateError::InvalidResponse("DeepL translation entry missing `text`".into())
        })?;
        out.push(text.to_string());
    }
    Ok(out)
}

fn translate_libretranslate(
    settings: &ResolvedTranslateSettings,
    texts: &[String],
) -> Result<Vec<String>, TranslateError> {
    let endpoint = settings
        .endpoint
        .clone()
        .unwrap_or_else(|| "https://libretranslate.com/translate".to_string());
    let source = settings
        .source_lang
        .clone()
        .unwrap_or_else(|| "auto".to_string())
        .to_lowercase();
    let target = settings.target_lang.to_lowercase();
    let mut payload = json!({
        "q": texts,
        "source": source,
        "target": target,
        "format": "text",
    });
    if let Some(api_key) = &settings.api_key {
        payload["api_key"] = Value::String(api_key.clone());
    }

    let client = Client::new();
    let response = client
        .post(endpoint)
        .header(CONTENT_TYPE, "application/json")
        .json(&payload)
        .send()
        .map_err(|e| TranslateError::Http(format!("LibreTranslate request failed: {e}")))?;

    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .unwrap_or_else(|_| "<unable to read response body>".to_string());
        return Err(TranslateError::Http(format!(
            "LibreTranslate request failed with status {status}: {body}"
        )));
    }

    let value: Value = response.json().map_err(|e| {
        TranslateError::InvalidResponse(format!("LibreTranslate JSON parse failed: {e}"))
    })?;
    parse_libretranslate_translations(&value)
}

fn parse_libretranslate_translations(value: &Value) -> Result<Vec<String>, TranslateError> {
    if let Some(text) = value.get("translatedText").and_then(Value::as_str) {
        return Ok(vec![text.to_string()]);
    }
    if let Some(texts) = value.get("translatedText").and_then(Value::as_array) {
        let mut out = Vec::with_capacity(texts.len());
        for text in texts {
            let text = text.as_str().ok_or_else(|| {
                TranslateError::InvalidResponse(
                    "LibreTranslate `translatedText` array item must be a string".into(),
                )
            })?;
            out.push(text.to_string());
        }
        return Ok(out);
    }
    if let Some(entries) = value.get("translations").and_then(Value::as_array) {
        let mut out = Vec::with_capacity(entries.len());
        for entry in entries {
            let text = entry
                .get("translatedText")
                .or_else(|| entry.get("text"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    TranslateError::InvalidResponse(
                        "LibreTranslate `translations` entry missing text".into(),
                    )
                })?;
            out.push(text.to_string());
        }
        return Ok(out);
    }
    if let Some(entries) = value.as_array() {
        let mut out = Vec::with_capacity(entries.len());
        for entry in entries {
            let text = entry
                .get("translatedText")
                .or_else(|| entry.get("text"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    TranslateError::InvalidResponse(
                        "LibreTranslate array item missing translation text".into(),
                    )
                })?;
            out.push(text.to_string());
        }
        return Ok(out);
    }
    Err(TranslateError::InvalidResponse(format!(
        "Unsupported LibreTranslate response shape: {}",
        value
    )))
}

#[cfg(test)]
mod tests {
    use super::{
        SpanReplacement, TranslateError, apply_replacements, collect_translatable_spans,
        contains_translatable_chars, parse_deepl_translations, parse_libretranslate_translations,
    };
    use serde_json::json;

    #[test]
    fn parse_deepl_translations_accepts_expected_shape() {
        let value = json!({
            "translations": [
                {"text": "Bonjour"},
                {"text": "Monde"}
            ]
        });
        let out = parse_deepl_translations(&value).expect("deepl response should parse");
        assert_eq!(out, vec!["Bonjour".to_string(), "Monde".to_string()]);
    }

    #[test]
    fn parse_libretranslate_translations_accepts_array_shape() {
        let value = json!([
            {"translatedText": "Bonjour"},
            {"translatedText": "Monde"}
        ]);
        let out = parse_libretranslate_translations(&value)
            .expect("libretranslate response should parse");
        assert_eq!(out, vec!["Bonjour".to_string(), "Monde".to_string()]);
    }

    #[test]
    fn parse_libretranslate_translations_accepts_translated_text_array_shape() {
        let value = json!({
            "translatedText": ["Bonjour", "Monde"]
        });
        let out = parse_libretranslate_translations(&value)
            .expect("libretranslate translatedText array should parse");
        assert_eq!(out, vec!["Bonjour".to_string(), "Monde".to_string()]);
    }

    #[test]
    fn parse_libretranslate_translations_accepts_translations_key_shape() {
        let value = json!({
            "translations": [
                {"text": "Bonjour"},
                {"translatedText": "Monde"}
            ]
        });
        let out = parse_libretranslate_translations(&value)
            .expect("libretranslate translations array should parse");
        assert_eq!(out, vec!["Bonjour".to_string(), "Monde".to_string()]);
    }

    #[test]
    fn parse_libretranslate_translations_rejects_invalid_shape() {
        let value = json!({"unexpected": true});
        let err = parse_libretranslate_translations(&value).expect_err("invalid shape should fail");
        assert!(matches!(err, TranslateError::InvalidResponse(_)));
    }

    #[test]
    fn contains_translatable_chars_requires_alphabetic_content() {
        assert!(!contains_translatable_chars("12345"));
        assert!(contains_translatable_chars("Hello"));
        assert!(!contains_translatable_chars("@test.qmd"));
        assert!(!contains_translatable_chars("docs/index.qmd"));
        assert!(!contains_translatable_chars("https://example.com/page"));
    }

    #[test]
    fn collects_yaml_scalar_content_without_indentation() {
        use crate::config::Config;
        use crate::parser::parse;

        let input =
            "---\ntitle: Hello World\ndescription: >\n  Multi line text\n---\n\nParagraph.\n";
        let tree = parse(input, Some(Config::default()));
        let spans = collect_translatable_spans(input, &tree);

        assert!(
            spans.iter().any(|s| s.text.contains("Hello World")),
            "YAML scalar values should be collected"
        );
        assert!(
            spans.iter().any(|s| s.text == "Multi line text"),
            "Folded YAML block text should be collected without indentation"
        );
        assert!(
            spans.iter().any(|s| s.text == "Paragraph."),
            "Body paragraph text should still be translated"
        );
    }

    #[test]
    fn yaml_replacement_preserves_indentation() {
        let mut output = "---\ndescription: >\n  Multi line text\n---\n".to_string();
        let start = output.find("Multi line text").expect("text exists");
        let end = start + "Multi line text".len();
        let spans = vec![SpanReplacement {
            start,
            end,
            text: "Multi line text".to_string(),
            continuation_indent: Some("  ".to_string()),
        }];
        let translated = vec!["Texte multi ligne".to_string()];
        apply_replacements(&mut output, &spans, &translated).expect("replacement should succeed");
        assert_eq!(output, "---\ndescription: >\n  Texte multi ligne\n---\n");
    }

    #[test]
    fn yaml_replacement_reindents_multiline_translations() {
        let mut output = "---\ndescription: >\n  Multi line text\n---\n".to_string();
        let start = output.find("Multi line text").expect("text exists");
        let end = start + "Multi line text".len();
        let spans = vec![SpanReplacement {
            start,
            end,
            text: "Multi line text".to_string(),
            continuation_indent: Some("  ".to_string()),
        }];
        let translated = vec!["Ligne une\nLigne deux".to_string()];
        apply_replacements(&mut output, &spans, &translated).expect("replacement should succeed");
        assert_eq!(
            output,
            "---\ndescription: >\n  Ligne une\n  Ligne deux\n---\n"
        );
    }

    #[test]
    fn yaml_inline_value_is_collected_without_key_prefix() {
        use crate::config::Config;
        use crate::parser::parse;

        let input = "---\ntitle: Hello World\npath: @test.qmd\n---\n";
        let tree = parse(input, Some(Config::default()));
        let spans = collect_translatable_spans(input, &tree);

        assert!(spans.iter().any(|s| s.text == "Hello World"));
        assert!(!spans.iter().any(|s| s.text.contains("title:")));
        assert!(!spans.iter().any(|s| s.text.contains("@test.qmd")));
    }
}
