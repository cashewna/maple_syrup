{
  description = "maple_syrup — cleanroom Maple-inspired Symbolic CAS & GPUI Worksheet";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs =
    { self, nixpkgs, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems =
        f: nixpkgs.lib.genAttrs systems (system: f system (import nixpkgs { inherit system; }));
      runtimeLibsFor =
        pkgs: with pkgs; [
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
      packages = forAllSystems (
        _system: pkgs:
        let
          runtimeLibs = runtimeLibsFor pkgs;
          desktopItem = pkgs.makeDesktopItem {
            name = "maple_syrup";
            exec = "maple_syrup";
            desktopName = "Maple Syrup (GPUI)";
            comment = "Cleanroom symbolic CAS and interactive GPUI worksheet";
            categories = [
              "Education"
              "Science"
              "Math"
            ];
            terminal = false;
          };
          maple_syrup = pkgs.rustPlatform.buildRustPackage {
            pname = "maple_syrup";
            version = "0.1.0";
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;

            nativeBuildInputs = with pkgs; [
              pkg-config
              cmake
              clang
              llvmPackages.libclang
              makeWrapper
            ];
            buildInputs = runtimeLibs;

            env.LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";

            postInstall = ''
              mkdir -p $out/share/applications
              cp ${desktopItem}/share/applications/* $out/share/applications/
              wrapProgram $out/bin/maple_syrup \
                --prefix LD_LIBRARY_PATH : "${pkgs.lib.makeLibraryPath runtimeLibs}"
            '';

            meta = with pkgs.lib; {
              description = "Cleanroom Maple-inspired symbolic CAS and interactive GPUI worksheet";
              license = licenses.mit;
              mainProgram = "maple_syrup";
            };
          };
        in
        {
          inherit maple_syrup;
          default = maple_syrup;
        }
      );

      apps = forAllSystems (
        system: _pkgs: {
          default = {
            type = "app";
            program = "${self.packages.${system}.default}/bin/maple_syrup";
          };
        }
      );

      devShells = forAllSystems (
        _system: pkgs:
        let
          runtimeLibs = runtimeLibsFor pkgs;
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
