use clap::CommandFactory;
use clap_complete::{Shell, generate_to};
use clap_mangen::Man;
use std::env;
use std::fs;
use std::io::Result;
use std::path::PathBuf;

#[path = "src/cli.rs"]
mod cli;

use cli::Cli;

fn generate_completions(outdir: &std::ffi::OsString) -> Result<()> {
    let mut cmd = Cli::command();

    // Generate shell completions to OUT_DIR (for cargo build)
    for shell in [
        Shell::Bash,
        Shell::Fish,
        Shell::Zsh,
        Shell::PowerShell,
        Shell::Elvish,
    ] {
        generate_to(shell, &mut cmd, "panache", outdir)?;
    }

    // Also copy completions to target/completions for packaging
    let completions_dir = PathBuf::from("target/completions");
    fs::create_dir_all(&completions_dir)?;

    let outdir_path = PathBuf::from(outdir);

    // Copy bash, fish, and zsh completions for packaging
    let bash_src = outdir_path.join("panache.bash");
    let fish_src = outdir_path.join("panache.fish");
    let zsh_src = outdir_path.join("_panache");

    if bash_src.exists() {
        fs::copy(&bash_src, completions_dir.join("panache.bash"))?;
    }
    if fish_src.exists() {
        fs::copy(&fish_src, completions_dir.join("panache.fish"))?;
    }
    if zsh_src.exists() {
        fs::copy(&zsh_src, completions_dir.join("_panache"))?;
    }

    Ok(())
}

fn generate_cli_markdown() -> Result<()> {
    // Skip during cargo package/publish - file should be committed to git
    // During packaging, cargo runs build in a temporary directory
    let is_packaging = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.contains("/target/package/")))
        .unwrap_or(false);

    if is_packaging {
        return Ok(());
    }

    let cmd = Cli::command();
    let docs_dir = PathBuf::from("docs/reference");

    // Only proceed if docs directory exists
    if !docs_dir.exists() {
        return Ok(());
    }

    let opts = clap_markdown::MarkdownOptions::default()
        .show_footer(false)
        .show_table_of_contents(false);

    // Generate markdown documentation
    let markdown = clap_markdown::help_markdown_command_custom(&cmd, &opts);

    // // Build the complete document with frontmatter
    let mut document = String::new();
    document.push_str("---\n");
    document.push_str("title: CLI Reference\n");
    document.push_str("description: >-\n  Comprehensive reference for the Panache CLI, including all commands, options, and usage examples.\n");
    document.push_str("---\n\n");
    document.push_str(&markdown);

    // Write the document
    let output_path = docs_dir.join("cli.qmd");
    fs::write(&output_path, &document)?;
    println!("Generated CLI markdown: {:?}", output_path);

    Ok(())
}

#[derive(Debug)]
struct FormatterPresetDocRow {
    preset_name: String,
    description: String,
    homepage: String,
    supported_languages: Vec<String>,
    cmd: String,
    args: Vec<String>,
    mode: &'static str,
}

#[derive(Debug)]
struct LinterDocRow {
    name: String,
    description: String,
    homepage: String,
    supported_languages: Vec<String>,
    cmd: String,
    args: Vec<String>,
}

fn extract_quoted_strings(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = src;
    while let Some(start) = rest.find('"') {
        let after_start = &rest[start + 1..];
        if let Some(end) = after_start.find('"') {
            out.push(after_start[..end].to_string());
            rest = &after_start[end + 1..];
        } else {
            break;
        }
    }
    out
}

fn extract_field_string(block: &str, field: &str) -> Option<String> {
    let marker = format!("{field}:");
    let pos = block.find(&marker)?;
    let after = &block[pos + marker.len()..];
    let first = after.find('"')?;
    let after_first = &after[first + 1..];
    let end = after_first.find('"')?;
    Some(after_first[..end].to_string())
}

fn extract_field_list(block: &str, field: &str) -> Option<Vec<String>> {
    let marker_slice = format!("{field}: &[");
    if let Some(start) = block.find(&marker_slice) {
        let after = &block[start + marker_slice.len()..];
        let end = after.find("],")?;
        return Some(extract_quoted_strings(&after[..end]));
    }

    let marker_vec = format!("{field}: vec![");
    let start = block.find(&marker_vec)?;
    let after = &block[start + marker_vec.len()..];
    let end = after.find("],")?;
    Some(extract_quoted_strings(&after[..end]))
}

