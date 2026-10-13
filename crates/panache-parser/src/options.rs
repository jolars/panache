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
    /// MyST (CommonMark + Sphinx/MyST directives, roles, and targets)
    #[cfg_attr(feature = "serde", serde(rename = "myst"))]
    Myst,
    /// Hugo Markdown (CommonMark + Goldmark defaults and Hugo syntax).
    Hugo,
}

/// Markdown extensions configuration.
/// Each field controls support for a syntax feature.
/// Extensions marked with a comment indicate implementation status.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Extensions {
    /// Require blank line before headers (default: enabled)
    pub blank_before_header: bool,
    /// Full attribute syntax on headers {#id .class key=value}
    pub header_attributes: bool,
    /// Auto-generate identifiers from headings
    pub auto_identifiers: bool,
    /// Use GitHub's algorithm for auto-generated heading identifiers
    pub gfm_auto_identifiers: bool,
    /// Implicit header references ([Heading] links to header)
    pub implicit_header_references: bool,

    /// Require blank line before blockquotes (default: enabled)
    pub blank_before_blockquote: bool,

    /// Fancy list markers (roman numerals, letters, etc.)
    pub fancy_lists: bool,
    /// Start ordered lists at arbitrary numbers
    pub startnum: bool,
    /// Example lists with (@) markers
    pub example_lists: bool,
    /// GitHub-style task lists - [ ] and - [x]
    pub task_lists: bool,
    /// Term/definition syntax
    pub definition_lists: bool,
    /// Allow lists without a preceding blank line
    pub lists_without_preceding_blankline: bool,
    /// [NON-DEFAULT] Pandoc <= 2.0 list semantics: continuation paragraphs and
    /// nested lists require four-space (one tab-width) indentation
    pub four_space_rule: bool,

    /// Fenced code blocks with backticks
    pub backtick_code_blocks: bool,
    /// Fenced code blocks with tildes
    pub fenced_code_blocks: bool,
    /// Attributes on fenced code blocks {.language #id}
    pub fenced_code_attributes: bool,
    /// Executable code syntax (currently fenced chunks like ```{r} / ```{python})
    pub executable_code: bool,
    /// R Markdown inline executable code and nested-backtick tails.
    pub rmarkdown_inline_code: bool,
    /// Quarto inline R, Python, and Julia expressions and nested-backtick R tails.
    pub quarto_inline_code: bool,
    /// Attributes on inline code
    pub inline_code_attributes: bool,

    /// Simple table syntax
    pub simple_tables: bool,
    /// Multiline cell content in tables
    pub multiline_tables: bool,
    /// Grid-style tables
    pub grid_tables: bool,
    /// Pipe tables (GitHub/PHP Markdown style)
    pub pipe_tables: bool,
    /// Table captions
    pub table_captions: bool,

    /// Fenced divs ::: {.class}
    pub fenced_divs: bool,
    /// HTML <div> elements
    pub native_divs: bool,

    /// Line blocks for poetry | prefix
    pub line_blocks: bool,

    /// Underscores don't trigger emphasis in snake_case
    pub intraword_underscores: bool,
    /// Strikethrough ~~text~~
    pub strikeout: bool,
    /// Superscript and subscript ^super^ ~sub~
    pub superscript: bool,
    pub subscript: bool,

    /// Inline links [text](url)
    pub inline_links: bool,
    /// Reference links [text][ref]
    pub reference_links: bool,
    /// Shortcut reference links [ref] without second []
    pub shortcut_reference_links: bool,
    /// Attributes on links [text](url){.class}
    pub link_attributes: bool,
    /// Automatic links <http://example.com>
    pub autolinks: bool,

    /// Inline images ![alt](url)
    pub inline_images: bool,
    /// Paragraph with just image becomes figure
    pub implicit_figures: bool,

    /// Dollar-delimited math $x$ and $$equation$$
    pub tex_math_dollars: bool,
    /// [NON-DEFAULT] GFM math: inline $`...`$ and fenced ``` math blocks
    pub tex_math_gfm: bool,
    /// [NON-DEFAULT] Single backslash math \(...\) and \[...\] (RMarkdown default)
    pub tex_math_single_backslash: bool,
    /// [NON-DEFAULT] Double backslash math \\(...\\) and \\[...\\]
    pub tex_math_double_backslash: bool,

    /// Inline footnotes ^[text]
    pub inline_footnotes: bool,
    /// Reference footnotes `[^1]` (requires footnote parsing)
    pub footnotes: bool,

    /// Citation syntax [@cite]
    pub citations: bool,

    /// Bracketed spans [text]{.class}
    pub bracketed_spans: bool,
    /// HTML <span> elements
    pub native_spans: bool,

    /// YAML metadata block
    pub yaml_metadata_block: bool,
    /// TOML front matter delimited by `+++`, preserved verbatim.
    pub toml_metadata_block: bool,
    /// Hugo shortcode calls and paired bodies, preserved verbatim.
    pub hugo_shortcodes: bool,
    /// Pandoc title block (Title/Author/Date)
    pub pandoc_title_block: bool,
    /// [NON-DEFAULT] MultiMarkdown metadata/title block (Key: Value ...)
    pub mmd_title_block: bool,

    /// Raw HTML blocks and inline
    pub raw_html: bool,
    /// Markdown inside HTML blocks
    pub markdown_in_html_blocks: bool,
    /// LaTeX commands and environments
    pub raw_tex: bool,
    /// Generic raw blocks with {=format} syntax
    pub raw_attribute: bool,

    /// Backslash escapes any symbol
    pub all_symbols_escapable: bool,
    /// Backslash at line end = hard line break
    pub escaped_line_breaks: bool,

    /// [NON-DEFAULT] Bare URLs become links
    pub autolink_bare_uris: bool,
    /// [NON-DEFAULT] Newline = <br>
    pub hard_line_breaks: bool,
    /// [NON-DEFAULT] Ignore soft breaks between two East Asian wide characters
    pub east_asian_line_breaks: bool,
    /// [NON-DEFAULT] MultiMarkdown style heading identifiers [my-id]
    pub mmd_header_identifiers: bool,
    /// [NON-DEFAULT] MultiMarkdown key=value attributes on reference defs
    pub mmd_link_attributes: bool,
    /// [NON-DEFAULT] GitHub/CommonMark alerts in blockquotes (`> [!NOTE]`)
    pub alerts: bool,
    /// [NON-DEFAULT] python-markdown admonitions (`!!! note`) with 4-space
    /// indented content
    pub python_markdown_admonitions: bool,
    /// [NON-DEFAULT] pymdownx details: collapsible admonitions (`???`/`???+`)
    pub pymdownx_details: bool,
    /// [NON-DEFAULT] :emoji: syntax
    pub emoji: bool,
    /// [NON-DEFAULT] Highlighted ==text==
    pub mark: bool,
    /// [NON-DEFAULT] Pandoc wikilinks with title after pipe: `[[url|title]]`
    pub wikilinks_title_after_pipe: bool,
    /// [NON-DEFAULT] Pandoc wikilinks with title before pipe: `[[title|url]]`
    pub wikilinks_title_before_pipe: bool,
    /// [NON-DEFAULT] Allow whitespace between reference link brackets: `[foo] [bar]`
    pub spaced_reference_links: bool,

    /// Quarto callout blocks (.callout-note, etc.)
    pub quarto_callouts: bool,
    /// Quarto cross-references @fig-id, @tbl-id
    pub quarto_crossrefs: bool,
    /// Quarto shortcodes {{< name args >}}
    pub quarto_shortcodes: bool,
    /// Bookdown references \@ref(label) and (\#label)
    pub bookdown_references: bool,
    /// Bookdown equation references in LaTeX math blocks (\#eq:label)
    pub bookdown_equation_references: bool,

    /// [NON-DEFAULT] Svelte template syntax: `{#if}`/`{:else}`/`{/each}` block
    /// logic, `{@html ...}`/`{@const ...}` tags, and `{expr}` interpolation.
    /// Parsed as opaque, lossless spans (content preserved verbatim).
    pub svelte_template: bool,

    /// [NON-DEFAULT] MyST directives: ```` ```{name} ```` (and, with
    /// `myst_colon_fence`, `:::{name}`) blocks carrying `:key: value` options
    /// and a markdown body.
    pub myst_directives: bool,
    /// [NON-DEFAULT] MyST inline roles: `` {name}`content` ``.
    pub myst_roles: bool,
    /// [NON-DEFAULT] MyST targets `(label)=` preceding a block.
    pub myst_targets: bool,
    /// [NON-DEFAULT] MyST inline substitutions `{{ name }}`.
    pub myst_substitutions: bool,
    /// [NON-DEFAULT] MyST line comments beginning with `%`.
    pub myst_comments: bool,
    /// [NON-DEFAULT] MyST colon-fence directive form `:::{name}` (off by default
    /// even in MyST, matching `myst-parser`'s opt-in `colon_fence` extension).
    pub myst_colon_fence: bool,
    /// [NON-DEFAULT] MyST block breaks: a line of 3+ `+` markers (`+++`),
    /// optionally carrying trailing cell metadata. `myst-parser` loads these via
    /// `myst_block_plugin`, so they are on by default for `Flavor::Myst`.
    pub myst_block_breaks: bool,
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
            myst_directives: false,
            myst_roles: false,
            myst_targets: false,
            myst_substitutions: false,
            myst_comments: false,
            myst_colon_fence: false,
            myst_block_breaks: false,
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
            toml_metadata_block: false,
            hugo_shortcodes: false,
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
            Flavor::Myst => Self::myst_defaults(),
            Flavor::Hugo => Self::hugo_defaults(),
        }
    }

    fn pandoc_defaults() -> Self {
        Self {
            auto_identifiers: true,
            blank_before_blockquote: true,
            blank_before_header: true,
            gfm_auto_identifiers: false,
            header_attributes: true,
            implicit_header_references: true,

            definition_lists: true,
            example_lists: true,
            fancy_lists: true,
            lists_without_preceding_blankline: false,
            startnum: true,
            task_lists: true,

            backtick_code_blocks: true,
            executable_code: false,
            rmarkdown_inline_code: false,
            quarto_inline_code: false,
            fenced_code_attributes: true,
            fenced_code_blocks: true,
            inline_code_attributes: true,

            grid_tables: true,
            multiline_tables: true,
            pipe_tables: true,
            simple_tables: true,
            table_captions: true,

            fenced_divs: true,
            native_divs: true,

            line_blocks: true,

            intraword_underscores: true,
            strikeout: true,
            subscript: true,
            superscript: true,

            autolinks: true,
            inline_links: true,
            link_attributes: true,
            reference_links: true,
            shortcut_reference_links: true,

            implicit_figures: true,
            inline_images: true,

            tex_math_dollars: true,
            tex_math_double_backslash: false,
            tex_math_gfm: false,
            tex_math_single_backslash: false,

            footnotes: true,
            inline_footnotes: true,

            citations: true,

            bracketed_spans: true,
            native_spans: true,

            mmd_title_block: false,
            pandoc_title_block: true,
            yaml_metadata_block: true,
            toml_metadata_block: false,
            hugo_shortcodes: false,

            markdown_in_html_blocks: false,
            raw_attribute: true,
            raw_html: true,
            raw_tex: true,

            all_symbols_escapable: true,
            escaped_line_breaks: true,

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

            bookdown_references: false,
            bookdown_equation_references: false,
            quarto_callouts: false,
            quarto_crossrefs: false,
            quarto_shortcodes: false,

            wikilinks_title_after_pipe: false,
            wikilinks_title_before_pipe: false,

            spaced_reference_links: false,

            svelte_template: false,

            myst_directives: false,
            myst_roles: false,
            myst_targets: false,
            myst_substitutions: false,
            myst_comments: false,
            myst_colon_fence: false,
            myst_block_breaks: false,
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
        ext.escaped_line_breaks = true;
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
        let mut ext = Self::gfm_defaults();

        ext.footnotes = false;
        ext.tex_math_dollars = false;
        ext.tex_math_gfm = false;
        ext.emoji = false;
        ext.alerts = false;
        ext.auto_identifiers = false;

        ext.svelte_template = true;

        ext
    }

    fn hugo_defaults() -> Self {
        let mut ext = Self::commonmark_defaults();
        ext.pipe_tables = true;
        ext.strikeout = true;
        ext.task_lists = true;
        ext.autolink_bare_uris = true;
        ext.definition_lists = true;
        ext.footnotes = true;
        ext.header_attributes = true;
        ext.auto_identifiers = true;
        ext.gfm_auto_identifiers = true;
        ext.yaml_metadata_block = true;
        ext.toml_metadata_block = true;
        ext.hugo_shortcodes = true;
        ext
    }

    fn myst_defaults() -> Self {
        let mut ext = Self::commonmark_defaults();

        ext.myst_directives = true;
        ext.myst_roles = true;
        ext.myst_targets = true;
        ext.myst_comments = true;
        ext.myst_block_breaks = true;

        ext.pipe_tables = true;
        ext.footnotes = true;
        ext.inline_footnotes = false;

        ext.yaml_metadata_block = true;

        ext.myst_substitutions = false;
        ext.myst_colon_fence = false;

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
            /// generator. Only kebab-case is accepted; the snake_case aliases
            /// were removed in 3.0.
            pub const KNOWN_NAMES: &'static [&'static str] = &[ $($kebab),* ];

            /// True if `name` is a known (kebab-case) extension key.
            pub fn is_known_name(name: &str) -> bool {
                Self::KNOWN_NAMES.iter().any(|k| *k == name)
            }

            fn set_by_name(&mut self, name: &str, value: bool) -> bool {
                match name {
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
    "toml-metadata-block" => toml_metadata_block,
    "hugo-shortcodes" => hugo_shortcodes,
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
    "myst-directives" => myst_directives,
    "myst-roles" => myst_roles,
    "myst-targets" => myst_targets,
    "myst-substitutions" => myst_substitutions,
    "myst-comments" => myst_comments,
    "myst-colon-fence" => myst_colon_fence,
    "myst-block-breaks" => myst_block_breaks,
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
            Flavor::Myst,
        ] {
            assert!(
                !Extensions::for_flavor(flavor).four_space_rule,
                "four_space_rule should be off by default for {flavor:?}"
            );
        }
    }

    #[test]
    fn escaped_line_breaks_defaults_on_for_commonmark_derived_flavors() {
        for flavor in [
            Flavor::Pandoc,
            Flavor::Quarto,
            Flavor::RMarkdown,
            Flavor::Gfm,
            Flavor::CommonMark,
            Flavor::Mdsvex,
            Flavor::Myst,
        ] {
            assert!(
                Extensions::for_flavor(flavor).escaped_line_breaks,
                "escaped_line_breaks should be on by default for {flavor:?}"
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
            Flavor::Myst,
        ] {
            assert!(
                !Extensions::for_flavor(flavor).svelte_template,
                "svelte_template should be off by default for {flavor:?}"
            );
        }
    }

    #[test]
    fn myst_uses_commonmark_dialect() {
        assert_eq!(Dialect::for_flavor(Flavor::Myst), Dialect::CommonMark);
    }

    #[test]
    fn myst_defaults_enable_core_constructs_only() {
        let ext = Extensions::for_flavor(Flavor::Myst);

        assert!(ext.myst_directives);
        assert!(ext.myst_roles);
        assert!(ext.myst_targets);
        assert!(ext.myst_comments);
        assert!(ext.myst_block_breaks);

        assert!(ext.pipe_tables);
        assert!(ext.footnotes);
        assert!(!ext.inline_footnotes);

        assert!(ext.yaml_metadata_block);

        assert!(!ext.myst_colon_fence);
        assert!(!ext.myst_substitutions);
        assert!(!ext.tex_math_dollars);
        assert!(!ext.definition_lists);
        assert!(!ext.task_lists);
        assert!(!ext.strikeout);

        assert!(ext.inline_links);
        assert!(ext.backtick_code_blocks);
        assert!(!ext.fenced_divs);
        assert!(!ext.bracketed_spans);
        assert!(!ext.header_attributes);
    }

    #[test]
    fn myst_core_constructs_off_for_other_flavors() {
        for flavor in [
            Flavor::Pandoc,
            Flavor::Quarto,
            Flavor::RMarkdown,
            Flavor::Gfm,
            Flavor::CommonMark,
            Flavor::MultiMarkdown,
            Flavor::Mdsvex,
        ] {
            let ext = Extensions::for_flavor(flavor);
            assert!(
                !ext.myst_directives
                    && !ext.myst_roles
                    && !ext.myst_targets
                    && !ext.myst_comments
                    && !ext.myst_block_breaks,
                "MyST core constructs should be off by default for {flavor:?}"
            );
        }
    }

    #[test]
    fn mdsvex_defaults_match_remark_parse_8_gfm() {
        let ext = Extensions::for_flavor(Flavor::Mdsvex);

        assert!(ext.svelte_template);
        assert!(ext.raw_html);
        assert!(ext.yaml_metadata_block);

        assert!(ext.pipe_tables);
        assert!(ext.strikeout);
        assert!(ext.task_lists);
        assert!(ext.autolink_bare_uris);

        assert!(!ext.footnotes);
        assert!(!ext.tex_math_dollars);
        assert!(!ext.emoji);
        assert!(!ext.alerts);

        assert!(!ext.auto_identifiers);
        assert!(ext.gfm_auto_identifiers);

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
    #[cfg_attr(
        feature = "serde",
        serde(rename = "3.9", alias = "3-9", alias = "v3.9", alias = "v3-9")
    )]
    V3_9,
    /// Match Pandoc 3.10 behavior for ambiguous syntax edge cases.
    #[cfg_attr(
        feature = "serde",
        serde(rename = "3.10", alias = "3-10", alias = "v3.10", alias = "v3-10")
    )]
    V3_10,
    /// Match Pandoc 3.11 behavior for ambiguous syntax edge cases.
    #[cfg_attr(
        feature = "serde",
        serde(rename = "3.11", alias = "3-11", alias = "v3.11", alias = "v3-11")
    )]
    V3_11,
    /// Match Pandoc 3.12 behavior for ambiguous syntax edge cases.
    #[default]
    #[cfg_attr(
        feature = "serde",
        serde(rename = "3.12", alias = "3-12", alias = "v3.12", alias = "v3-12")
    )]
    V3_12,
}

