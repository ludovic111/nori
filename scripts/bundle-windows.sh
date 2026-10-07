#!/usr/bin/env bash
# Builds nori for x86_64 Windows and packages it (run in Git Bash on Windows, with NSIS's
# makensis on PATH or in its default folder, and 7-Zip; GitHub's windows runners have both):
#   scripts/bundle-windows.sh
# Writes to target/dist/:
#   nori_x64-setup.exe      per-user installer (lsuite.xyz/nori/download/windows-x86_64)
#   nori_x64-portable.zip   the same files, to run from any folder
#
# Environment (all optional):
#   NORI_SKIP_BUILD=1 reuse the binaries already in target/<triple>/release
#   CARGO              the cargo to run (default: cargo)
set -euo pipefail
cd "$(dirname "$0")/.."

triple=x86_64-pc-windows-msvc
version=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)
[ -n "$version" ] || { echo "No version in Cargo.toml [workspace.package]" >&2; exit 1; }
cargo=${CARGO:-cargo}
resources=crates/nori-desktop/resources
dist=target/dist
stage="$dist/$triple/nori"
bins=(nori nori-cli nori-mcp)

if [ "${NORI_SKIP_BUILD:-}" != 1 ]; then
  locked=()
  [ -n "${CI:-}" ] && locked=(--locked)
  packages=()
  for b in "${bins[@]}"; do packages+=(-p "$b"); done
  # Static CRT: no Visual C++ redistributable needed.
  RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+crt-static" \
    "$cargo" build --release ${locked[@]+"${locked[@]}"} --target "$triple" "${packages[@]}"
fi

rm -rf "$stage"
mkdir -p "$stage"
for b in "${bins[@]}"; do cp "target/$triple/release/$b.exe" "$stage/"; done
cp "$resources/nori.ico" "$stage/"
cp LICENSE "$stage/LICENSE.txt"

zip="$dist/nori_x64-portable.zip"
rm -f "$zip"
(cd "$dist/$triple" && 7z a -tzip -mx=9 "../nori_x64-portable.zip" nori > /dev/null)

makensis=$(command -v makensis || true)
[ -n "$makensis" ] || makensis="/c/Program Files (x86)/NSIS/makensis.exe"
setup="$dist/nori_x64-setup.exe"
rm -f "$setup"
win() { cygpath -w "$1" 2> /dev/null || printf '%s' "$1"; }
"$makensis" -V2 -DVERSION="$version" -DSRC="$(win "$PWD/$stage")" -DOUTFILE="$(win "$PWD/$setup")" \
  "$(win "$PWD/$resources/windows/installer.nsi")"

echo "Built nori $version for $triple:"
ls -lh "$setup" "$zip"
