{ pkgs ? import <nixpkgs> { overlays = [ (import (builtins.fetchTarball https://github.com/mozilla/nixpkgs-mozilla/archive/master.tar.gz)) ]; } }:

pkgs.mkShell {
  buildInputs = with pkgs; [
    # needed to compile openssl-sys crate
    pkg-config
    openssl

    (latest.rustChannels.stable.rust.override {
      extensions = ["rust-src"];
    })
  ];
}
