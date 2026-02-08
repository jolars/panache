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
    pkgs.pandoc
    pkgs.air
    pkgs.ruff
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
