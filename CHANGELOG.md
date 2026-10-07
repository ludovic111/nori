# Changelog

## 0.1.0 — 2026-10-07

nori's first release, in beta: one app for pictures, drawings and pages, part of lsuite.

- **Photos and painting.** Open PNG, JPEG, WebP, TIFF, BMP and GIF; layers with blend modes,
  opacity, masks and clipping; adjustment layers (Levels, Curves, Hue/Saturation, Exposure,
  Brightness/Contrast, Vibrance, Invert, Black & White, Threshold, Posterize, Color Lookup from
  `.cube` files); brush and eraser with hardness, flow and pen pressure; paint bucket, gradients,
  selections (rectangle, ellipse, lasso, magic wand, from a layer), filters (Gaussian and motion
  blur, sharpen, noise, pixelate), crop, image and canvas size, transforms.
- **Drawings.** Vector shapes with live corners, a pen for Bézier paths, fills, strokes and
  gradients, path operations (union, subtract, intersect, exclude), SVG in and out.
- **Pages.** Documents with several pages or artboards, master pages, margins, columns and
  guides; text frames whose words flow from frame to frame across pages; paragraph and
  character styles; page numbers; PDF export with every page, vectors kept as vectors.
- **Coming from another editor.** Photoshop documents open with their layers (also from
  Affinity Photo, Pixelmator Pro and Photopea); OpenRaster from Krita and GIMP; SVG from
  Illustrator, Inkscape, Figma, Affinity Designer, Canva and Scribus. GIMP brushes and Adobe
  swatches import too.
- **Agents.** Every action is a command (`nori-cli`, `nori-mcp`, the Agent panel); the Agent
  panel runs lsuite AI (sign in, no setup), Claude Code, Codex, the Anthropic or OpenAI API, or
  a model on Ollama. One undo history for everyone.
- **Plugins.** Filters written in Rust with the nori SDK load without a restart; ask the agent
  to write one. Two examples ship: Duotone and Halftone.
- **lsuite.** Send a picture straight to kimchi's timeline; nori appears to the other lsuite
  apps in `~/.lsuite/apps`.

### Release completion

- Independent document tabs, protected unsaved closes and quits, identity-checked background edits, and save completion that only marks the saved revision clean.
- Embedded smart objects with editable sources, replacement, lossless repeated resizing and explicit rasterizing.
- Interactive RGB/channel curves and brush pressure size/flow controls, with native macOS tablet event input.
- Layered PSD and IDML export, selectable PDF text, PDF/PDF-compatible AI import, bounded 8-bit GIMP XCF import.
- Signed updater archives with verification, install progress, rollback and restart; notarized Mac release packaging.

See README for beta format and tablet backend limitations.
