# Changelog

## Unreleased

lsuite is fully free: no account, no paid plan.

- **No lsuite account.** Updates come from lsuite.xyz without signing in
  (`<server>/api/apps/nori/latest.json`, no `Authorization` header; `LSUITE_SERVER` replaces
  `LSUITE_ACCOUNT_SERVER`). Settings › Updates no longer asks to sign in. An old
  `~/.lsuite/account.json` is ignored and left alone.
- **lsuite AI is gone.** The agent runs on what you already have: Claude Code, Codex, an
  Anthropic or OpenAI key, Ollama or an OpenAI-compatible server. Claude Code is the default;
  settings that chose lsuite AI switch to it, and conversations saved with it still open.
- **Removed:** the `account.*` commands (`account.status`, `account.signIn`, `account.signOut`,
  `account.plans`), Settings › lsuite AI, the sign-in buttons in the Agent panel and the
  first-run setup, the plan and allowance lines, and `nori-cli doctor`'s account check.

## 0.2.0 — 2026-10-07

The agent harness: an agent working in nori now knows the trade, sees and measures what it
made, and checks it before it says it's done — the built-in agent, Claude Code, Codex or any
MCP client alike. And updates now come through lsuite.

nori is in beta for **Linux** (AppImage and .deb); macOS and Windows are coming soon.

- **An expert brief.** The agent's instructions are a designer's brief: retouching,
  compositing, vector illustration, type and grids, layout and print (bleed, margins, RGB and
  CMYK), colour, hierarchy and contrast, accessibility; the commands for the common jobs, the
  usual mistakes and a finish routine (look, compare with the request, fix up to three times,
  report). One source for the Agent panel and `nori-mcp`'s instructions (`harness.brief`).
- **Skills.** Eleven playbooks with the exact commands and the checks that prove the job
  worked: retouch a photo, cut out and composite, poster, social graphics set, logo, booklet,
  brand kit, mockup, export for print and web, batch edits, write a plugin
  (`harness.skills`, `harness.skill`; MCP prompts and `nori://skills/<name>` resources).
- **Live context before every step.** The page and its grid, its layers, the selection,
  problems, and what you changed in the window while the agent worked, refreshed before each
  model step (`harness.context`; with MCP tool results too).
- **Eyes and numbers.** `harness.look` shows the page to the model with objective checks
  (`harness.check`): the WCAG contrast of every text layer against what is behind it, things
  outside the page, past its edge or margins or short of the bleed, pictures' real resolution at
  their printed size, overflowing text, empty layers, colours CMYK can't print.
- **PDF bleed.** A page with a bleed exports on a sheet that much bigger, with its trim and
  bleed boxes marked for the printer.
- **`nori-cli agent`.** Run the agent on a file without the app, with any provider
  (`nori-cli --file poster.nori agent "…" --provider claude-code`).
- **Evals.** Twelve design jobs run headless with a real model and scored automatically on the
  resulting document (`evals/`, results in `evals/RESULTS.md`).
- **Notes reach every MCP client.** The live context, the file saved and a finish-routine
  reminder after each change go into a tool result's `structuredContent.harnessNotes` as well as
  after its text: Claude Code shows only the structured content when there is some.
- **`doc.batch` takes tool names.** A batch's commands can be written `text.update` or
  `text_update`, the name agents know them by.
- **Updates through lsuite.** nori's updates come from lsuite.xyz with your free lsuite account
  (signed in once in the lsuite app); signed out, Settings › Updates says so. Signatures are
  checked exactly as before.

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
