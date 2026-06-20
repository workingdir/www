{
  description = "cwd: one Rust binary that is the whole of cwd.dev (HTTP, SSH, git-over-SSH)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      # Linux is what we deploy; darwin is handy for local dev.
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
        "x86_64-darwin"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f system);
      pkgsFor = system: nixpkgs.legacyPackages.${system};

      cwdFor =
        system:
        let
          pkgs = pkgsFor system;
        in
        pkgs.rustPlatform.buildRustPackage {
          pname = "cwd";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          # The product: HTTP website + SSH faux shell + git-over-SSH.
          buildFeatures = [ "ssh" ];

          # The git-over-SSH bridge shells out to `git`; make it always present.
          nativeBuildInputs = [ pkgs.makeWrapper ];
          postInstall = ''
            wrapProgram $out/bin/cwd \
              --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.git ]}
          '';

          meta = {
            description = "cwd.dev website, SSH faux shell, and git-over-SSH in one binary";
            mainProgram = "cwd";
          };
        };
    in
    {
      packages = forAllSystems (system: {
        default = cwdFor system;
        cwd = cwdFor system;
      });

      # `nix flake check` builds the binary on the host platform.
      checks = forAllSystems (system: {
        build = cwdFor system;
      });

      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
        in
        {
          default = pkgs.mkShell {
            packages = [
              pkgs.cargo
              pkgs.rustc
              pkgs.rustfmt
              pkgs.clippy
              pkgs.git
            ];
          };
        }
      );

      formatter = forAllSystems (system: (pkgsFor system).nixfmt-rfc-style);
    };
}
