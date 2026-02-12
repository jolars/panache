{
  description = "A formatter for Quarto, R Markdown, and Markdown files";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
        };

        panache = pkgs.rustPlatform.buildRustPackage {
          pname = "panache";
          version = "1.0.0";

          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          nativeBuildInputs = [ pkgs.installShellFiles ];

          postInstall = ''
            installShellCompletion --cmd panache \
              --bash target/completions/panache.bash \
              --fish target/completions/panache.fish \
              --zsh target/completions/_panache

            installManPage target/man/*
          '';

          meta = with pkgs.lib; {
            description = "A formatter, linter, and LSP for Quarto, R Markdown, and Pandoc Markdown files";
            homepage = "https://github.com/jolars/panache";
            license = licenses.mit;
            maintainers = [ ];
          };
        };
      in
      {
        packages = {
          default = panache;
          panache = panache;
        };

        apps = {
          default = {
            type = "app";
            program = "${panache}/bin/panache";
          };
        };

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            cargo
            rustc
            rustfmt
            clippy
            rust-analyzer
            go-task
            quartoMinimal
            wasm-pack
            llvmPackages.bintools
          ];
        };
      }
    );
}
