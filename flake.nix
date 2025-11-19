{

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      nixpkgs,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };

        pkgDesc = (pkgs.lib.importTOML ./Cargo.toml).package;
        patreonServicePkg = pkgs.rustPlatform.buildRustPackage {
          pname = pkgDesc.name;
          version = pkgDesc.version;
          meta.mainProgram = pkgDesc.name;

          src = pkgs.lib.sources.cleanSource ./.;
          cargoLock.lockFile = ./Cargo.lock;
        };
      in
      {
        nixpkgs = pkgs;

        packages.default = patreonServicePkg;
        devShell = pkgs.mkShell {
          inputsFrom = [ patreonServicePkg ];
          buildInputs = with pkgs; [
            rustfmt
            clippy
          ];

          RUST_SRC_PATH = pkgs.rustPlatform.rustLibSrc;
        };

        dockerImage = pkgs.dockerTools.buildLayeredImage {
          name = pkgDesc.name;
          tag = "latest-${pkgs.stdenv.system}";

          config.Cmd = pkgs.lib.getExe patreonServicePkg;
        };
      }
    );
}
