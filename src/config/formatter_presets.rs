use crate::config::FormatterConfig;

#[derive(Debug, Clone, Copy)]
pub struct FormatterPresetMetadata {
    pub name: &'static str,
    pub url: &'static str,
    pub description: &'static str,
    pub cmd: &'static str,
    pub args: &'static [&'static str],
    pub stdin: bool,
    pub supported_languages: &'static [&'static str],
}

impl FormatterPresetMetadata {
    pub fn to_formatter_config(self) -> FormatterConfig {
        FormatterConfig {
            cmd: self.cmd.to_string(),
            args: self.args.iter().map(ToString::to_string).collect(),
            enabled: true,
            stdin: self.stdin,
        }
    }
}

const PRESETS: &[FormatterPresetMetadata] = &[
    FormatterPresetMetadata {
        name: "air",
        url: "https://github.com/posit-dev/air",
        description: "R formatter for reproducible style conventions.",
        cmd: "air",
        args: &["format", "{}"],
        stdin: false,
        supported_languages: &["r"],
    },
    FormatterPresetMetadata {
        name: "alejandra",
        url: "https://kamadorueda.com/alejandra/",
        description: "Uncompromising Nix formatter.",
        cmd: "alejandra",
        args: &[],
        stdin: true,
        supported_languages: &["nix"],
    },
    FormatterPresetMetadata {
        name: "asmfmt",
        url: "https://github.com/klauspost/asmfmt",
        description: "Go assembler formatter.",
        cmd: "asmfmt",
        args: &[],
        stdin: true,
        supported_languages: &["asm", "assembly"],
    },
    FormatterPresetMetadata {
        name: "astyle",
        url: "https://astyle.sourceforge.net/astyle.html",
        description: "Formatter for C/C++/Java/C# source code.",
        cmd: "astyle",
        args: &["--quiet"],
        stdin: true,
        supported_languages: &["c", "cpp", "c++", "java", "csharp", "c#"],
    },
    FormatterPresetMetadata {
        name: "autocorrect",
        url: "https://github.com/huacnlee/autocorrect",
        description: "Formatter/linter for CJK spacing and punctuation.",
        cmd: "autocorrect",
        args: &["--stdin"],
        stdin: true,
        supported_languages: &["text", "txt", "markdown", "md"],
    },
    FormatterPresetMetadata {
        name: "black",
        url: "https://github.com/psf/black",
        description: "Opinionated Python formatter.",
        cmd: "black",
        args: &["-"],
        stdin: true,
        supported_languages: &["python", "py"],
    },
    FormatterPresetMetadata {
        name: "clang-format",
        url: "https://clang.llvm.org/docs/ClangFormat.html",
        description: "Formatter for C-family languages.",
        cmd: "clang-format",
        args: &["-"],
        stdin: true,
        supported_languages: &[
            "c",
            "h",
            "cpp",
            "c++",
            "hpp",
            "cc",
            "cxx",
            "objc",
            "objective-c",
            "obj-c",
            "java",
            "csharp",
            "c#",
        ],
    },
    FormatterPresetMetadata {
        name: "cmake-format",
        url: "https://github.com/cheshirekow/cmake_format",
        description: "Formatter for CMake listfiles.",
        cmd: "cmake-format",
        args: &["-"],
        stdin: true,
        supported_languages: &["cmake"],
    },
    FormatterPresetMetadata {
        name: "cue-fmt",
        url: "https://cuelang.org",
        description: "Format CUE files.",
        cmd: "cue",
        args: &["fmt", "-"],
        stdin: true,
        supported_languages: &["cue"],
    },
    FormatterPresetMetadata {
        name: "gleam",
        url: "https://github.com/gleam-lang/gleam",
        description: "Format Gleam source files.",
        cmd: "gleam",
        args: &["format", "--stdin"],
        stdin: true,
        supported_languages: &["gleam"],
    },
    FormatterPresetMetadata {
        name: "gofmt",
        url: "https://pkg.go.dev/cmd/gofmt",
        description: "Go formatter.",
        cmd: "gofmt",
        args: &[],
        stdin: true,
        supported_languages: &["go", "golang"],
    },
    FormatterPresetMetadata {
        name: "gofumpt",
        url: "https://github.com/mvdan/gofumpt",
        description: "Stricter formatting for Go.",
        cmd: "gofumpt",
        args: &[],
        stdin: true,
        supported_languages: &["go", "golang"],
    },
    FormatterPresetMetadata {
        name: "jsonnetfmt",
        url: "https://github.com/google/go-jsonnet",
        description: "Format Jsonnet files.",
        cmd: "jsonnetfmt",
        args: &["-"],
        stdin: true,
        supported_languages: &["jsonnet", "libsonnet"],
    },
    FormatterPresetMetadata {
        name: "mdformat",
        url: "https://github.com/executablebooks/mdformat",
        description: "Opinionated Markdown formatter.",
        cmd: "mdformat",
        args: &["-"],
        stdin: true,
        supported_languages: &["md", "markdown", "qmd", "rmd"],
    },
    FormatterPresetMetadata {
        name: "nixfmt",
        url: "https://github.com/NixOS/nixfmt",
        description: "Official formatter for Nix code.",
        cmd: "nixfmt",
        args: &[],
        stdin: true,
        supported_languages: &["nix"],
    },
    FormatterPresetMetadata {
        name: "prettier",
        url: "https://prettier.io/",
        description: "Opinionated formatter (preset uses YAML parser mode).",
        cmd: "prettier",
        args: &["--parser", "yaml"],
        stdin: true,
        supported_languages: &["yaml", "yml"],
    },
    FormatterPresetMetadata {
        name: "ruff",
        url: "https://docs.astral.sh/ruff/",
        description: "Python formatter via Ruff.",
        cmd: "ruff",
        args: &["format", "--stdin-filename", "stdin.py", "-"],
        stdin: true,
        supported_languages: &["python", "py"],
    },
    FormatterPresetMetadata {
        name: "shfmt",
        url: "https://github.com/mvdan/sh",
        description: "Shell script formatter.",
        cmd: "shfmt",
        args: &["-"],
        stdin: true,
        supported_languages: &["sh", "bash", "zsh", "ksh", "shell"],
    },
    FormatterPresetMetadata {
        name: "sqlfmt",
        url: "https://sqlfmt.com",
        description: "SQL formatter inspired by Black.",
        cmd: "sqlfmt",
        args: &["-"],
        stdin: true,
        supported_languages: &["sql"],
    },
    FormatterPresetMetadata {
        name: "styler",
        url: "https://styler.r-lib.org/",
        description: "R formatter via styler::style_file.",
        cmd: "Rscript",
        args: &["-e", "styler::style_file('{}')"],
        stdin: false,
        supported_languages: &["r"],
    },
    FormatterPresetMetadata {
        name: "taplo",
        url: "https://taplo.tamasfe.dev/",
        description: "TOML formatter.",
        cmd: "taplo",
        args: &["format", "-"],
        stdin: true,
        supported_languages: &["toml"],
    },
    FormatterPresetMetadata {
        name: "terraform-fmt",
        url: "https://developer.hashicorp.com/terraform/cli/commands/fmt",
        description: "Terraform formatter.",
        cmd: "terraform",
        args: &["fmt", "-no-color", "-"],
        stdin: true,
        supported_languages: &["terraform", "hcl", "tf"],
    },
    FormatterPresetMetadata {
        name: "yamlfmt",
        url: "https://github.com/google/yamlfmt",
        description: "YAML formatter.",
        cmd: "yamlfmt",
        args: &["-"],
        stdin: true,
        supported_languages: &["yaml", "yml"],
    },
    FormatterPresetMetadata {
        name: "yamlfix",
        url: "https://github.com/lyz-code/yamlfix",
        description: "YAML formatter preserving comments.",
        cmd: "yamlfix",
        args: &["-"],
        stdin: true,
        supported_languages: &["yaml", "yml"],
    },
    FormatterPresetMetadata {
        name: "yq",
        url: "https://github.com/mikefarah/yq",
        description: "YAML processor in pretty-print mode.",
        cmd: "yq",
        args: &["-P", "-"],
        stdin: true,
        supported_languages: &["yaml", "yml"],
    },
];

