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
mkdir -p "$release_dir/$bundle/packaging"
install -m755 "$binary" "$release_dir/$bundle/vimouse"
install -m755 packaging/install.sh "$release_dir/$bundle/install.sh"
cp config.example.toml README.md CHANGELOG.md LICENSE NOTICE packaging/THIRD-PARTY-NOTICES.txt "$release_dir/$bundle/"
cp -R docs "$release_dir/$bundle/"
cp packaging/70-vimouse.rules packaging/vimouse.service "$release_dir/$bundle/packaging/"
mkdir -p dist
archive="dist/$bundle.tar.gz"
tar -czf "$archive" -C "$release_dir" "$bundle"
printf '%s\n' "Built $archive"
