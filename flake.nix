{
  description = "maple-gpui — cleanroom Maple-inspired Symbolic CAS & GPUI Worksheet";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs =
    { nixpkgs, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems =
        f: nixpkgs.lib.genAttrs systems (system: f (import nixpkgs { inherit system; }));
    in
    {
      devShells = forAllSystems (
        pkgs:
        let
          runtimeLibs = with pkgs; [
            libGL
            vulkan-loader
            wayland
            libxkbcommon
            libxcb
            libx11
            libxext
            fontconfig
            freetype
            openssl
            glib
          ];
        in
        {
          default = pkgs.mkShell {
            nativeBuildInputs = with pkgs; [
              cargo
              rustc
              rustfmt
              clippy
              rust-analyzer
              pkg-config
              cmake
              clang
              llvmPackages.libclang
              bashInteractive
            ];

            buildInputs = runtimeLibs;

            env = {
              LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
              LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
              SHELL = "${pkgs.bashInteractive}/bin/bash";
            };
          };
        }
      );
    };
}
