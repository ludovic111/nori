# nori

lsuite's editor for pictures, drawings and pages: Photoshop, Illustrator and InDesign's jobs in
one app ("that's the philosophy of lsuite"). A native Rust app: the window is **GPUI** (pinned to
Zed commit `7733b99…` exactly as kimchi pins it, `runtime_shaders` on macOS), and every action goes
through one command registry. See README.md.

```
crates/nori-core     the document: pages and master pages, layers (raster, fill, adjustment, text,
                     vector, group), blend modes, masks, clipping, selection, styles; pixels in
                     shared 256-px tiles (raster.rs); the one undo history (history.rs: snapshots
                     that share tiles, batches, coalescing, checkpoints); vector.rs (shapes, Bézier
                     paths, paints); the .nori file (file.rs, docs/FILE_FORMAT.md)
crates/nori-render   compositing tile by tile in parallel (composite.rs, W3C blend modes in
                     blend.rs), adjustments (adjust.rs), filters (filters.rs, the stock list),
                     the brush engine (brush.rs), fill/wand/gradient (fill.rs), resampling and
                     transforms (transform.rs), text (text.rs: cosmic-text, frames and threads),
                     vector drawing and path operations (shapes.rs: tiny-skia, i_overlay)
crates/nori-io       open/export: pictures (image crate), PSD layers (psd crate), OpenRaster,
                     SVG in (usvg) and out, PDF out (krilla), .gbr brushes and .ase swatches,
                     the editors people come from (APPS) and formats (FORMATS)
crates/nori-control  registry (commands/mod.rs lists every spec), session, permissions, bridge,
                     discovery, lsuite account (account.rs), plugin host (plugins.rs), settings,
                     the agent harness (harness/: brief.md, skills/*.md, context.rs, checks.rs),
                     updates through lsuite (update.rs)
crates/nori-agent    the built-in agent (lsuite AI, Claude Code, Codex, Anthropic, OpenAI, Ollama,
                     OpenAI-compatible), ported from kimchi-agent
crates/nori-desktop  the window (package/binary `nori`): store.rs, app.rs, views/, ui/, theme.rs
crates/nori-cli      `nori-cli` (and `nori-cli agent`: the built-in agent on a file);  crates/nori-mcp: `nori-mcp`
crates/nori-plugin   the plugin SDK: frozen repr(C) ABI (ffi.rs), GUIDE.md (what plugin.guide returns)
plugins/             two example SDK plugins: duotone, halftone
evals/               the agent evals (nori-evals): jobs/*.json, runner, RESULTS.md
scripts/publish-build.sh  a kimchi suite-build run → nori-v<version> in ludovic111/lsuite-builds
```

Rules that keep it working:

- **A feature is a command first.** Spec in `nori-control/src/commands/mod.rs`, handler in its
  family file, then the window calls it with `Store::run`. The window never changes the document
  itself (the canvas's live brush stroke only draws; the `raster.stroke` command it sends gives the
  very same pixels — `brush::Stroke` is deterministic, keep it so). Regenerate `docs/COMMANDS.md`
  with `cargo run -p nori-cli -- docs` (a test fails otherwise).
- **Undo is snapshots of the whole document.** That is cheap only because rasters and masks are
  tiles behind `Arc`s: never copy a whole raster where a tile write would do (`Plane::tile_mut`,
  `write_rect`), and drop tiles that became empty (`compact`). `history::same` decides whether a
  change made a step.
- **Units are document pixels on the page**, x right and y down, from the page's top-left corner.
  Vectors and text are `f32` and stay sharp; `dpi` only matters for print sizes and PDF points.
- **Heavy work leaves the lock**: commands take `session.snapshot()`, compute on a blocking
  thread, then `session.edit_at(revision, …)` (refused if the document moved meanwhile).
- **The canvas draws from tiles** (`views/canvas.rs`): level-0 tiles composited in the
  background (rayon), a pyramid of half-size tiles for zoomed-out views, only the tiles a change
  touched redrawn (`canvas::dirty` diffs two documents: tile pointers for pixels, boxes for
  settings). Images go to GPUI as straight BGRA; `drop_image` the ones replaced.
- Look: design system v2 like kimchi (black and white, square corners, grain behind the chrome,
  solid work surfaces; `theme.rs` with its contrast test, `ui/grain.rs`, Chakra Petch + IBM Plex
  Mono). Every area is titled (Tools, the page's name, Layers/Pages, Properties, History, Agent);
  tools are boxed by kind (select · paint · draw · type · view); the title bar groups history ·
  views · agent · app with Export as the primary action. Logos of other apps keep their colours
  (`ui/logos.rs`, `assets/logos/SOURCES.md`); nori's mark is one ink (`scripts/gen-mark.py` →
  `brand/`, `assets/icons/mark.svg`; `scripts/make-icons.sh` → `resources/`).