fn format_languages_for_docs(languages: &[String]) -> String {
    let mut unique: Vec<String> = Vec::new();
    for language in languages {
        let language = language.trim();
        if language.is_empty() {
            continue;
        }
        if !unique.iter().any(|existing| existing == language) {
            unique.push(language.to_string());
        }
    }
    unique
        .into_iter()
        .map(|language| format!("`{}`", language))
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_args(args: &[String]) -> String {
    if args.is_empty() {
        "[]".to_string()
    } else {
        format!("[{}]", args.join(", "))
    }
}

fn generate_external_formatter_table() -> Result<()> {
    // Skip during cargo package/publish - file should be committed to git
    let is_packaging = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.contains("/target/package/")))
        .unwrap_or(false);
    if is_packaging {
        return Ok(());
    }

    let presets_path = PathBuf::from("src/config/formatter_presets.rs");
    let docs_dir = PathBuf::from("docs/reference");
    if !presets_path.exists() || !docs_dir.exists() {
        return Ok(());
    }

    let source = fs::read_to_string(&presets_path)?;
    let mut rows: Vec<FormatterPresetDocRow> = Vec::new();

    for chunk in source.split("FormatterPresetMetadata {").skip(1) {
        let Some(block_end) = chunk.find("\n    },") else {
            continue;
        };
        let block = &chunk[..block_end];

        let Some(name) = extract_field_string(block, "name") else {
            continue;
        };
        let Some(cmd) = extract_field_string(block, "cmd") else {
            continue;
        };
        let Some(description) = extract_field_string(block, "description") else {
            continue;
        };
        let Some(homepage) = extract_field_string(block, "url") else {
            continue;
        };
        let Some(args) = extract_field_list(block, "args") else {
            continue;
        };
        let mode = if block.contains("stdin: false") {
            "File-based"
        } else {
            "Stdin"
        };
        let Some(supported_languages) = extract_field_list(block, "supported_languages") else {
            continue;
        };

        rows.push(FormatterPresetDocRow {
            preset_name: name,
            description,
            homepage,
            supported_languages,
            cmd,
            args,
            mode,
        });
    }

    rows.sort_by(|a, b| a.preset_name.cmp(&b.preset_name));

    let mut out = String::new();
    out.push_str("<!-- AUTO-GENERATED by build.rs -->\n");
    for row in rows {
        out.push_str(&format!("## `{}`\n\n", row.preset_name));
        out.push_str(&format!("{}\n\n", row.description));
        out.push_str("Homepage\n");
        out.push_str(&format!(":   <{}>\n\n", row.homepage));
        out.push_str("Supported Languages\n");
        out.push_str(&format!(
            ":   {}\n\n",
            format_languages_for_docs(&row.supported_languages)
        ));
        out.push_str("Command\n");
        out.push_str(&format!(":   `{}`\n\n", row.cmd));
        out.push_str("`args`\n");
        out.push_str(&format!(":   `{}`\n\n", format_args(&row.args)));
        out.push_str("Type\n");
        out.push_str(&format!(":   {}\n\n", row.mode));
    }

    let output_path = docs_dir.join("_formatter-presets-details.qmd");
    fs::write(&output_path, out)?;
    println!("Generated external formatter presets: {:?}", output_path);

    Ok(())
}

fn generate_external_linter_table() -> Result<()> {
    // Skip during cargo package/publish - file should be committed to git
    let is_packaging = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.contains("/target/package/")))
        .unwrap_or(false);
    if is_packaging {
        return Ok(());
    }

    let linters_path = PathBuf::from("src/linter/external_linters.rs");
    let docs_dir = PathBuf::from("docs/reference");
    if !linters_path.exists() || !docs_dir.exists() {
        return Ok(());
    }

    let source = fs::read_to_string(&linters_path)?;
    let mut rows: Vec<LinterDocRow> = Vec::new();

    for chunk in source.split("LinterInfo {").skip(1) {
        let Some(block_end) = chunk.find("\n        );") else {
            continue;
        };
        let block = &chunk[..block_end];

        let Some(name) = extract_field_string(block, "name") else {
            continue;
        };
        let Some(description) = extract_field_string(block, "description") else {
            continue;
        };
        let Some(homepage) = extract_field_string(block, "url") else {
            continue;
        };
        let Some(cmd) = extract_field_string(block, "command") else {
            continue;
        };
        let Some(args) = extract_field_list(block, "args") else {
            continue;
        };
        let Some(supported_languages) = extract_field_list(block, "supported_languages") else {
            continue;
        };

        rows.push(LinterDocRow {
            name,
            description,
            homepage,
            supported_languages,
            cmd,
            args,
        });
    }

    rows.sort_by(|a, b| a.name.cmp(&b.name));

    let mut out = String::new();
    out.push_str("<!-- AUTO-GENERATED by build.rs -->\n");
    for row in rows {
        out.push_str(&format!("## `{}`\n\n", row.name));
        out.push_str(&format!("{}\n\n", row.description));
        out.push_str("Homepage\n");
        out.push_str(&format!(":   <{}>\n\n", row.homepage));
        out.push_str("Supported Languages\n");
        out.push_str(&format!(
            ":   {}\n\n",
            format_languages_for_docs(&row.supported_languages)
        ));
        out.push_str("Command\n");
        out.push_str(&format!(":   `{}`\n\n", row.cmd));
        out.push_str("`args`\n");
        out.push_str(&format!(":   `{}`\n\n", format_args(&row.args)));
    }

    let output_path = docs_dir.join("_linter-presets-details.qmd");
    fs::write(&output_path, out)?;
    println!("Generated external linter presets: {:?}", output_path);

    Ok(())
}

