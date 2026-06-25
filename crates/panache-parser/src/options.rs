use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// The flavor of Markdown to parse and format.
/// Each flavor has a different set of default extensions enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum Flavor {
    /// Standard Pandoc Markdown (default extensions enabled)
    #[default]
    Pandoc,
    /// Quarto (Pandoc + Quarto-specific extensions)
    Quarto,
    /// R Markdown (Pandoc + R-specific extensions)
    #[cfg_attr(feature = "serde", serde(rename = "rmarkdown"))]
    RMarkdown,
    /// GitHub Flavored Markdown
    Gfm,
    /// CommonMark
    #[cfg_attr(feature = "serde", serde(alias = "commonmark"))]
    CommonMark,
    /// MultiMarkdown
    #[cfg_attr(feature = "serde", serde(rename = "multimarkdown"))]
    MultiMarkdown,
    /// mdsvex (Svelte-flavored Markdown: CommonMark + Svelte template syntax)
    #[cfg_attr(feature = "serde", serde(rename = "mdsvex"))]
    Mdsvex,
}

/// Pandoc/Markdown extensions configuration.
/// Each field represents a specific Pandoc extension.
/// Extensions marked with a comment indicate implementation status.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Extensions {
    // ===== Block-level extensions =====

    // Headings
    /// Require blank line before headers (default: enabled)
    #[cfg_attr(feature = "serde", serde(alias = "blank_before_header"))]
    pub blank_before_header: bool,
    /// Full attribute syntax on headers {#id .class key=value}
    #[cfg_attr(feature = "serde", serde(alias = "header_attributes"))]
    pub header_attributes: bool,
    /// Auto-generate identifiers from headings
    pub auto_identifiers: bool,
    /// Use GitHub's algorithm for auto-generated heading identifiers
    pub gfm_auto_identifiers: bool,
    /// Implicit header references ([Heading] links to header)
    pub implicit_header_references: bool,

    // Block quotes
    /// Require blank line before blockquotes (default: enabled)
    #[cfg_attr(feature = "serde", serde(alias = "blank_before_blockquote"))]
    pub blank_before_blockquote: bool,

    // Lists
    /// Fancy list markers (roman numerals, letters, etc.)
    #[cfg_attr(feature = "serde", serde(alias = "fancy_lists"))]
    pub fancy_lists: bool,
    /// Start ordered lists at arbitrary numbers
    pub startnum: bool,
    /// Example lists with (@) markers
    #[cfg_attr(feature = "serde", serde(alias = "example_lists"))]
    pub example_lists: bool,
    /// GitHub-style task lists - [ ] and - [x]
    #[cfg_attr(feature = "serde", serde(alias = "task_lists"))]
    pub task_lists: bool,
    /// Term/definition syntax
    #[cfg_attr(feature = "serde", serde(alias = "definition_lists"))]
    pub definition_lists: bool,
    /// Allow lists without a preceding blank line
    #[cfg_attr(feature = "serde", serde(alias = "lists_without_preceding_blankline"))]
    pub lists_without_preceding_blankline: bool,
    /// [NON-DEFAULT] Pandoc <= 2.0 list semantics: continuation paragraphs and
    /// nested lists require four-space (one tab-width) indentation
    #[cfg_attr(feature = "serde", serde(alias = "four_space_rule"))]
    pub four_space_rule: bool,

    // Code blocks
    /// Fenced code blocks with backticks
    #[cfg_attr(feature = "serde", serde(alias = "backtick_code_blocks"))]
    pub backtick_code_blocks: bool,
    /// Fenced code blocks with tildes
    #[cfg_attr(feature = "serde", serde(alias = "fenced_code_blocks"))]
    pub fenced_code_blocks: bool,
    /// Attributes on fenced code blocks {.language #id}
    #[cfg_attr(feature = "serde", serde(alias = "fenced_code_attributes"))]
    pub fenced_code_attributes: bool,
    /// Executable code syntax (currently fenced chunks like ```{r} / ```{python})
    pub executable_code: bool,
    /// R Markdown inline executable code (`...`r ...)
    pub rmarkdown_inline_code: bool,
    /// Quarto inline executable code (`...`{r} ...)
    pub quarto_inline_code: bool,
    /// Attributes on inline code
    #[cfg_attr(feature = "serde", serde(alias = "inline_code_attributes"))]
    pub inline_code_attributes: bool,

    // Tables
    /// Simple table syntax
    #[cfg_attr(feature = "serde", serde(alias = "simple_tables"))]
    pub simple_tables: bool,
    /// Multiline cell content in tables
    #[cfg_attr(feature = "serde", serde(alias = "multiline_tables"))]
    pub multiline_tables: bool,
    /// Grid-style tables
    #[cfg_attr(feature = "serde", serde(alias = "grid_tables"))]
    pub grid_tables: bool,
    /// Pipe tables (GitHub/PHP Markdown style)
    #[cfg_attr(feature = "serde", serde(alias = "pipe_tables"))]
    pub pipe_tables: bool,
    /// Table captions
    #[cfg_attr(feature = "serde", serde(alias = "table_captions"))]
    pub table_captions: bool,

    // Divs
    /// Fenced divs ::: {.class}
    #[cfg_attr(feature = "serde", serde(alias = "fenced_divs"))]
    pub fenced_divs: bool,
    /// HTML <div> elements
    #[cfg_attr(feature = "serde", serde(alias = "native_divs"))]
    pub native_divs: bool,

    // Other block elements
    /// Line blocks for poetry | prefix
    #[cfg_attr(feature = "serde", serde(alias = "line_blocks"))]
    pub line_blocks: bool,

    // ===== Inline elements =====

    // Emphasis
    /// Underscores don't trigger emphasis in snake_case
    #[cfg_attr(feature = "serde", serde(alias = "intraword_underscores"))]
    pub intraword_underscores: bool,
    /// Strikethrough ~~text~~
    pub strikeout: bool,
    /// Superscript and subscript ^super^ ~sub~
    pub superscript: bool,
    pub subscript: bool,

    // Links
    /// Inline links [text](url)
    #[cfg_attr(feature = "serde", serde(alias = "inline_links"))]
    pub inline_links: bool,
    /// Reference links [text][ref]
    #[cfg_attr(feature = "serde", serde(alias = "reference_links"))]
    pub reference_links: bool,
    /// Shortcut reference links [ref] without second []
    #[cfg_attr(feature = "serde", serde(alias = "shortcut_reference_links"))]
    pub shortcut_reference_links: bool,
    /// Attributes on links [text](url){.class}
    #[cfg_attr(feature = "serde", serde(alias = "link_attributes"))]
    pub link_attributes: bool,
    /// Automatic links <http://example.com>
    pub autolinks: bool,

    // Images
    /// Inline images ![alt](url)
    #[cfg_attr(feature = "serde", serde(alias = "inline_images"))]
    pub inline_images: bool,
    /// Paragraph with just image becomes figure
    #[cfg_attr(feature = "serde", serde(alias = "implicit_figures"))]
    pub implicit_figures: bool,

    // Math
    /// Dollar-delimited math $x$ and $$equation$$
    #[cfg_attr(feature = "serde", serde(alias = "tex_math_dollars"))]
    pub tex_math_dollars: bool,
    /// [NON-DEFAULT] GFM math: inline $`...`$ and fenced ``` math blocks
    #[cfg_attr(feature = "serde", serde(alias = "tex_math_gfm"))]
    pub tex_math_gfm: bool,
    /// [NON-DEFAULT] Single backslash math \(...\) and \[...\] (RMarkdown default)
    #[cfg_attr(feature = "serde", serde(alias = "tex_math_single_backslash"))]
    pub tex_math_single_backslash: bool,
    /// [NON-DEFAULT] Double backslash math \\(...\\) and \\[...\\]
    #[cfg_attr(feature = "serde", serde(alias = "tex_math_double_backslash"))]
    pub tex_math_double_backslash: bool,

    // Footnotes
    /// Inline footnotes ^[text]
    #[cfg_attr(feature = "serde", serde(alias = "inline_footnotes"))]
    pub inline_footnotes: bool,
    /// Reference footnotes `[^1]` (requires footnote parsing)
    pub footnotes: bool,

    // Citations
    /// Citation syntax [@cite]
    pub citations: bool,

    // Spans
    /// Bracketed spans [text]{.class}
    #[cfg_attr(feature = "serde", serde(alias = "bracketed_spans"))]
    pub bracketed_spans: bool,
    /// HTML <span> elements
    #[cfg_attr(feature = "serde", serde(alias = "native_spans"))]
    pub native_spans: bool,

    // ===== Metadata =====
    /// YAML metadata block
    #[cfg_attr(feature = "serde", serde(alias = "yaml_metadata_block"))]
    pub yaml_metadata_block: bool,
    /// Pandoc title block (Title/Author/Date)
    #[cfg_attr(feature = "serde", serde(alias = "pandoc_title_block"))]
    pub pandoc_title_block: bool,
    /// [NON-DEFAULT] MultiMarkdown metadata/title block (Key: Value ...)
    pub mmd_title_block: bool,

    // ===== Raw content =====
    /// Raw HTML blocks and inline
    #[cfg_attr(feature = "serde", serde(alias = "raw_html"))]
    pub raw_html: bool,
    /// Markdown inside HTML blocks
    #[cfg_attr(feature = "serde", serde(alias = "markdown_in_html_blocks"))]
    pub markdown_in_html_blocks: bool,
    /// LaTeX commands and environments
    #[cfg_attr(feature = "serde", serde(alias = "raw_tex"))]
    pub raw_tex: bool,
    /// Generic raw blocks with {=format} syntax
    #[cfg_attr(feature = "serde", serde(alias = "raw_attribute"))]
    pub raw_attribute: bool,

    // ===== Escapes and special characters =====
    /// Backslash escapes any symbol
    #[cfg_attr(feature = "serde", serde(alias = "all_symbols_escapable"))]
    pub all_symbols_escapable: bool,
    /// Backslash at line end = hard line break
    #[cfg_attr(feature = "serde", serde(alias = "escaped_line_breaks"))]
    pub escaped_line_breaks: bool,

    // ===== NON-DEFAULT EXTENSIONS =====
    // These are disabled by default in Pandoc
    /// [NON-DEFAULT] Bare URLs become links
    #[cfg_attr(feature = "serde", serde(alias = "autolink_bare_uris"))]
    pub autolink_bare_uris: bool,
    /// [NON-DEFAULT] Newline = <br>
    #[cfg_attr(feature = "serde", serde(alias = "hard_line_breaks"))]
    pub hard_line_breaks: bool,
    /// [NON-DEFAULT] Ignore soft breaks between two East Asian wide characters
    #[cfg_attr(feature = "serde", serde(alias = "east_asian_line_breaks"))]
    pub east_asian_line_breaks: bool,
    /// [NON-DEFAULT] MultiMarkdown style heading identifiers [my-id]
    pub mmd_header_identifiers: bool,
    /// [NON-DEFAULT] MultiMarkdown key=value attributes on reference defs
    pub mmd_link_attributes: bool,
    /// [NON-DEFAULT] GitHub/CommonMark alerts in blockquotes (`> [!NOTE]`)
    pub alerts: bool,
    /// [NON-DEFAULT] python-markdown admonitions (`!!! note`) with 4-space
    /// indented content
    #[cfg_attr(feature = "serde", serde(alias = "python_markdown_admonitions"))]
    pub python_markdown_admonitions: bool,
    /// [NON-DEFAULT] pymdownx details: collapsible admonitions (`???`/`???+`)
    #[cfg_attr(feature = "serde", serde(alias = "pymdownx_details"))]
    pub pymdownx_details: bool,
    /// [NON-DEFAULT] :emoji: syntax
    pub emoji: bool,
    /// [NON-DEFAULT] Highlighted ==text==
    pub mark: bool,
    /// [NON-DEFAULT] Pandoc wikilinks with title after pipe: `[[url|title]]`
    #[cfg_attr(feature = "serde", serde(alias = "wikilinks_title_after_pipe"))]
    pub wikilinks_title_after_pipe: bool,
    /// [NON-DEFAULT] Pandoc wikilinks with title before pipe: `[[title|url]]`
    #[cfg_attr(feature = "serde", serde(alias = "wikilinks_title_before_pipe"))]
    pub wikilinks_title_before_pipe: bool,
    /// [NON-DEFAULT] Allow whitespace between reference link brackets: `[foo] [bar]`
    #[cfg_attr(feature = "serde", serde(alias = "spaced_reference_links"))]
    pub spaced_reference_links: bool,

    // ===== Quarto-specific extensions =====
    /// Quarto callout blocks (.callout-note, etc.)
    #[cfg_attr(feature = "serde", serde(alias = "quarto_callouts"))]
    pub quarto_callouts: bool,
    /// Quarto cross-references @fig-id, @tbl-id
    #[cfg_attr(feature = "serde", serde(alias = "quarto_crossrefs"))]
    pub quarto_crossrefs: bool,
    /// Quarto shortcodes {{< name args >}}
    #[cfg_attr(feature = "serde", serde(alias = "quarto_shortcodes"))]
    pub quarto_shortcodes: bool,
    /// Bookdown references \@ref(label) and (\#label)
    pub bookdown_references: bool,
    /// Bookdown equation references in LaTeX math blocks (\#eq:label)
    pub bookdown_equation_references: bool,

    // ===== mdsvex-specific extensions =====
    /// [NON-DEFAULT] Svelte template syntax: `{#if}`/`{:else}`/`{/each}` block
    /// logic, `{@html ...}`/`{@const ...}` tags, and `{expr}` interpolation.
    /// Parsed as opaque, lossless spans (content preserved verbatim).
    #[cfg_attr(feature = "serde", serde(alias = "svelte_template"))]
    pub svelte_template: bool,
}

