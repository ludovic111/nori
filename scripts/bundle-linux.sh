#!/usr/bin/env bash
# Builds nori for x86_64 Linux and packages it (run on Linux, e.g. Ubuntu 22.04 for an old
# enough glibc):
#   scripts/bundle-linux.sh
# Writes to target/dist/:
#   nori_amd64.AppImage   one-file app
#   nori_amd64.deb        Debian/Ubuntu package (/usr/lib/nori, /usr/bin/nori)
# These are the names lsuite.xyz/nori/download/linux-{appimage,deb} look for.
#
# The binaries look for shared libraries in ../lib first (rpath, like Zed), where the libraries
# ldd finds are copied, except glibc's and the GPU/display stack the system must provide.
#
# Environment (all optional):
#   APPIMAGETOOL       path to appimagetool (downloaded otherwise)
#   NORI_SKIP_BUILD=1 reuse the binaries already in target/<triple>/release
#   CARGO              the cargo to run (default: cargo)
set -euo pipefail
cd "$(dirname "$0")/.."

triple=x86_64-unknown-linux-gnu
version=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)
[ -n "$version" ] || { echo "No version in Cargo.toml [workspace.package]" >&2; exit 1; }
cargo=${CARGO:-cargo}
resources=crates/nori-desktop/resources
dist=target/dist
work="$dist/$triple"
bins=(nori nori-cli nori-mcp)

if [ "${NORI_SKIP_BUILD:-}" != 1 ]; then
  locked=()
  [ -n "${CI:-}" ] && locked=(--locked)
  packages=()
  for b in "${bins[@]}"; do packages+=(-p "$b"); done
  RUSTFLAGS="${RUSTFLAGS:-} -C link-args=-Wl,--disable-new-dtags,-rpath,\$ORIGIN/../lib" \
    "$cargo" build --release ${locked[@]+"${locked[@]}"} --target "$triple" "${packages[@]}"
fi

# One tree for both packages: bin/ (our binaries), lib/ (bundled libraries), share/.
tree="$work/nori"
rm -rf "$work"
mkdir -p "$tree/bin" "$tree/lib" "$tree/share/applications" "$tree/share/icons/hicolor/512x512/apps" \
  "$tree/share/mime/packages" "$tree/share/doc/nori"
for b in "${bins[@]}"; do
  cp "target/$triple/release/$b" "$tree/bin/$b"
  strip --strip-debug "$tree/bin/$b" || true
done
cp LICENSE "$tree/share/doc/nori/"
cp "$resources/nori.desktop" "$tree/share/applications/nori.desktop"
cp "$resources/nori.png" "$tree/share/icons/hicolor/512x512/apps/nori.png"
cp "$resources/nori-mime.xml" "$tree/share/mime/packages/nori.xml"

# Libraries: everything ldd resolves except glibc, the C++/GCC runtime, and the graphics and
# display stack (drivers must match the system's).
skip='^(linux-vdso|ld-linux|libc|libm|libdl|libpthread|librt|libresolv|libgcc_s|libstdc\+\+|libGL|libEGL|libGLX|libGLdispatch|libvulkan|libdrm|libgbm|libwayland|libX|libxcb|libxkbcommon|libasound|libdbus-1|libsystemd|libfontconfig|libfreetype|libexpat|libz)\.'
ldd "$tree/bin/nori" | awk '/=> \// { print $1 " " $3 }' | while read -r name path; do
  if ! printf '%s\n' "$name" | grep -Eq "$skip"; then
    cp -L "$path" "$tree/lib/"
    echo "bundled $name"
  fi
done

# ---- AppImage -------------------------------------------------------------
appdir="$work/nori.AppDir"
mkdir -p "$appdir/usr"
cp -a "$tree/bin" "$tree/lib" "$tree/share" "$appdir/usr/"
cp "$resources/nori.desktop" "$appdir/nori.desktop"
cp "$resources/nori.png" "$appdir/nori.png"
ln -s nori.png "$appdir/.DirIcon"
cat > "$appdir/AppRun" <<'APPRUN'
#!/bin/sh
here="$(dirname "$(readlink -f "$0")")"
exec "$here/usr/bin/nori" "$@"
APPRUN
chmod 755 "$appdir/AppRun"

tool=${APPIMAGETOOL:-}
if [ -z "$tool" ]; then
  tool="$work/appimagetool"
  curl -fsSL --retry 3 -o "$tool" https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
  chmod 755 "$tool"
fi
appimage="$dist/nori_amd64.AppImage"
rm -f "$appimage"
# No FUSE on CI runners: let the tool unpack itself.
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 VERSION="$version" "$tool" --no-appstream "$appdir" "$appimage"
chmod 755 "$appimage"

# ---- .deb -----------------------------------------------------------------
pkg="$work/deb"
mkdir -p "$pkg/DEBIAN" "$pkg/usr/lib/nori" "$pkg/usr/bin" "$pkg/usr/share"
cp -a "$tree/bin" "$tree/lib" "$pkg/usr/lib/nori/"
cp -a "$tree/share/." "$pkg/usr/share/"
for b in "${bins[@]}"; do ln -s "../lib/nori/bin/$b" "$pkg/usr/bin/$b"; done
size=$(du -sk "$pkg/usr" | cut -f1)
cat > "$pkg/DEBIAN/control" <<CONTROL
Package: nori
Version: $version
Section: editors
Priority: optional
Architecture: amd64
Maintainer: Ludovic Marie <ludovic111@users.noreply.github.com>
Installed-Size: $size
Depends: libc6 (>= 2.35), libvulkan1, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libx11-xcb1, libxcb1, libfontconfig1, libfreetype6, libdbus-1-3
Homepage: https://lsuite.xyz/nori
Description: Images, vectors and page layout
 nori is an open-source image and design app. Part of lsuite.
CONTROL
# The .nori type and the menu entry: refresh the system's caches when they exist.
cat > "$pkg/DEBIAN/postinst" <<'POSTINST'
#!/bin/sh
set -e
command -v update-mime-database > /dev/null && update-mime-database /usr/share/mime || true
command -v update-desktop-database > /dev/null && update-desktop-database -q /usr/share/applications || true
command -v gtk-update-icon-cache > /dev/null && gtk-update-icon-cache -q -t /usr/share/icons/hicolor || true
POSTINST
cp "$pkg/DEBIAN/postinst" "$pkg/DEBIAN/postrm"
chmod 755 "$pkg/DEBIAN/postinst" "$pkg/DEBIAN/postrm"
deb="$dist/nori_amd64.deb"
rm -f "$deb"
dpkg-deb --root-owner-group --build "$pkg" "$deb"

echo "Built nori $version for $triple:"
ls -lh "$appimage" "$deb"
