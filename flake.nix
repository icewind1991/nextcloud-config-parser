{
  inputs = {
    utils.url = "github:numtide/flake-utils";
    nixpkgs.url = "nixpkgs/release-23.11";
    naersk.url = "github:nix-community/naersk";
    naersk.inputs.nixpkgs.follows = "nixpkgs";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
    rust-overlay.inputs.flake-utils.follows = "utils";
  };

  outputs = {
    self,
    nixpkgs,
    utils,
    naersk,
    rust-overlay,
  }:
    utils.lib.eachDefaultSystem (system: let
      overlays = [(import rust-overlay)];
      pkgs = (import nixpkgs) {
        inherit system overlays;
      };

      inherit (pkgs) lib callPackage rust-bin mkShell;
      inherit (lib.sources) sourceByRegex;

      msrv = (fromTOML (readFile ./Cargo.toml)).package.rust-version;
      inherit (builtins) fromTOML readFile;
      toolchain = rust-bin.stable.latest.default;
      msrvToolchain = rust-bin.stable."${msrv}".default;

      naersk' = callPackage naersk {
        rustc = toolchain;
        cargo = toolchain;
      };
      msrvNaersk = callPackage naersk {
        rustc = msrvToolchain;
        cargo = msrvToolchain;
      };

      src = sourceByRegex ./. ["Cargo.*" "(src|derive|benches|tests|examples)(/.*)?"];
      nearskOpt = {
        pname = "nextcloud-config-parser";
        root = src;
      };
    in rec {
      packages = {
        check = naersk'.buildPackage (nearskOpt
          // {
            mode = "check";
          });
        checkAll = naersk'.buildPackage (nearskOpt
          // {
            mode = "check";
            cargoBuildOptions = x: x++ ["--all-features"];
          });
        clippy = naersk'.buildPackage (nearskOpt
          // {
            mode = "clippy";
            cargoBuildOptions = x: x++ ["--all-features"];
          });
        test = naersk'.buildPackage (nearskOpt
          // {
            release = false;
            mode = "test";
          });
        testAll = naersk'.buildPackage (nearskOpt
          // {
            release = false;
            mode = "test";
            cargoTestOptions = x: x++ ["--all-features"];
          });
        msrv = msrvNaersk.buildPackage (nearskOpt
          // {
            mode = "check";
            cargoBuildOptions = x: x++ ["--all-features"];
          });
      };

      # `nix develop`
      devShell = pkgs.mkShell {
        nativeBuildInputs = with pkgs; [rustc cargo bacon cargo-edit cargo-outdated clippy cargo-audit cargo-msrv cargo-semver-checks];
      };
    });
}
