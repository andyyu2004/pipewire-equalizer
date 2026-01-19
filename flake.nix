{
  description = "PipeWire Equalizer development environment";

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
        pkgs = nixpkgs.legacyPackages.${system};
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            pipewire.dev
            pkg-config
            libclang.lib
          ];

          shellHook = ''
            export LIBCLANG_PATH="${pkgs.libclang.lib}/lib"
            echo "PipeWire development environment loaded"
            echo "PipeWire version: ${pkgs.pipewire.version}"
          '';
        };
      }
    );
}
