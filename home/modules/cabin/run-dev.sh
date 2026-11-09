#!/bin/sh
# Dev wrapper: a binary of the local release build (cabind by default; e.g.
# `run-dev.sh cabin-applet --kind status`), with the dlopen library path
# the nix build bakes into its RUNPATH (wayland, xkbcommon, vulkan, GL) and
# the icon dirs.
here=$(dirname "$(realpath "$0")")
. "$here/run-dev.env"
bin=${1:-cabind}
[ $# -gt 0 ] && shift
exec "$here/target/release/$bin" "$@"
