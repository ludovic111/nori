---
name: write-plugin
title: Write a plugin
when: The person wants a filter, effect or adjustment nori doesn't have, written as a Rust plugin and installed without a restart.
---
# Write a plugin

Follow the plugin recipe; nothing ships until it builds and has been tried on a picture.

## Steps

1. `plugin_guide` (the SDK, the kinds, the manifest, an example) and `plugin_toolchain`. Without
   Rust, say so and how to install it (the person decides); stop there. If a plugins command is
   refused, the "plugins" agent permission is off: say how to turn it on (Settings › Agent ›
   Permissions).
2. `plugin_new name=<lowercase-dashes> kind=filter`.
3. Write `src/lib.rs` with `plugin_writeSource name=… path=src/lib.rs contents=…`: typed
   parameters with sensible defaults and ranges, a margin when the filter reads neighbours,
   premultiplied RGBA f32 in and out, no allocation per pixel, no panics.
4. `plugin_build name=…` until it is green; fix from the structured errors (`{file, line,
   message}`), one round at a time.
5. `plugin_publishLocal name=…`: it installs and loads without a restart.
6. Try it: `filter_list` shows `plugin:<id>`; on a pixel layer (or a duplicate of the photo)
   `filter_apply filter=plugin:<id> params={…}`; `harness_look`; adjust parameters or code.

## Checks

- `plugin_list`: the plugin is installed and enabled.
- The filter visibly does what was asked on a real picture (`harness_look`, before and after
  with `history_undo` / `history_redo`), with no seams at tile edges (`page_look region` across
  a 256-px boundary).
