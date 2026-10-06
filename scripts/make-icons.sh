#!/usr/bin/env bash
# Renders nori's app icon from brand/icon.svg (the lsuite icon template; scripts/gen-mark.py writes
# it) into every format the packages need, and writes them to the repository (run it after
# changing the SVG, then commit):
#   brand/icon.png                                 1024 px
#   crates/nori-desktop/resources/nori.icns        macOS (iconutil on a Mac, else written here)
#   crates/nori-desktop/resources/nori.ico         Windows (16–256 px, PNG-compressed)
#   crates/nori-desktop/resources/nori.png         Linux (512 px; .desktop, AppImage, .deb)
# Needs resvg (`cargo install resvg`) or rsvg-convert (`brew install librsvg`), or else node and
# npm (it then downloads @resvg/resvg-js once into ~/.cache/nori-icons); and python3.
set -euo pipefail
cd "$(dirname "$0")/.."
svg=brand/icon.svg
out=crates/nori-desktop/resources
mkdir -p "$out"

resvg_js() { # Installs @resvg/resvg-js into a cache folder the first time.
  local dir="${XDG_CACHE_HOME:-$HOME/.cache}/nori-icons"
  if [ ! -d "$dir/node_modules/@resvg/resvg-js" ]; then
    mkdir -p "$dir"
    (cd "$dir" && npm install --silent --no-audit --no-fund @resvg/resvg-js@2 > /dev/null)
  fi
  echo "$dir/node_modules/@resvg/resvg-js"
}

render() { # size output
  if command -v resvg > /dev/null; then
    resvg -w "$1" -h "$1" "$svg" "$2"
  elif command -v rsvg-convert > /dev/null; then
    rsvg-convert -w "$1" -h "$1" "$svg" -o "$2"
  elif command -v node > /dev/null && command -v npm > /dev/null; then
    node -e '
      const [mod, inp, size, outp] = process.argv.slice(1);
      const { Resvg } = require(mod);
      const fs = require("fs");
      const r = new Resvg(fs.readFileSync(inp), { fitTo: { mode: "width", value: +size } });
      fs.writeFileSync(outp, r.render().asPng());' "$(resvg_js)" "$svg" "$1" "$2"
  else
    echo "Install resvg (cargo install resvg), rsvg-convert or node to render the icon." >&2
    exit 1
  fi
}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

render 1024 brand/icon.png
render 512 "$out/nori.png"

# Each size is rendered from the SVG, not scaled down, so small sizes stay sharp.
set_dir="$work/nori.iconset"
mkdir -p "$set_dir"
for size in 16 32 128 256 512; do
  render "$size" "$set_dir/icon_${size}x${size}.png"
  render $((size * 2)) "$set_dir/icon_${size}x${size}@2x.png"
done
if command -v iconutil > /dev/null; then
  iconutil -c icns "$set_dir" -o "$out/nori.icns"
else
  python3 - "$set_dir" "$out/nori.icns" <<'PY'
# An .icns whose entries are PNGs (macOS 10.7 and later), the types iconutil writes.
import struct, sys
src, out = sys.argv[1], sys.argv[2]
types = [("icp4", "16x16"), ("ic11", "16x16@2x"), ("icp5", "32x32"), ("ic12", "32x32@2x"),
         ("ic07", "128x128"), ("ic13", "128x128@2x"), ("ic08", "256x256"), ("ic14", "256x256@2x"),
         ("ic09", "512x512"), ("ic10", "512x512@2x")]
body = b""
for kind, name in types:
    data = open(f"{src}/icon_{name}.png", "rb").read()
    body += kind.encode() + struct.pack(">I", 8 + len(data)) + data
open(out, "wb").write(b"icns" + struct.pack(">I", 8 + len(body)) + body)
PY
fi

sizes=(16 24 32 48 64 128 256)
for size in "${sizes[@]}"; do render "$size" "$work/ico-$size.png"; done
python3 - "$out/nori.ico" "${sizes[@]/#/$work/ico-}" <<'PY'
# An .ico whose entries are PNGs (Windows Vista and later), largest last.
import struct, sys
out, files = sys.argv[1], [f + ".png" for f in sys.argv[2:]]
images = [open(f, "rb").read() for f in files]
header = struct.pack("<HHH", 0, 1, len(images))
offset = 6 + 16 * len(images)
entries = b""
for f, data in zip(files, images):
    w, h = struct.unpack(">II", data[16:24])
    entries += struct.pack("<BBBBHHII", w % 256, h % 256, 0, 0, 1, 32, len(data), offset)
    offset += len(data)
open(out, "wb").write(header + entries + b"".join(images))
PY
ls -l brand/icon.png "$out"/nori.*
