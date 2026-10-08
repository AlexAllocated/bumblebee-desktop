{
  description = "Bumblebee desktop: native Rust, Tauri and Svelte development";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  outputs = { self, nixpkgs }: let
    system = "x86_64-linux";
    pkgs = import nixpkgs { inherit system; };
    native = with pkgs; [ pkg-config cmake clang rustc cargo bun nodejs_24 makeWrapper ];
    libraries = with pkgs; [ gtk3 webkitgtk_4_1 libsoup_3 glib openssl dbus libayatana-appindicator alsa-lib libsecret libuuid stdenv.cc.cc.lib llvmPackages.libcxx ];
  in {
    devShells.${system}.default = pkgs.mkShell {
      nativeBuildInputs = native;
      buildInputs = libraries;
      LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
      LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath libraries;
      GIO_MODULE_DIR = "${pkgs.glib-networking}/lib/gio/modules";
      shellHook = ''export XDG_DATA_DIRS="${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}:${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}:$XDG_DATA_DIRS"'';
    };
    # A prepared release can be installed declaratively without a compiler or Node runtime.
    # See docs/packaging.md; hashes deliberately must be pinned by the consumer.
    lib.packageAppImage = { url, sha256, version }: pkgs.appimageTools.wrapType2 {
      pname = "bumblebee-desktop"; inherit version;
      src = pkgs.fetchurl { inherit url sha256; };
      extraPkgs = p: [ p.libsecret p.alsa-lib p.libayatana-appindicator p.libuuid p.llvmPackages.libcxx ];
    };
  };
}