impl PandocCompat {
    /// Pinned target for `latest`.
    pub const PINNED_LATEST: Self = Self::V3_12;

    pub fn effective(self) -> Self {
        match self {
            Self::Latest => Self::PINNED_LATEST,
            other => other,
        }
    }

    /// Whether an ordered list nested inside a list item or definition body
    /// must start at 1 (`1.`, `i.`, `a.`, `A.`, `(1)`, ...) to be recognized
    /// as a list at all.
    ///
    /// Pandoc 3.10.1 aligned the markdown reader with CommonMark here
    /// (jgm/pandoc#11735) to avoid unintended list starts: under the new rule
    /// `- item\n\n  2. sub` reads `2. sub` as a paragraph, not a sublist.
    /// Top-level lists (including inside blockquotes and divs) still accept
    /// any start number.
    ///
    /// Deliberately a named predicate rather than an `Ord` comparison: the
    /// `Latest` variant sorts first, so deriving an ordering on this enum
    /// would silently give the wrong answer.
    pub fn restricts_ordered_sublist_start(self) -> bool {
        matches!(self.effective(), Self::V3_10 | Self::V3_11 | Self::V3_12)
    }

    /// Whether example-list markers may reset the counter with `(1@label)`.
    /// Pandoc introduced this syntax in 3.11 (jgm/pandoc#10940).
    pub fn supports_example_list_resets(self) -> bool {
        matches!(self.effective(), Self::V3_11 | Self::V3_12)
    }