impl Default for Extensions {
    fn default() -> Self {
        Self::for_flavor(Flavor::default())
    }
}

impl Extensions {
    fn none_defaults() -> Self {
        Self {
            alerts: false,
            all_symbols_escapable: false,
            auto_identifiers: false,
            autolink_bare_uris: false,
            autolinks: false,
            backtick_code_blocks: false,
            blank_before_blockquote: false,
            blank_before_header: false,
            bookdown_references: false,
            bookdown_equation_references: false,
            bracketed_spans: false,
            citations: false,
            definition_lists: false,
            lists_without_preceding_blankline: false,
            emoji: false,
            escaped_line_breaks: false,
            example_lists: false,
            executable_code: false,
            rmarkdown_inline_code: false,
            quarto_inline_code: false,
            fancy_lists: false,
            fenced_code_attributes: false,
            fenced_code_blocks: false,
            fenced_divs: false,
            footnotes: false,
            four_space_rule: false,
            gfm_auto_identifiers: false,
            grid_tables: false,
            east_asian_line_breaks: false,
            hard_line_breaks: false,
            header_attributes: false,
            implicit_figures: false,
            implicit_header_references: false,
            inline_code_attributes: false,
            inline_footnotes: false,
            inline_images: false,
            inline_links: false,
            intraword_underscores: false,
            line_blocks: false,
            link_attributes: false,
            mark: false,
            markdown_in_html_blocks: false,
            mmd_header_identifiers: false,
            mmd_link_attributes: false,
            mmd_title_block: false,
            multiline_tables: false,
            native_divs: false,
            native_spans: false,
            pandoc_title_block: false,
            pipe_tables: false,
            python_markdown_admonitions: false,
            pymdownx_details: false,
            quarto_callouts: false,
            quarto_crossrefs: false,
            quarto_shortcodes: false,
            raw_attribute: false,
            raw_html: false,
            raw_tex: false,
            reference_links: false,
            shortcut_reference_links: false,
            simple_tables: false,
            startnum: false,
            strikeout: false,
            subscript: false,
            superscript: false,
            svelte_template: false,
            table_captions: false,
            task_lists: false,
            tex_math_dollars: false,
            tex_math_double_backslash: false,
            tex_math_gfm: false,
            tex_math_single_backslash: false,
            wikilinks_title_after_pipe: false,
            wikilinks_title_before_pipe: false,
            spaced_reference_links: false,
            yaml_metadata_block: false,
        }
    }

