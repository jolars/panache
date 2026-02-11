use clap::CommandFactory;
use clap_complete::{Shell, generate_to};
use clap_mangen::Man;
use std::env;
use std::fs;
use std::io::Result;
use std::path::PathBuf;
use std::process::Command;

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
    let docs_dir = PathBuf::from("docs");

    // Only proceed if docs directory exists
    if !docs_dir.exists() {
        return Ok(());
    }

    let opts = clap_markdown::MarkdownOptions::default()
        .show_footer(false)
        .title(String::from(""))
        .show_table_of_contents(false);

    // Generate markdown documentation
    let markdown = clap_markdown::help_markdown_command_custom(&cmd, &opts);

    // // Build the complete document with frontmatter
    let mut document = String::new();
    document.push_str("---\n");
    document.push_str("title: CLI Reference\n");
    document.push_str("---\n\n");
    document.push_str(&markdown);

    // Write unformatted version first
    let output_path = docs_dir.join("cli.qmd");
    fs::write(&output_path, &document)?;

    // Try to format with panache if the binary exists
    // Check if panache binary exists in target/release or is in PATH
    let panache_bin = PathBuf::from("target/release/panache");
    if panache_bin.exists() {
        // Format the file in place using the panache binary
        let status = Command::new(&panache_bin)
            .arg("format")
            .arg("--write")
            .arg(&output_path)
            .status();

        match status {
            Ok(exit_status) if exit_status.success() => {
                println!("Generated and formatted CLI markdown: {:?}", output_path);
            }
            _ => {
                println!(
                    "Generated CLI markdown (formatting skipped): {:?}",
                    output_path
                );
            }
        }
    } else {
        println!(
            "Generated CLI markdown (panache binary not found, skipping format): {:?}",
            output_path
        );
    }

    Ok(())
}

fn generate_man_pages() -> Result<()> {
    // Create man directory if it doesn't exist
    let out_dir = PathBuf::from("target/man");
    fs::create_dir_all(&out_dir)?;

    // Generate main man page and all subcommand pages (like git/cargo do)
    let cmd = Cli::command();

    // Generate main page
    let man = Man::new(cmd.clone());
    let mut buffer = Vec::new();
    man.render(&mut buffer)?;
    fs::write(out_dir.join("panache.1"), buffer)?;

    // Generate pages for each top-level subcommand
    for subcommand in cmd.get_subcommands() {
        let subcommand_name = subcommand.get_name();
        if subcommand_name == "help" {
            continue; // Skip help command
        }

        let name = format!("panache-{}", subcommand_name);
        let man = Man::new(subcommand.clone()).title(&name);
        let mut buffer = Vec::new();
        man.render(&mut buffer)?;

        // Post-process to fix nested subcommand references
        let content = String::from_utf8_lossy(&buffer);
        let fixed_content = content.replace(
            &format!("{}\\-", subcommand_name),
            &format!("panache\\-{}\\-", subcommand_name),
        );

        fs::write(
            out_dir.join(format!("{}.1", name)),
            fixed_content.as_bytes(),
        )?;

        // Generate pages for nested subcommands (e.g., daemon start -> panache-daemon-start)
        for nested in subcommand.get_subcommands() {
            let nested_name = nested.get_name();
            if nested_name == "help" {
                continue;
            }

            let full_name = format!("panache-{}-{}", subcommand_name, nested_name);
            let man = Man::new(nested.clone()).title(&full_name);
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

            fs::write(
                out_dir.join(format!("{}.1", full_name)),
                fixed_content.as_bytes(),
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

    println!("cargo:rerun-if-changed=src/cli.rs");
    println!("cargo:rerun-if-changed=build.rs");

    Ok(())
}
