#!/bin/sh
# Dev build: `cargo build --release` (or the cargo args given) in a shell with
# what it links. <nixpkgs> is the system's pinned nixpkgs. Don't reach it via
# builtins.getFlake on this repo's path: that's a path: flake, so every run
# copies the whole tree, .git and target/ (9 GB) included, into the store.
cd "$(dirname "$(realpath "$0")")" || exit
[ $# -eq 0 ] && set -- build --release
exec nix develop --impure --expr 'with import <nixpkgs> {}; mkShell { nativeBuildInputs = [pkg-config]; buildInputs = [libxkbcommon wayland wayland-protocols libgbm]; }' -c cargo "$@"