    /// Whether table paragraphs compact only when every cell is empty or
    /// contains a single paragraph. Pandoc 3.12 replaced per-cell compaction
    /// with this table-wide rule (jgm/pandoc#11864).
    pub fn compacts_tables_as_whole(self) -> bool {
        matches!(self.effective(), Self::V3_12)
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
    /// CommonMark family. Default for CommonMark, GFM, mdsvex, MyST, and Hugo.
    CommonMark,
}

impl Dialect {
    /// Default dialect for a given user-facing flavor.
    pub fn for_flavor(flavor: Flavor) -> Self {
        match flavor {
            Flavor::CommonMark | Flavor::Gfm | Flavor::Mdsvex | Flavor::Myst | Flavor::Hugo => {
                Dialect::CommonMark
            }
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
    /// Preserve unresolved reference-shaped bracket groups as typed CST nodes.
    ///
    /// CommonMark normally treats an unresolved reference as literal text. This
    /// opt-in retains that source shape for downstream semantic extensions while
    /// leaving ordinary CommonMark and GFM parsing unchanged by default.
    #[cfg_attr(feature = "serde", serde(default))]
    pub preserve_unresolved_references: bool,
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
            preserve_unresolved_references: false,
            crossref_prefixes: Vec::new(),
            refdef_labels: None,
        }
    }
}

impl ParserOptions {
    /// Construct internally consistent parser options for a user-facing flavor.
    pub fn for_flavor(flavor: Flavor) -> Self {
        Self {
            flavor,
            dialect: Dialect::for_flavor(flavor),
            extensions: Extensions::for_flavor(flavor),
            ..Self::default()
        }
    }

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
                "mdsvex",
                "myst",
                "hugo"
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
                "3.9", "3-9", "v3.9", "v3-9",
                "3.10", "3-10", "v3.10", "v3-10",
                "3.11", "3-11", "v3.11", "v3-11",
                "3.12", "3-12", "v3.12", "v3-12"
            ]
        })
    }
}
