{
  pkgs,
  ...
}:

{
  packages = [
    pkgs.go-task
    pkgs.quartoMinimal
    pkgs.wasm-pack
    pkgs.llvmPackages.bintools
    pkgs.bashInteractive
    pkgs.cmark
    pkgs.shfmt
    pkgs.pandoc
    pkgs.jarl
    pkgs.air
    pkgs.yamlfmt
    pkgs.ruff
    (pkgs.rWrapper.override {
      packages = with pkgs.rPackages; [
        knitr
        rmarkdown
        bookdown
      ];
    })
  ];

  languages.rust = {
    enable = true;
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
    };
  };
}