    /// Get the default extension set for a given flavor.
    pub fn for_flavor(flavor: Flavor) -> Self {
        match flavor {
            Flavor::Pandoc => Self::pandoc_defaults(),
            Flavor::Quarto => Self::quarto_defaults(),
            Flavor::RMarkdown => Self::rmarkdown_defaults(),
            Flavor::Gfm => Self::gfm_defaults(),
            Flavor::CommonMark => Self::commonmark_defaults(),
            Flavor::MultiMarkdown => Self::multimarkdown_defaults(),
            Flavor::Mdsvex => Self::mdsvex_defaults(),
        }
    }

    fn pandoc_defaults() -> Self {
        Self {
            // Block-level - enabled by default in Pandoc
            auto_identifiers: true,
            blank_before_blockquote: true,
            blank_before_header: true,
            gfm_auto_identifiers: false,
            header_attributes: true,
            implicit_header_references: true,

            // Lists
            definition_lists: true,
            example_lists: true,
            fancy_lists: true,
            lists_without_preceding_blankline: false,
            startnum: true,
            task_lists: true,

            // Code
            backtick_code_blocks: true,
            executable_code: false,
            rmarkdown_inline_code: false,
            quarto_inline_code: false,
            fenced_code_attributes: true,
            fenced_code_blocks: true,
            inline_code_attributes: true,

            // Tables
            grid_tables: true,
            multiline_tables: true,
            pipe_tables: true,
            simple_tables: true,
            table_captions: true,

            // Divs
            fenced_divs: true,
            native_divs: true,

            // Other blocks
            line_blocks: true,

            // Inline
            intraword_underscores: true,
            strikeout: true,
            subscript: true,
            superscript: true,

            // Links
            autolinks: true,
            inline_links: true,
            link_attributes: true,
            reference_links: true,
            shortcut_reference_links: true,

            // Images
            implicit_figures: true,
            inline_images: true,

            // Math
            tex_math_dollars: true,
            tex_math_double_backslash: false,
            tex_math_gfm: false,
            tex_math_single_backslash: false,

            // Footnotes
            footnotes: true,
            inline_footnotes: true,

            // Citations
            citations: true,

            // Spans
            bracketed_spans: true,
            native_spans: true,

            // Metadata
            mmd_title_block: false,
            pandoc_title_block: true,
            yaml_metadata_block: true,

            // Raw
            markdown_in_html_blocks: false,
            raw_attribute: true,
            raw_html: true,
            raw_tex: true,

            // Escapes
            all_symbols_escapable: true,
            escaped_line_breaks: true,

            // Non-default
            alerts: false,
            python_markdown_admonitions: false,
            pymdownx_details: false,
            autolink_bare_uris: false,
            east_asian_line_breaks: false,
            emoji: false,
            four_space_rule: false,
            hard_line_breaks: false,
            mark: false,
            mmd_header_identifiers: false,
            mmd_link_attributes: false,

            // Quarto/Bookdown-specific
            bookdown_references: false,
            bookdown_equation_references: false,
            quarto_callouts: false,
            quarto_crossrefs: false,
            quarto_shortcodes: false,

            // Wikilinks (opt-in, no flavor default)
            wikilinks_title_after_pipe: false,
            wikilinks_title_before_pipe: false,

            // Spaced reference links (opt-in)
            spaced_reference_links: false,

            // mdsvex (opt-in, mdsvex flavor only)
            svelte_template: false,
        }
    }

