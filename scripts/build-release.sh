#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
binary=target/release/vimouse
if [ "${1-}" = --static ]; then
    shift
    target="$(uname -m)-unknown-linux-musl"
    cargo build --release --locked --target "$target" "$@"
    binary="target/$target/release/vimouse"
else
    cargo build --release --locked "$@"
fi
release_dir=$(mktemp -d)
trap 'rm -rf "$release_dir"' EXIT HUP INT TERM
bundle="vimouse-linux-$(uname -m)"
mkdir -p "$release_dir/$bundle"
install -m755 "$binary" "$release_dir/$bundle/vimouse"
mkdir -p dist
archive="dist/$bundle.tar.gz"
tar -czf "$archive" -C "$release_dir" "$bundle"
# dist contains generated outputs; replace the old expanded bundle as well,
# so the documented direct-test path always runs the just-built executable.
rm -rf -- "dist/$bundle"
cp -R "$release_dir/$bundle" "dist/$bundle"
printf '%s\n' "Built $archive"
