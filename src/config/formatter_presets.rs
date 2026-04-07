use crate::config::FormatterConfig;

fn preset(cmd: &str, args: &[&str], stdin: bool) -> FormatterConfig {
    FormatterConfig {
        cmd: cmd.to_string(),
        args: args.iter().map(ToString::to_string).collect(),
        enabled: true,
        stdin,
    }
}

pub fn get_formatter_preset(name: &str) -> Option<FormatterConfig> {
    match name {
        // Existing presets
        "yamlfmt" => Some(preset("yamlfmt", &["-"], true)),
        "prettier" => Some(preset("prettier", &["--parser", "yaml"], true)),
        "taplo" => Some(preset("taplo", &["format", "-"], true)),
        "shfmt" => Some(preset("shfmt", &["-"], true)),
        "clang-format" => Some(preset("clang-format", &["-"], true)),
        "air" => Some(preset("air", &["format", "{}"], false)),
        "styler" => Some(preset(
            "Rscript",
            &["-e", "styler::style_file('{}')"],
            false,
        )),
        "ruff" => Some(preset(
            "ruff",
            &["format", "--stdin-filename", "stdin.py", "-"],
            true,
        )),
        "black" => Some(preset("black", &["-"], true)),

        // Curated conform.nvim-derived presets (non-deprecated, static-compatible)
        "alejandra" => Some(preset("alejandra", &[], true)),
        "mdformat" => Some(preset("mdformat", &["-"], true)),
        "sqlfmt" => Some(preset("sqlfmt", &["-"], true)),
        "terraform-fmt" => Some(preset("terraform", &["fmt", "-no-color", "-"], true)),
        "yamlfix" => Some(preset("yamlfix", &["-"], true)),

        _ => None,
    }
}

pub fn formatter_preset_names() -> &'static [&'static str] {
    &[
        "air",
        "alejandra",
        "black",
        "clang-format",
        "mdformat",
        "prettier",
        "ruff",
        "shfmt",
        "sqlfmt",
        "styler",
        "taplo",
        "terraform-fmt",
        "yamlfix",
        "yamlfmt",
    ]
}