    fn quarto_defaults() -> Self {
        let mut ext = Self::pandoc_defaults();

        ext.executable_code = true;
        ext.rmarkdown_inline_code = true;
        ext.quarto_inline_code = true;
        ext.quarto_callouts = true;
        ext.quarto_crossrefs = true;
        ext.quarto_shortcodes = true;

        ext
    }

    fn rmarkdown_defaults() -> Self {
        let mut ext = Self::pandoc_defaults();

        ext.bookdown_references = true;
        ext.bookdown_equation_references = true;
        ext.executable_code = true;
        ext.rmarkdown_inline_code = true;
        ext.quarto_inline_code = false;
        ext.tex_math_dollars = true;
        ext.tex_math_single_backslash = true;

        ext
    }

    fn gfm_defaults() -> Self {
        let mut ext = Self::none_defaults();

        ext.alerts = true;
        ext.auto_identifiers = true;
        ext.autolink_bare_uris = true;
        ext.autolinks = true;
        ext.backtick_code_blocks = true;
        ext.emoji = true;
        ext.fenced_code_blocks = true;
        ext.footnotes = true;
        ext.gfm_auto_identifiers = true;
        ext.inline_images = true;
        ext.inline_links = true;
        ext.pipe_tables = true;
        ext.raw_html = true;
        ext.reference_links = true;
        ext.shortcut_reference_links = true;
        ext.strikeout = true;
        ext.task_lists = true;
        ext.tex_math_dollars = true;
        ext.tex_math_gfm = true;
        ext.yaml_metadata_block = true;

        ext
    }

