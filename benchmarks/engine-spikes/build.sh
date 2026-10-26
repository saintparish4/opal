#!/usr/bin/env bash
# Builds every spike configuration into bin/, one executable each:
#
#   quickjs            quickjs-ng, bare bootstrap (console only)
#   quickjs-pad256     quickjs-ng, plus 256 KB of never-called bootstrap code
#   v8                 V8, bootstrap run on every start, no custom snapshot
#   v8-pad256          V8, 256 KB bootstrap run on every start
#   v8-snap            V8, bootstrap baked into a startup snapshot
#   v8-snap-pad256     V8, 256 KB bootstrap baked into the snapshot
#   v8-snap-jitless    V8 --jitless, snapshot: no machine code generated at run time
#
# A snapshot is written by a binary built with the same pad and V8 flags as
# the one that embeds it: V8 refuses a snapshot made under other flags.
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p bin

build() { # name package pad flags snapshot
  SPIKE_PAD_KB="$3" SPIKE_V8_FLAGS="$4" SPIKE_V8_SNAPSHOT="$5" \
    cargo build --release --quiet -p "$2"
  cp "target/release/$2" "bin/$1"
}
build_unset_snapshot() { # name package pad flags
  env -u SPIKE_V8_SNAPSHOT SPIKE_PAD_KB="$3" SPIKE_V8_FLAGS="$4" \
    cargo build --release --quiet -p "$2"
  cp "target/release/$2" "bin/$1"
}
snapshot() { # name pad flags
  build_unset_snapshot "$1.maker" spike-v8 "$2" "$3"
  "bin/$1.maker" --write-snapshot "target/$1.snapshot"
  rm "bin/$1.maker"
  build "$1" spike-v8 "$2" "$3" "$PWD/target/$1.snapshot"
}

build_unset_snapshot quickjs spike-quickjs 0 ""
build_unset_snapshot quickjs-pad256 spike-quickjs 256 ""
build_unset_snapshot v8 spike-v8 0 ""
build_unset_snapshot v8-pad256 spike-v8 256 ""
snapshot v8-snap 0 ""
snapshot v8-snap-pad256 256 ""
snapshot v8-snap-jitless 0 "--jitless"
ls -l bin
