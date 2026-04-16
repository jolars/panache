{
  pkgs,
  ...
}:

{
  packages = [
    pkgs.bashInteractive
    pkgs.perf
    pkgs.cargo-flamegraph
    pkgs.cargo-llvm-cov
    pkgs.cargo-audit
    pkgs.cargo-deny
    pkgs.cmark
    pkgs.go-task
    pkgs.jarl
    pkgs.llvmPackages.bintools
    pkgs.prettier
    pkgs.quartoMinimal
    pkgs.air-formatter
    pkgs.ruff
    pkgs.shfmt
    pkgs.wasm-pack
    pkgs.stylua
    pkgs.shellcheck
    pkgs.eslint
    pkgs.go-tools
    pkgs.yamlfmt
    pkgs.go
    pkgs.vsce
    (pkgs.rWrapper.override {
      packages = with pkgs.rPackages; [
        knitr
        rmarkdown
        bookdown
      ];
    })
  ];

  languages = {
    rust = {
      enable = true;

      toolchainFile = ./rust-toolchain.toml;
    };

    javascript = {
      enable = true;

      # corepack.enable = true;

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