    fn commonmark_defaults() -> Self {
        let mut ext = Self::none_defaults();
        // CommonMark's core grammar is what pandoc's commonmark reader treats
        // as "not extensions" — they're built into the reader. Panache's
        // parser still gates each construct on its extension flag, so we have
        // to enable the CommonMark-mandatory ones explicitly here.
        //
        // Notably absent: `all_symbols_escapable`. CommonMark only allows
        // backslash escapes of ASCII punctuation, and panache's
        // `all_symbols_escapable` flag widens that to any character — so it
        // must stay off for CommonMark.
        ext.autolinks = true;
        ext.backtick_code_blocks = true;
        ext.escaped_line_breaks = true;
        ext.fenced_code_blocks = true;
        ext.inline_images = true;
        ext.inline_links = true;
        ext.intraword_underscores = true;
        ext.raw_html = true;
        ext.reference_links = true;
        ext.shortcut_reference_links = true;
        ext
    }

    fn multimarkdown_defaults() -> Self {
        let mut ext = Self::none_defaults();

        ext.all_symbols_escapable = true;
        ext.auto_identifiers = true;
        ext.backtick_code_blocks = true;
        ext.definition_lists = true;
        ext.footnotes = true;
        ext.implicit_figures = true;
        ext.implicit_header_references = true;
        ext.intraword_underscores = true;
        ext.mmd_header_identifiers = true;
        ext.mmd_link_attributes = true;
        ext.mmd_title_block = true;
        ext.pipe_tables = true;
        ext.raw_attribute = true;
        ext.raw_html = true;
        ext.reference_links = true;
        ext.shortcut_reference_links = true;
        ext.subscript = true;
        ext.superscript = true;
        ext.tex_math_dollars = true;
        ext.tex_math_double_backslash = true;

        ext
    }

    fn mdsvex_defaults() -> Self {
        // mdsvex (≤0.12.x) builds on `remark-parse@8`, the pre-micromark parser
        // whose options default to `gfm: true`. So tables, strikethrough, bare
        // autolinks, and task lists work out of the box with **no** plugins —
        // confirmed by the official "Svex up your markdown" getting-started
        // (which uses a pipe table) and by real plugin-free `svelte.config.js`
        // setups. `remark-gfm` is only needed on *modern* remark, which mdsvex
        // does not use. A CommonMark-only default would wrongly reflow those
        // tables as prose.
        //
        // We therefore start from the GFM extension set (still the CommonMark
        // *dialect* per `Dialect::for_flavor`, and still with every Pandoc
        // attribute extension off, so `{` stays free for Svelte), then trim the
        // extras that remark-parse@8's `gfm: true` does NOT include: footnotes
        // (a separate, default-off option), math (needs remark-math), emoji
        // (needs remark-gemoji), and GitHub alerts (postdates this remark).
        let mut ext = Self::gfm_defaults();

        ext.footnotes = false;
        ext.tex_math_dollars = false;
        ext.tex_math_gfm = false;
        ext.emoji = false;
        ext.alerts = false;

        ext.svelte_template = true;

        ext
    }

    /// Merge user-specified extension overrides with flavor defaults.
    ///
    /// This is used to support partial extension overrides in config files.
    /// For example, if a user specifies `flavor = "quarto"` and then sets
    /// `[extensions] quarto-crossrefs = false`, we want all other extensions
    /// to use Quarto defaults, not Pandoc defaults.
    ///
    /// # Arguments
    /// * `user_overrides` - Map of extension names to their user-specified values
    /// * `flavor` - The flavor to use for default values
    ///
    /// # Returns
    /// A new Extensions struct with flavor defaults merged with user overrides
    pub fn merge_with_flavor(user_overrides: HashMap<String, bool>, flavor: Flavor) -> Self {
        let defaults = Self::for_flavor(flavor);
        Self::merge_overrides(defaults, user_overrides)
    }

