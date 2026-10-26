{
  description = "Torogao - a 2D puppet authoring tool for games";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
  };

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          # The canonical build environment. Everything the
          # vendored build needs is here: the D toolchain, the
          # CMake/native dependencies of the vendored i2d-imgui
          # submodules (SDL, GLFW, FreeType, glbinding all build
          # from the pinned sources in third-party/), and the
          # OpenGL/X11/Wayland libraries the editor links and
          # dlopens against.
          default = pkgs.mkShell {
            packages = with pkgs; [
              ldc
              dub
              cmake
              pkg-config
              git
              SDL2
              libGL
              freetype
              dbus
              xorg.libX11
              xorg.libXext
              xorg.libXrandr
              xorg.libXi
              xorg.libXcursor
              xorg.libXinerama
              wayland
              libxkbcommon
            ];

            # The vendored GLFW/SDL builds dlopen the X11 and
            # Wayland client libraries at runtime; make them
            # findable inside the shell.
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (with pkgs; [
              libGL
              xorg.libX11
              xorg.libXext
              xorg.libXrandr
              xorg.libXi
              xorg.libXcursor
              xorg.libXinerama
              wayland
              libxkbcommon
            ]);

            shellHook = ''
              echo "torogao dev shell: $(ldc2 --version | head -1), $(dub --version | head -1)"
            '';
          };
        });
    };
}
