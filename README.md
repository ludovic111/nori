# nori

Part of [lsuite](https://lsuite.xyz) — [lsuite.xyz/nori](https://lsuite.xyz/nori)

**Pictures, drawings and pages in one app.** nori retouches photos with layers, masks and
adjustment layers, draws with the pen, shapes and gradients, and lays out posters and booklets
with text that flows from page to page — in one window, with one undo history your agent shares.
Free and open source (MIT), written in Rust, in beta.

## What it does

- **Photos and painting.** PNG, JPEG, WebP, TIFF, BMP, GIF; pixel layers with blend modes,
  opacity, masks and clipping; adjustment layers (Levels, Curves, Hue/Saturation, Exposure,
  Brightness/Contrast, Vibrance, Black & White, Invert, Threshold, Posterize, and Color Lookup
  from `.cube` files); brush and eraser (hardness, flow, pen pressure, GIMP brush tips); paint
  bucket, gradients; selections (rectangle, ellipse, lasso, magic wand, from a layer; grow,
  shrink, feather); filters (Gaussian and motion blur, sharpen, noise, pixelate); crop, image and
  canvas size, transforms.
- **Drawings.** Shapes with live corners (rectangle, ellipse, polygon, star, line), the pen for
  Bézier paths and direct selection of their points, fills, strokes, dashes and gradients, path
  operations (unite, subtract, intersect, exclude). SVG in and out.
- **Pages.** Several pages or artboards in one document, master pages, margins, columns and
  guides; text frames whose words flow from frame to frame across pages; paragraph and character
  styles; page numbers; PDF export with every page, shapes and text kept as vectors.
- **Coming from another editor.** Photoshop documents open with their layers (also what
  Affinity Photo, Pixelmator Pro and Photopea save); OpenRaster from Krita and GIMP; SVG from
  Illustrator, Inkscape, Figma, Affinity Designer, Canva and Scribus. Adobe swatches (`.ase`) and
  GIMP brushes (`.gbr`) import too.
- **Your agent.** The Agent panel (⌘J) runs **lsuite AI** (sign in, nothing to set up) or your own
  Claude Code, Codex, Anthropic or OpenAI key, or a model on Ollama. It works through the same
  commands as the window; each change is a step you can undo, and a run can be reverted at once.
  `nori-mcp` gives every command to any MCP client; `nori-cli` to scripts. See
  [docs/AI_CONTROL.md](docs/AI_CONTROL.md) and [docs/COMMANDS.md](docs/COMMANDS.md).
- **Plugins.** Filters written in Rust with the nori SDK (`crates/nori-plugin`) load without a
  restart. Describe one in Plugins › Build with your agent and the agent writes, builds and
  installs it. Two examples ship in `plugins/`: Duotone and Halftone.
- **lsuite.** Send a picture straight to kimchi's timeline (`handoff.toKimchi`); nori tells the
  other lsuite apps it is here (`~/.lsuite/apps/nori.json`).

## The file

A `.nori` document is a zip of `document.json` and PNGs: open it with any zip tool. See
[docs/FILE_FORMAT.md](docs/FILE_FORMAT.md).

## Build

```sh
cargo run -p nori                 # the app
cargo run -p nori-cli -- --help   # the CLI
cargo test --workspace
scripts/bundle-macos.sh           # nori.app and a .dmg (on a Mac)
```

Linux needs GPUI's usual libraries (see `.github/workflows/ci.yml`).

## Support

nori is free. If it helps you, [support it](https://lsuite.xyz/nori/support).

Licence: MIT. Fonts: Chakra Petch, Manrope and IBM Plex Mono (SIL OFL). Icons: Lucide (ISC).
The logos in `crates/nori-desktop/assets/logos` belong to their owners
([sources](crates/nori-desktop/assets/logos/SOURCES.md)).