- Logs with `tracing`, never `println!` (stdout is MCP's protocol in `nori-mcp`).
- Testing the app on Linux: `NORI_DATA_DIR`, `NORI_CONFIG_DIR`, `LSUITE_HOME` to scratch folders,
  `NORI_NO_UPDATE=1`, `NORI_NO_SETUP=1` (skips the first-run setup), `NORI_WINDOW_SIZE=1600x1000`;
  `vscreen start target/debug/nori` (`VSCREEN=nori-night`), drive it with `target/debug/nori-cli`,
  `vscreen shot`. `NORI_NO_SYSTEM_FONTS=1` keeps tests to the bundled fonts.

## Names (for a rename)

The app's name lives in: `nori_core::APP`, the crate names (`nori-*`), the binaries (`nori`,
`nori-cli`, `nori-mcp`), the file extension (`nori_core::file::EXTENSION`, `.nori`), the
environment variables (`NORI_*`), the bundle id (`xyz.lsuite.nori` in `resources/Info.plist`,
`scripts/bundle-macos.sh`, `secrets.rs`'s keychain service), the discovery file (`discovery.rs`),
the plugin entry symbol (`nori_plugin_entry`) and folder (`~/.lsuite/plugins/nori`), and the
strings in the window. `grep -rn nori` finds them all.

## Decisions (2026-10-07, the first night)

- One document open at a time (tabs later). Opening a picture makes an unsaved document; Save
  writes a `.nori` and never overwrites the picture; Export writes pictures. `nori-cli --file
  photo.jpg` saves to `photo.nori` beside it for the same reason.
- Pages and artboards are the same thing (`Page` with a pasteboard position). Master pages are
  pages in `Document::masters`; editing one makes it the active page.
- Screen documents start with a pixel "Background" layer (as photo editors do); print presets
  and multi-page documents start with a "Paper" fill layer per page (no pixels to keep).
- Text: point text or frames; a thread's words live in its first frame; paragraph styles copy
  their values into the layers that use them (changing the style re-sets them); character styles
  apply through runs. PDF and SVG keep text as outlines (vectors, sharp, not yet selectable).
- PDF and SVG flatten what they can't express (adjustment layers, masks, clipping, blend modes
  PDF lacks) together with everything under it, and keep the rest as vectors (`nori-io/plan.rs`).
- Plugins are filters for now (RGBA f32 premultiplied tiles, typed parameters, a margin for
  neighbours). The template's `Cargo.toml` points at a copy of the SDK nori writes into
  `~/.lsuite/plugins-src/nori/.sdk/` (works offline and before the repository has a tag; the git
  line is in a comment). A library is copied before loading, so rebuilds reload without a restart;
  old copies stay mapped.
- lsuite AI is the default agent provider (AI.md); signing in is the person's own action
  (`account.signIn` is person-only).
- Auto-update (0.2, lsuite's DISTRIBUTION.md): `app.checkUpdates` reads
  `<server>/api/apps/nori/latest.json` with the lsuite account's token; signed out, the status's
  `signIn` says "Sign in to lsuite (in the lsuite app) to get updates". `NORI_UPDATE_URL` keeps
  working without an account (tests). Signatures unchanged.

## lsuite

nori is part of **lsuite** with ryolune (music), kimchi (video) and zenith (code); its page is
lsuite.xyz/nori. Contract: `../lsuite/STANDARD.md`, `PLUGINS.md`, `AI.md`, `design/DESIGN.md`.

- [x] Command registry: `family.verb` (doc, page, layer, raster, brushes, vector, text, select,
      filter, color, history, export, handoff, account, plugin, app, agent, ui), one undo history,
      `doc.batch`, `doc.overview`.
- [x] CLI `nori-cli` (running app, `--file`, `--headless`, `convert`), MCP `nori-mcp --live | --file`,
      docs/AI_CONTROL.md, generated docs/COMMANDS.md.
- [x] Built-in agent with lsuite AI first; permissions in `settings.agent.permissions`.
- [x] Discovery: `~/.lsuite/apps/nori.json` (kind `image`); hand-off `handoff.toKimchi`.
- [x] Plugins per PLUGINS.md (stock, installed, formats, build with your agent, `plugin.*`).
- [x] Design system v2, one-ink mark and icon.
- [x] Signed auto-update through lsuite (0.2): `<server>/api/apps/nori/latest.json` with the
      account's token, the file route with the token (never to another host), "Sign in to lsuite"
      when signed out; tests against a fake server in `update.rs`.
- [x] Builds: kimchi's `suite-build.yml` (app=nori) builds and signs; `scripts/publish-build.sh
      <version> <run-id>` checks the signatures, writes `latest.json` and `SHA256SUMS` and creates
      `nori-v<version>` in `ludovic111/lsuite-builds` (`--dry-run` first).
- [x] Agent harness (HARNESS.md, 0.2): 1 expert brief (`harness/brief.md`, ~1,300 words, the
      Agent panel's system prompt and nori-mcp's instructions; a test keeps it 800–1,500 words);
      2 eleven skills (`harness/skills/*.md`, MCP prompts and `nori://skills/<name>`; a test
      checks every command they name exists); 3 live context before every model step
      (`harness.context`, `Part::Context` in the thread, appended to MCP tool results when the
      document changed); 4 `harness.look` / `harness.check` (contrast, bounds and bleed,
      resolution, overflow, print colours); 5 the finish routine in the brief and skills; 6 one
      checkpoint per turn (as before); 7 evals in `evals/` (12 jobs, `nori-evals`, RESULTS.md).
      Part 8 (the suite agent) lives in the lsuite app.
- [x] Evals for 0.2.0: all twelve with Claude Code on Opus, 12/12 (evals/RESULTS.md).
- [ ] Harness next: run all twelve evals on each release candidate; a vision-free fallback for
      local models that can't see (they get the checks' numbers only).
- [ ] Not done: tabs (several documents), smart objects, a curves graph editor (presets and
      `layer.setAdjustment` only), selectable text in PDF, PSD and IDML export, XCF and `.ai`
      import, pen pressure from tablets (the engine takes pressure; the window sends 1).

## Verified local beta (2026-10-07)

App names are always lowercase in UI and documentation. Apple silicon bundles were built on
macmini under `~/builds/lsuite-2026-10-07/` and smoke-tested through their bundled CLIs. These
are local ad-hoc-signed builds; public release, notarization and signed in-place updates remain
separate release work. Linux workspace tests and clippy passed (existing warnings remain).
