{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  buildInputs = with pkgs; [ 
    rustc 
    cargo
    rustfmt
    rustPackages.clippy
    gcc 
    protobuf
    pkg-config
    glib
    graphene
    gtk4
    ];
  RUST_BACKTRACE = 1;

  shellHook = ''
    export CARGO_HOME=$HOME/.cargo
    export RUSTUP_HOME=$HOME/.rustup
  '';
}