    /// Apply `user_overrides` on top of an already-resolved `Extensions`.
    /// Unknown keys are silently ignored (mirrors the panache.toml loader).
    /// Use this when overriding individual extensions on top of a config that
    /// has already merged flavor defaults + file-based overrides (e.g. CLI
    /// `-o extensions.<name>=<bool>`).
    pub fn apply_overrides(&mut self, user_overrides: HashMap<String, bool>) {
        *self = Self::merge_overrides(self.clone(), user_overrides);
    }

    fn merge_overrides(mut base: Extensions, user_overrides: HashMap<String, bool>) -> Self {
        for (key, value) in user_overrides {
            base.set_by_name(&key, value);
        }
        base
    }
}

/// Define the canonical mapping between kebab-case extension names (as users
/// write them in `[extensions]`) and the corresponding `Extensions` fields.
/// Drives both the runtime setter and the public name list, so adding an
/// extension means editing exactly one table.
macro_rules! known_extensions {
    ( $( $kebab:literal => $field:ident ),* $(,)? ) => {
        impl Extensions {
            /// Canonical kebab-case names accepted in `[extensions]`. Used by
            /// the config loader's typo check and by the JSON Schema
            /// generator. Snake_case is also accepted at runtime via
            /// normalization in [`Extensions::set_by_name`].
            pub const KNOWN_NAMES: &'static [&'static str] = &[ $($kebab),* ];

            /// True if `name` (in either kebab- or snake-case) is a known
            /// extension key.
            pub fn is_known_name(name: &str) -> bool {
                let normalized = name.replace('_', "-");
                Self::KNOWN_NAMES.iter().any(|k| *k == normalized)
            }

            /// Set the named extension on `self`, returning `true` if `name`
            /// matched a known field. Kebab- and snake-case are accepted.
            fn set_by_name(&mut self, name: &str, value: bool) -> bool {
                match name.replace('_', "-").as_str() {
                    $( $kebab => { self.$field = value; true } )*
                    _ => false,
                }
            }
        }
    };
}

known_extensions! {
    "blank-before-header" => blank_before_header,
    "header-attributes" => header_attributes,
    "auto-identifiers" => auto_identifiers,
    "gfm-auto-identifiers" => gfm_auto_identifiers,
    "implicit-header-references" => implicit_header_references,
    "blank-before-blockquote" => blank_before_blockquote,
    "fancy-lists" => fancy_lists,
    "startnum" => startnum,
    "example-lists" => example_lists,
    "task-lists" => task_lists,
    "definition-lists" => definition_lists,
    "lists-without-preceding-blankline" => lists_without_preceding_blankline,
    "four-space-rule" => four_space_rule,
    "backtick-code-blocks" => backtick_code_blocks,
    "fenced-code-blocks" => fenced_code_blocks,
    "fenced-code-attributes" => fenced_code_attributes,
    "executable-code" => executable_code,
    "rmarkdown-inline-code" => rmarkdown_inline_code,
    "quarto-inline-code" => quarto_inline_code,
    "inline-code-attributes" => inline_code_attributes,
    "simple-tables" => simple_tables,
    "multiline-tables" => multiline_tables,
    "grid-tables" => grid_tables,
    "pipe-tables" => pipe_tables,
    "table-captions" => table_captions,
    "fenced-divs" => fenced_divs,
    "native-divs" => native_divs,
    "line-blocks" => line_blocks,
    "intraword-underscores" => intraword_underscores,
    "strikeout" => strikeout,
    "superscript" => superscript,
    "subscript" => subscript,
    "inline-links" => inline_links,
    "reference-links" => reference_links,
    "shortcut-reference-links" => shortcut_reference_links,
    "link-attributes" => link_attributes,
    "autolinks" => autolinks,
    "inline-images" => inline_images,
    "implicit-figures" => implicit_figures,
    "tex-math-dollars" => tex_math_dollars,
    "tex-math-gfm" => tex_math_gfm,
    "tex-math-single-backslash" => tex_math_single_backslash,
    "tex-math-double-backslash" => tex_math_double_backslash,
    "inline-footnotes" => inline_footnotes,
    "footnotes" => footnotes,
    "citations" => citations,
    "bracketed-spans" => bracketed_spans,
    "native-spans" => native_spans,
    "yaml-metadata-block" => yaml_metadata_block,
    "pandoc-title-block" => pandoc_title_block,
    "mmd-title-block" => mmd_title_block,
    "raw-html" => raw_html,
    "markdown-in-html-blocks" => markdown_in_html_blocks,
    "raw-tex" => raw_tex,
    "raw-attribute" => raw_attribute,
    "all-symbols-escapable" => all_symbols_escapable,
    "escaped-line-breaks" => escaped_line_breaks,
    "autolink-bare-uris" => autolink_bare_uris,
    "hard-line-breaks" => hard_line_breaks,
    "east-asian-line-breaks" => east_asian_line_breaks,
    "mmd-header-identifiers" => mmd_header_identifiers,
    "mmd-link-attributes" => mmd_link_attributes,
    "alerts" => alerts,
    "python-markdown-admonitions" => python_markdown_admonitions,
    "pymdownx-details" => pymdownx_details,
    "emoji" => emoji,
    "mark" => mark,
    "quarto-callouts" => quarto_callouts,
    "quarto-crossrefs" => quarto_crossrefs,
    "quarto-shortcodes" => quarto_shortcodes,
    "bookdown-references" => bookdown_references,
    "bookdown-equation-references" => bookdown_equation_references,
    "wikilinks-title-after-pipe" => wikilinks_title_after_pipe,
    "wikilinks-title-before-pipe" => wikilinks_title_before_pipe,
    "spaced-reference-links" => spaced_reference_links,
    "svelte-template" => svelte_template,
}