pub fn formatter_preset_metadata(name: &str) -> Option<&'static FormatterPresetMetadata> {
    PRESETS.iter().find(|preset| preset.name == name)
}

pub fn all_formatter_preset_metadata() -> &'static [FormatterPresetMetadata] {
    PRESETS
}

pub fn formatter_presets_for_language(language: &str) -> Vec<&'static FormatterPresetMetadata> {
    let normalized = language.trim().to_ascii_lowercase().replace('_', "-");
    PRESETS
        .iter()
        .filter(|preset| {
            preset
                .supported_languages
                .iter()
                .any(|supported| *supported == normalized)
        })
        .collect()
}

pub fn get_formatter_preset(name: &str) -> Option<FormatterConfig> {
    formatter_preset_metadata(name)
        .copied()
        .map(|meta| meta.to_formatter_config())
}

pub fn formatter_preset_supported_languages(name: &str) -> Option<&'static [&'static str]> {
    formatter_preset_metadata(name).map(|meta| meta.supported_languages)
}

pub fn formatter_preset_names() -> &'static [&'static str] {
    &[
        "air",
        "alejandra",
        "asmfmt",
        "astyle",
        "autocorrect",
        "black",
        "clang-format",
        "cmake-format",
        "cue-fmt",
        "gleam",
        "gofmt",
        "gofumpt",
        "jsonnetfmt",
        "mdformat",
        "nixfmt",
        "prettier",
        "ruff",
        "shfmt",
        "sqlfmt",
        "styler",
        "taplo",
        "terraform-fmt",
        "yamlfix",
        "yamlfmt",
        "yq",
    ]
}
