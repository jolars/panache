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

        quartofmt = pkgs.rustPlatform.buildRustPackage {
          pname = "quartofmt";
          version = "0.1.0";

          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          meta = with pkgs.lib; {
            description = "A formatter for Quarto, R Markdown, and Markdown files";
            homepage = "https://github.com/jolars/quartofmt";
            license = licenses.mit;
            maintainers = [ ];
          };
        };
      in
      {
        packages = {
          default = quartofmt;
          quartofmt = quartofmt;
        };

        apps = {
          default = {
            type = "app";
            program = "${quartofmt}/bin/quartofmt";
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
