{ pkgs ? import <nixpkgs> {} }:

let
  unstable = import (builtins.fetchTarball https://nixos.org/channels/nixos-unstable/nixexprs.tar.xz) {};
in

pkgs.mkShell {
  buildInputs = with pkgs; [
    # needed to compile openssl-sys crate
    pkg-config
    openssl
  ] ++ (with unstable; [
    cargo
    clippy
    rust-analyzer
    rustc
    rustfmt
  ]);
}
