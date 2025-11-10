{
  description = "Rust Development Shell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      nixpkgs,
      rust-overlay,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ rust-overlay.overlays.default ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

      in
      with pkgs;
      {
        devShells.default = mkShell {
          buildInputs = [
            openssl
            pkg-config
            (rust-bin.beta.latest.default.override { extensions = [ "rust-src" ]; })
          ];

          shellHook = ''
          '';
        };
      }
    );
}
