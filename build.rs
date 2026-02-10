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

    // Generate shell completions
    for shell in [
        Shell::Bash,
        Shell::Fish,
        Shell::Zsh,
        Shell::PowerShell,
        Shell::Elvish,
    ] {
        generate_to(shell, &mut cmd, "panache", outdir)?;
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

    println!("cargo:rerun-if-changed=src/cli.rs");
    println!("cargo:rerun-if-changed=build.rs");

    Ok(())
}
