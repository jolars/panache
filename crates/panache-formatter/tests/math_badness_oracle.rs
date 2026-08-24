//! Byte-exact Badness output oracle for Panache's experimental math formatter.
//!
//! Badness formats complete LaTeX documents, whereas Panache's math entry point
//! receives a delimiter-free body. These test-only adapters place the same body
//! in controlled inline, display, and environment contexts, then mechanically
//! remove the wrappers. They do not parse or normalize the resulting TeX.

use badness_formatter::{FormatStyle, LineEnding, MathWrap, formatter::format_with_style};
use panache_formatter::formatter::math::{MathContext, MathFormatOptions, format_math};

#[derive(Debug, Clone, Copy)]
enum OracleContext {
    Inline,
    Display,
    Environment,
}

impl OracleContext {
    const ALL: [Self; 3] = [Self::Inline, Self::Display, Self::Environment];

    fn wrapper(self, body: &str) -> (String, &'static str, &'static str) {
        match self {
            Self::Inline => (format!("${body}$\n"), "$", "$\n"),
            Self::Display => (format!("\\[\n{body}\n\\]\n"), "\\[\n", "\n\\]\n"),
            Self::Environment => (
                format!("\\begin{{aligned}}\n{body}\n\\end{{aligned}}\n"),
                "\\begin{aligned}\n",
                "\n\\end{aligned}\n",
            ),
        }
    }

    fn panache_context(self) -> MathContext {
        match self {
            Self::Inline => MathContext::Inline,
            Self::Display => MathContext::Display,
            Self::Environment => MathContext::EnvironmentBody,
        }
    }
}

fn badness_body(body: &str, context: OracleContext) -> Result<String, String> {
    let (wrapped, prefix, suffix) = context.wrapper(body);
    let formatted = format_with_style(
        &wrapped,
        FormatStyle {
            line_width: 80,
            indent_width: 2,
            math_wrap: MathWrap::Break,
            line_ending: LineEnding::Lf,
            ..FormatStyle::default()
        },
    )
    .map_err(|error| format!("Badness rejected {context:?} wrapper: {error}"))?;

    let body = formatted
        .strip_prefix(prefix)
        .and_then(|rest| rest.strip_suffix(suffix))
        .ok_or_else(|| {
            format!("Badness changed the controlled {context:?} wrapper shape:\n{formatted:?}")
        })?;
    Ok(body.to_owned())
}

fn panache_body(body: &str, context: OracleContext) -> Result<String, String> {
    format_math(
        body,
        &MathFormatOptions {
            enabled: true,
            math_indent: 2,
            line_width: 80,
            bookdown_equation_labels: false,
            context: context.panache_context(),
        },
    )
    .ok_or_else(|| format!("Panache declined {context:?} body"))
}

fn assert_formatter_parity(body: &str, context: OracleContext) {
    let badness = badness_body(body, context).unwrap_or_else(|error| panic!("{error}"));
    let panache = panache_body(body, context).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        panache, badness,
        "formatter parity failed in {context:?} context"
    );
}

#[test]
fn oracle_extracts_bodies_from_all_controlled_contexts() {
    let expected = ["a + b", "  a + b", "  a + b"];
    for (context, expected) in OracleContext::ALL.into_iter().zip(expected) {
        assert_eq!(badness_body("a+b", context).as_deref(), Ok(expected));
    }
}

#[test]
fn oracle_compares_formatter_output_byte_for_byte() {
    for context in OracleContext::ALL {
        assert_formatter_parity("a+b", context);
    }
}