#[cfg(test)]
mod tests {
    use super::{Dialect, Extensions, Flavor};
    use std::collections::HashMap;

    #[test]
    fn merge_with_flavor_keeps_known_extension_overrides() {
        let mut overrides = HashMap::new();
        overrides.insert("intraword-underscores".to_string(), false);
        let ext = Extensions::merge_with_flavor(overrides, Flavor::Pandoc);
        assert!(!ext.intraword_underscores);
    }

    #[test]
    fn merge_with_flavor_ignores_unknown_extension_overrides() {
        let mut overrides = HashMap::new();
        overrides.insert("smart".to_string(), true);
        overrides.insert("smart-quotes".to_string(), true);
        let ext = Extensions::merge_with_flavor(overrides, Flavor::Gfm);
        assert!(ext.strikeout, "known defaults should remain intact");
    }

    #[test]
    fn lists_without_preceding_blankline_defaults_false_for_pandoc_and_gfm() {
        assert!(!Extensions::for_flavor(Flavor::Pandoc).lists_without_preceding_blankline);
        assert!(!Extensions::for_flavor(Flavor::Gfm).lists_without_preceding_blankline);
    }

    #[test]
    fn merge_with_flavor_accepts_lists_without_preceding_blankline_override() {
        let mut overrides = HashMap::new();
        overrides.insert("lists-without-preceding-blankline".to_string(), true);
        let ext = Extensions::merge_with_flavor(overrides, Flavor::Pandoc);
        assert!(ext.lists_without_preceding_blankline);
    }

    #[test]
    fn four_space_rule_defaults_off_for_every_flavor() {
        for flavor in [
            Flavor::Pandoc,
            Flavor::Quarto,
            Flavor::RMarkdown,
            Flavor::Gfm,
            Flavor::CommonMark,
            Flavor::MultiMarkdown,
            Flavor::Mdsvex,
        ] {
            assert!(
                !Extensions::for_flavor(flavor).four_space_rule,
                "four_space_rule should be off by default for {flavor:?}"
            );
        }
    }

    #[test]
    fn svelte_template_defaults_off_for_non_mdsvex_flavors() {
        for flavor in [
            Flavor::Pandoc,
            Flavor::Quarto,
            Flavor::RMarkdown,
            Flavor::Gfm,
            Flavor::CommonMark,
            Flavor::MultiMarkdown,
        ] {
            assert!(
                !Extensions::for_flavor(flavor).svelte_template,
                "svelte_template should be off by default for {flavor:?}"
            );
        }
    }

    #[test]
    fn mdsvex_defaults_match_remark_parse_8_gfm() {
        let ext = Extensions::for_flavor(Flavor::Mdsvex);

        // The Svelte template layer is on; raw HTML and frontmatter pass through.
        assert!(ext.svelte_template);
        assert!(ext.raw_html);
        assert!(ext.yaml_metadata_block);

        // remark-parse@8 defaults to `gfm: true`, so these work with no plugins.
        assert!(ext.pipe_tables);
        assert!(ext.strikeout);
        assert!(ext.task_lists);
        assert!(ext.autolink_bare_uris);

        // Extras that remark-parse@8's gfm does NOT include stay off.
        assert!(!ext.footnotes);
        assert!(!ext.tex_math_dollars);
        assert!(!ext.emoji);
        assert!(!ext.alerts);

        // CommonMark dialect, so the Pandoc `{...}` attribute constructs are off
        // — this is what frees `{` for Svelte template syntax.
        assert_eq!(Dialect::for_flavor(Flavor::Mdsvex), Dialect::CommonMark);
        assert!(!ext.header_attributes);
        assert!(!ext.bracketed_spans);
        assert!(!ext.fenced_divs);
        assert!(!ext.raw_attribute);
        assert!(!ext.inline_code_attributes);
    }

