# Smoke-test repositories

The [smoke-test workflow](../workflows/smoke-test.yml) builds Panache and runs
`scan.py` against the repositories in `repositories.toml`. The scanner checks
tracked `.md`, `.qmd`, and `.Rmd` files for losslessness and idempotency.

Add a repository with GitHub's `owner/repo` shorthand or a full HTTPS clone URL:

```toml
[[repositories]]
repo = "gohugoio/hugoDocs"
fallback-config = "configs/hugo.toml"
```

`fallback-config` is optional and defaults to `configs/default.toml`, which uses
Panache's built-in defaults. Paths are relative to the manifest. Keep fallback
files and any configs they extend in `configs/`, and use relative `extend` paths
so the files remain usable after downloading the artifact.

## Configuration precedence

The scanner sets `PANACHE_CONFIG` to the selected fallback's absolute path.
Panache first discovers the nearest upstream `.panache.toml`, `panache.toml`, or
`.config/panache.toml`, including configs in subdirectories. An upstream config
replaces the fallback completely; the two are not merged. Normal file-extension
inference still applies, so `.qmd` and `.Rmd` retain their respective flavors.

If an upstream config fails to parse, the scanner retries that file with
`--config` pointing to the same repository fallback. This preserves the existing
handling of obsolete upstream settings: a failure becomes a regression record
only if it also occurs with a valid config.

The shared fallbacks use ordinary Panache configuration:

  | Config             | Behavior when no upstream config exists                                       |
  | ---                | ---                                                                           |
  | `default.toml`     | Built-in defaults.                                                            |
  | `github-docs.toml` | GFM for conventional GitHub documentation paths; built-in defaults elsewhere. |
  | `hugo.toml`        | Hugo, with the GitHub documentation mappings.                                 |
  | `quarto.toml`      | Quarto, with the GitHub documentation mappings.                               |
  | `rmarkdown.toml`   | R Markdown, with the GitHub documentation mappings.                           |
  | `pandoc.toml`      | Pandoc, with the GitHub documentation mappings.                               |

`github-docs.toml` maps READMEs, contributing guides, and other conventional
repository documentation to GFM. The other fallbacks extend it so a GitHub
README does not inherit the site's Markdown flavor.

## Repository audit

The initial manifest records the build files inspected in all 40 upstream
repositories on October 15, 2026. Quarto projects declare `_quarto.yml`;
bookdown projects declare their output formats in `_output.yml`; Hugo projects
configure Hugo and Goldmark. These declarations determine fallback selection.
The comments beside each repository identify the evidence and any upstream
Panache config.

The GitLab `umwelt-info/website` repository currently declares
`flavor = "commonmark"` in its own `panache.toml`, so CommonMark takes
precedence over its Hugo fallback. Likewise, `math-ku/compstat` declares GFM
despite using Quarto to build the site. The scanner preserves both choices.

`rdpeng/rprogdatascience` uses knitr to produce a Leanpub manuscript. Panache
has no dedicated Leanpub flavor, so its manuscript keeps the built-in defaults
while its GitHub documentation receives the GFM mappings.

## Local runs and tests

Run from the Panache repository root with Python 3.11 or newer and fresh output
directories:

```bash
cargo build
python3 .github/smoke-test/scan.py \
    --panache-bin target/debug/panache \
    --results-dir "$(mktemp -d)" \
    --repos-dir "$(mktemp -d)"
```

Use `--manifest` to select a smaller manifest for a local investigation. Its
fallback configs must live in the adjacent `configs/` directory.

The scanner records regressions in `failures.tsv` and leaves the exit-status
decision to the workflow's issue job. It classifies failures from Panache's
diagnostic lines so words in document diffs do not create extra failure records.
The scanner attempts each clone up to three times, waiting 5 and 10 seconds
between attempts and removing incomplete checkouts before retrying. If all three
attempts fail, or another Git or process-launch error occurs, the scan fails.
The workflow summarizes any partial results and uploads the available artifacts
even when the scan fails. Upstream checkouts stay outside the results directory.
The uploaded results contain diagnostics, pass dumps, fallback configs, and
reproduction scripts.

Issue bodies and follow-up comments include the reproduction script for a sample
file. Run it from the extracted `debug-format-repo-scan-results` artifact
directory. It checks out the recorded upstream commit and uses the archived
fallback, including `--config` when the scan had to bypass an invalid upstream
config.

Run the offline scanner tests with:

```bash
python3 -m unittest discover -s .github/smoke-test
```

The Linux build-and-test job also runs these tests.
