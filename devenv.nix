{
  pkgs,
  ...
}:

let
  yamark = pkgs.rustPlatform.buildRustPackage rec {
    pname = "yamark";
    version = "0.3.0";

    src = pkgs.fetchFromGitHub {
      owner = "t-kalinowski";
      repo = "yamark";
      tag = "v${version}";
      hash = "sha256-xYTLtwPhtO+5CWC3YIL2d3azmeHduWwCmAlW2yVFw8g=";
    };

    cargoHash = "sha256-YJ2leB+bkC0l604ihk8PnDSz5rXKSfZaROM3kYYb3Yc=";

    # Run formatter/parser tests; benchmark tests require Git checkout metadata.
    cargoTestFlags = [
      "--test=mark_formatting_specs"
      "--test=yaml_scan"
    ];
  };
in
{
  packages = [
    pkgs.bashInteractive
    pkgs.google-lighthouse
    pkgs.perf
    pkgs.cargo-flamegraph
    pkgs.cargo-llvm-cov
    pkgs.cargo-audit
    pkgs.cargo-deny
    pkgs.cargo-machete
    pkgs.cmark
    pkgs.go-task
    pkgs.jarl
    pkgs.llvmPackages.bintools
    pkgs.rumdl
    yamark
    pkgs.mado
    pkgs.marksman
    pkgs.prettier
    pkgs.hyperfine
    pkgs.quartoMinimal
    pkgs.air-formatter
    pkgs.cargo-show-asm
    pkgs.samply
    pkgs.mystmd
    pkgs.ruff
    pkgs.shfmt
    pkgs.wasm-pack
    pkgs.stylua
    pkgs.markdownlint-cli
    pkgs.markdownlint-cli2
    pkgs.shellcheck
    pkgs.mdformat
    pkgs.eslint
    pkgs.go-tools
    pkgs.yamlfmt
    pkgs.go
    pkgs.vsce
    pkgs.maturin
    pkgs.dprint
    pkgs.dprint-plugins.dprint-plugin-toml
    (pkgs.rWrapper.override {
      packages = with pkgs.rPackages; [
        knitr
        rmarkdown
        bookdown
      ];
    })
  ];

  # Yamark 0.3.0 does not expose a version command.
  env.PANACHE_BENCH_YAMARK_VERSION = yamark.version;

  languages = {
    rust = {
      enable = true;

      toolchainFile = ./rust-toolchain.toml;
    };

    javascript = {
      enable = true;

      pnpm = {
        enable = true;

        install = {
          enable = true;
        };
      };
    };

    typescript = {
      enable = true;
    };

    python = {
      enable = true;

      package = pkgs.python3.withPackages (ps: [
        ps.markdown
        ps.myst-parser
      ]);
    };
  };

  git-hooks = {
    hooks = {
      clippy = {
        enable = false;
        settings = {
          allFeatures = true;
        };
      };

      rustfmt = {
        enable = true;
      };

      panache-format = {
        enable = true;

        name = "panache format";

        entry = "cargo run -- --config panache.toml format --force-exclude";

        language = "system";

        files = "\.(qmd|md|Rmd)$";
      };

      eslint = {
        enable = true;
      };
    };
  };
}