fn format_see_also(refs: &[String]) -> String {
    let formatted: Vec<String> = refs.iter().map(|r| format!("\\fB{}\\fR(1)", r)).collect();
    format!(".SH \"SEE ALSO\"\n{}\n", formatted.join(", "))
}

fn generate_man_pages() -> Result<()> {
    // Create man directory if it doesn't exist
    let out_dir = PathBuf::from("target/man");
    fs::create_dir_all(&out_dir)?;

    // Generate main man page and all subcommand pages (like git/cargo do)
    let cmd = Cli::command();

    // Collect top-level subcommand names (skip "help") for SEE ALSO sections
    let subcommand_names: Vec<String> = cmd
        .get_subcommands()
        .filter(|s| s.get_name() != "help")
        .map(|s| format!("panache-{}", s.get_name()))
        .collect();

    // Generate main page
    let man = Man::new(cmd.clone());
    let mut buffer = Vec::new();
    man.render(&mut buffer)?;
    let main_content =
        String::from_utf8_lossy(&buffer).into_owned() + &format_see_also(&subcommand_names);
    fs::write(out_dir.join("panache.1"), main_content.as_bytes())?;

    // Generate pages for each top-level subcommand
    for subcommand in cmd.get_subcommands() {
        let subcommand_name = subcommand.get_name();
        if subcommand_name == "help" {
            continue; // Skip help command
        }

        let name = format!("panache-{}", subcommand_name);
        let man = Man::new(subcommand.clone().version(env!("CARGO_PKG_VERSION"))).title(&name);
        let mut buffer = Vec::new();
        man.render(&mut buffer)?;

        // Post-process: fix NAME, SYNOPSIS, and nested subcommand references
        let content = String::from_utf8_lossy(&buffer);
        let fixed_content = content
            .replace(
                &format!("{} \\-", subcommand_name),
                &format!("{} \\-", name),
            )
            .replace(
                &format!("\\fB{}\\fR", subcommand_name),
                &format!("\\fBpanache {}\\fR", subcommand_name),
            )
            .replace(
                &format!("{}\\-", subcommand_name),
                &format!("panache\\-{}\\-", subcommand_name),
            );

        // SEE ALSO: panache(1) plus sibling subcommand pages
        let mut see_also_refs: Vec<String> = vec!["panache".to_string()];
        see_also_refs.extend(subcommand_names.iter().filter(|n| *n != &name).cloned());
        let with_see_also = fixed_content + &format_see_also(&see_also_refs);

        fs::write(
            out_dir.join(format!("{}.1", name)),
            with_see_also.as_bytes(),
        )?;

        // Generate pages for nested subcommands (e.g., daemon start -> panache-daemon-start)
        for nested in subcommand.get_subcommands() {
            let nested_name = nested.get_name();
            if nested_name == "help" {
                continue;
            }

            let full_name = format!("panache-{}-{}", subcommand_name, nested_name);
            let man = Man::new(nested.clone().version(env!("CARGO_PKG_VERSION"))).title(&full_name);
            let mut buffer = Vec::new();
            man.render(&mut buffer)?;

            // Post-process nested pages: fix NAME and SYNOPSIS sections
            let content = String::from_utf8_lossy(&buffer);
            let fixed_content = content
                .replace(
                    &format!("{} \\-", nested_name),
                    &format!("{} \\-", full_name),
                )
                .replace(
                    &format!("\\fB{}\\fR", nested_name),
                    &format!("\\fBpanache {} {}\\fR", subcommand_name, nested_name),
                );

            // SEE ALSO: parent subcommand page and panache(1)
            let see_also_refs = vec![
                format!("panache-{}", subcommand_name),
                "panache".to_string(),
            ];
            let with_see_also = fixed_content + &format_see_also(&see_also_refs);

            fs::write(
                out_dir.join(format!("{}.1", full_name)),
                with_see_also.as_bytes(),
            )?;
        }
    }

    Ok(())
}

fn main() -> Result<()> {
    // Generate shell completions
    if let Some(outdir) = env::var_os("OUT_DIR") {
        generate_completions(&outdir)?;
    }

    // Generate man pages
    generate_man_pages()?;

    // Generate CLI markdown documentation
    generate_cli_markdown()?;
    generate_external_formatter_table()?;
    generate_external_linter_table()?;

    println!("cargo:rerun-if-changed=src/cli.rs");
    println!("cargo:rerun-if-changed=src/config/formatter_presets.rs");
    println!("cargo:rerun-if-changed=src/linter/external_linters.rs");
    println!("cargo:rerun-if-changed=build.rs");

    Ok(())
}
