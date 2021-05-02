{ pkgs ? import <nixpkgs> { overlays = [ (import (builtins.fetchTarball https://github.com/mozilla/nixpkgs-mozilla/archive/master.tar.gz)) ]; } }:

pkgs.mkShell {
  buildInputs = with pkgs; [
    (latest.rustChannels.stable.rust.override {
      extensions = ["rust-src"];
    })
  ];
}
