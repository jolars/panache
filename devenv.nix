{
  pkgs,
  ...
}:

{
  packages = [
    pkgs.air
    pkgs.bashInteractive
    pkgs.perf
    pkgs.cargo-flamegraph
    pkgs.cargo-llvm-cov
    pkgs.cmark
    pkgs.go-task
    pkgs.jarl
    pkgs.llvmPackages.bintools
    pkgs.pandoc
    pkgs.prettier
    pkgs.quartoMinimal
    pkgs.ruff
    pkgs.shfmt
    pkgs.wasm-pack
    pkgs.yamlfmt
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
        enable = true;
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

        entry = "panache format";

        language = "system";

        files = "\.(qmd|md|Rmd)$";

        excludes = [
          "^(pandoc|assets|tests)"
          "docs/cli.qmd"
        ];
      };

      eslint = {
        enable = true;
      };
    };
  };
}
