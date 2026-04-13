use std::collections::HashMap;

/// The flavor of Markdown to parse and format.
/// Each flavor has a different set of default extensions enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
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
    CommonMark,
    /// MultiMarkdown
    #[cfg_attr(feature = "serde", serde(rename = "multimarkdown"))]
    MultiMarkdown,
}

/// Pandoc/Markdown extensions configuration.
/// Each field represents a specific Pandoc extension.
/// Extensions marked with a comment indicate implementation status.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
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
    /// [NON-DEFAULT] MultiMarkdown style heading identifiers [my-id]
    pub mmd_header_identifiers: bool,
    /// [NON-DEFAULT] MultiMarkdown key=value attributes on reference defs
    pub mmd_link_attributes: bool,
    /// [NON-DEFAULT] GitHub/CommonMark alerts in blockquotes (`> [!NOTE]`)
    pub alerts: bool,
    /// [NON-DEFAULT] :emoji: syntax
    pub emoji: bool,
    /// [NON-DEFAULT] Highlighted ==text==
    pub mark: bool,

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
            gfm_auto_identifiers: false,
            grid_tables: false,
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
            table_captions: false,
            task_lists: false,
            tex_math_dollars: false,
            tex_math_double_backslash: false,
            tex_math_gfm: false,
            tex_math_single_backslash: false,
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
            autolink_bare_uris: false,
            emoji: false,
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
        ext.backtick_code_blocks = true;
        ext.emoji = true;
        ext.fenced_code_blocks = true;
        ext.footnotes = true;
        ext.gfm_auto_identifiers = true;
        ext.pipe_tables = true;
        ext.raw_html = true;
        ext.strikeout = true;
        ext.task_lists = true;
        ext.tex_math_dollars = true;
        ext.tex_math_gfm = true;
        ext.yaml_metadata_block = true;

        ext
    }

    fn commonmark_defaults() -> Self {
        let mut ext = Self::none_defaults();
        ext.raw_html = true;
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

    fn merge_overrides(mut base: Extensions, user_overrides: HashMap<String, bool>) -> Self {
        for (key, value) in user_overrides {
            let normalized_key = key.replace('_', "-");
            match normalized_key.as_str() {
                "blank-before-header" => base.blank_before_header = value,
                "header-attributes" => base.header_attributes = value,
                "auto-identifiers" => base.auto_identifiers = value,
                "gfm-auto-identifiers" => base.gfm_auto_identifiers = value,
                "implicit-header-references" => base.implicit_header_references = value,
                "blank-before-blockquote" => base.blank_before_blockquote = value,
                "fancy-lists" => base.fancy_lists = value,
                "startnum" => base.startnum = value,
                "example-lists" => base.example_lists = value,
                "task-lists" => base.task_lists = value,
                "definition-lists" => base.definition_lists = value,
                "backtick-code-blocks" => base.backtick_code_blocks = value,
                "fenced-code-blocks" => base.fenced_code_blocks = value,
                "fenced-code-attributes" => base.fenced_code_attributes = value,
                "executable-code" => base.executable_code = value,
                "rmarkdown-inline-code" => base.rmarkdown_inline_code = value,
                "quarto-inline-code" => base.quarto_inline_code = value,
                "inline-code-attributes" => base.inline_code_attributes = value,
                "simple-tables" => base.simple_tables = value,
                "multiline-tables" => base.multiline_tables = value,
                "grid-tables" => base.grid_tables = value,
                "pipe-tables" => base.pipe_tables = value,
                "table-captions" => base.table_captions = value,
                "fenced-divs" => base.fenced_divs = value,
                "native-divs" => base.native_divs = value,
                "line-blocks" => base.line_blocks = value,
                "intraword-underscores" => base.intraword_underscores = value,
                "strikeout" => base.strikeout = value,
                "superscript" => base.superscript = value,
                "subscript" => base.subscript = value,
                "inline-links" => base.inline_links = value,
                "reference-links" => base.reference_links = value,
                "shortcut-reference-links" => base.shortcut_reference_links = value,
                "link-attributes" => base.link_attributes = value,
                "autolinks" => base.autolinks = value,
                "inline-images" => base.inline_images = value,
                "implicit-figures" => base.implicit_figures = value,
                "tex-math-dollars" => base.tex_math_dollars = value,
                "tex-math-gfm" => base.tex_math_gfm = value,
                "tex-math-single-backslash" => base.tex_math_single_backslash = value,
                "tex-math-double-backslash" => base.tex_math_double_backslash = value,
                "inline-footnotes" => base.inline_footnotes = value,
                "footnotes" => base.footnotes = value,
                "citations" => base.citations = value,
                "bracketed-spans" => base.bracketed_spans = value,
                "native-spans" => base.native_spans = value,
                "yaml-metadata-block" => base.yaml_metadata_block = value,
                "pandoc-title-block" => base.pandoc_title_block = value,
                "mmd-title-block" => base.mmd_title_block = value,
                "raw-html" => base.raw_html = value,
                "markdown-in-html-blocks" => base.markdown_in_html_blocks = value,
                "raw-tex" => base.raw_tex = value,
                "raw-attribute" => base.raw_attribute = value,
                "all-symbols-escapable" => base.all_symbols_escapable = value,
                "escaped-line-breaks" => base.escaped_line_breaks = value,
                "autolink-bare-uris" => base.autolink_bare_uris = value,
                "hard-line-breaks" => base.hard_line_breaks = value,
                "mmd-header-identifiers" => base.mmd_header_identifiers = value,
                "mmd-link-attributes" => base.mmd_link_attributes = value,
                "alerts" => base.alerts = value,
                "emoji" => base.emoji = value,
                "mark" => base.mark = value,
                "quarto-callouts" => base.quarto_callouts = value,
                "quarto-crossrefs" => base.quarto_crossrefs = value,
                "quarto-shortcodes" => base.quarto_shortcodes = value,
                "bookdown-references" => base.bookdown_references = value,
                "bookdown-equation-references" => base.bookdown_equation_references = value,
                _ => {}
            }
        }
        base
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

/// Parser configuration.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default, rename_all = "kebab-case"))]
pub struct ParserConfig {
    /// Compatibility target for ambiguous Pandoc behavior.
    pub pandoc_compat: PandocCompat,
}

impl ParserConfig {
    pub fn effective_pandoc_compat(&self) -> PandocCompat {
        self.pandoc_compat.effective()
    }
}

#[derive(Debug, Clone)]
pub struct ParserOptions {
    pub flavor: Flavor,
    pub extensions: Extensions,
    /// Parser configuration (experimental)
    pub parser: ParserConfig,
}

impl Default for ParserOptions {
    fn default() -> Self {
        let flavor = Flavor::default();
        Self {
            flavor,
            extensions: Extensions::for_flavor(flavor),
            parser: ParserConfig::default(),
        }
    }
}