    #[test]
    fn merge_with_flavor_accepts_four_space_rule_override() {
        let mut overrides = HashMap::new();
        overrides.insert("four-space-rule".to_string(), true);
        let ext = Extensions::merge_with_flavor(overrides, Flavor::Pandoc);
        assert!(ext.four_space_rule);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PandocCompat {
    /// Alias for Panache's pinned newest supported Pandoc-compat behavior.
    ///
    /// This is intentionally NOT "floating upstream latest". It resolves to
    /// a concrete version that Panache has verified, and is bumped manually.
    #[cfg_attr(feature = "serde", serde(rename = "latest"))]
    Latest,
    /// Match Pandoc 3.7 behavior for ambiguous syntax edge cases.
    #[cfg_attr(
        feature = "serde",
        serde(rename = "3.7", alias = "3-7", alias = "v3.7", alias = "v3-7")
    )]
    V3_7,
    /// Match Pandoc 3.9 behavior for ambiguous syntax edge cases.
    #[default]
    #[cfg_attr(
        feature = "serde",
        serde(rename = "3.9", alias = "3-9", alias = "v3.9", alias = "v3-9")
    )]
    V3_9,
}

impl PandocCompat {
    /// Pinned target for `latest`.
    pub const PINNED_LATEST: Self = Self::V3_9;

    pub fn effective(self) -> Self {
        match self {
            Self::Latest => Self::PINNED_LATEST,
            other => other,
        }
    }
}

/// Parser dialect — the underlying inline tokenization rule set.
///
/// Distinct from [`Flavor`]: `Flavor` is the user-facing identity (Pandoc,
/// Quarto, GFM, etc.) and selects extension defaults; `Dialect` is the
/// structural parser identity. Several flavors share a dialect — Quarto and
/// RMarkdown both use `Pandoc`; CommonMark and GFM both use `CommonMark`.
///
/// Use this for parser branches whose behavior is fundamentally different
/// between dialect families (e.g. unmatched backtick run handling). Per-flavor
/// feature toggles still belong on [`Extensions`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Dialect {
    /// Pandoc-markdown family. Default for Pandoc, Quarto, RMarkdown,
    /// MultiMarkdown.
    #[default]
    Pandoc,
    /// CommonMark family. Default for CommonMark and GFM.
    CommonMark,
}

impl Dialect {
    /// Default dialect for a given user-facing flavor.
    pub fn for_flavor(flavor: Flavor) -> Self {
        match flavor {
            Flavor::CommonMark | Flavor::Gfm | Flavor::Mdsvex => Dialect::CommonMark,
            Flavor::Pandoc | Flavor::Quarto | Flavor::RMarkdown | Flavor::MultiMarkdown => {
                Dialect::Pandoc
            }
        }
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default, rename_all = "kebab-case"))]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ParserOptions {
    pub flavor: Flavor,
    pub dialect: Dialect,
    pub extensions: Extensions,
    /// Compatibility target for ambiguous Pandoc behavior.
    pub pandoc_compat: PandocCompat,
    /// Additional cross-reference key prefixes (beyond the Quarto built-ins
    /// recognized by [`crate::parser::inlines::citations::is_quarto_crossref_key`])
    /// that should parse as cross-references rather than citations. Populated
    /// from the top-level `crossref-prefixes` config key so documents relying on
    /// crossref-injecting extensions (e.g. pseudocode's `@algo-`) don't have
    /// those references misclassified as citations.
    #[cfg_attr(feature = "serde", serde(default, alias = "crossref_prefixes"))]
    pub crossref_prefixes: Vec<String>,
    /// Document-level reference link label set, populated by the
    /// top-level `parse()` function when running CommonMark dialect and
    /// consulted by inline parsing's bracket resolution pass. `None`
    /// means "not pre-computed"; the inline pipeline then treats every
    /// reference-shaped bracket pair conservatively (current behavior),
    /// which is correct for the Pandoc dialect and a graceful
    /// degradation for embedded use cases that bypass `parse()`.
    ///
    /// Skipped by serde so config files don't try to (de)serialize a
    /// runtime cache.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub refdef_labels: Option<Arc<HashSet<String>>>,
}

impl Default for ParserOptions {
    fn default() -> Self {
        let flavor = Flavor::default();
        Self {
            flavor,
            dialect: Dialect::for_flavor(flavor),
            extensions: Extensions::for_flavor(flavor),
            pandoc_compat: PandocCompat::default(),
            crossref_prefixes: Vec::new(),
            refdef_labels: None,
        }
    }
}

impl ParserOptions {
    pub fn effective_pandoc_compat(&self) -> PandocCompat {
        self.pandoc_compat.effective()
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for Flavor {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Flavor".into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        // Include serde aliases so the schema accepts every spelling the
        // parser accepts (e.g. `commonmark` alongside the kebab-case
        // `common-mark` canonical form).
        schemars::json_schema!({
            "type": "string",
            "description": "Markdown flavor to parse and format against.",
            "enum": [
                "pandoc",
                "quarto",
                "rmarkdown",
                "gfm",
                "common-mark",
                "commonmark",
                "multimarkdown",
                "mdsvex"
            ]
        })
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for PandocCompat {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "PandocCompat".into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "description": "Compatibility target for ambiguous Pandoc behavior.",
            "enum": [
                "latest",
                "3.7", "3-7", "v3.7", "v3-7",
                "3.9", "3-9", "v3.9", "v3-9"
            ]
        })
    }
}
