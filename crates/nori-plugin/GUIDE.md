# Writing a nori plugin

A nori plugin is a small Rust library that nori loads while it runs. Today a plugin is a
**filter**: pixels in, pixels out, with parameters the person sets in a dialog (or an agent
sets in `filter.apply`). It shows in the Filter menu under its group, in the Plugins area, and
to agents as `plugin:<id>` in `filter.list`.

## The recipe

1. `plugin.guide` (this text) and `plugin.toolchain` (is Rust installed? If not, offer the
   install: rustup, with the person's consent).
2. `plugin.new {name, kind: "filter"}` makes a crate in `~/.lsuite/plugins-src/nori/<name>/`.
3. Write `src/lib.rs` (with `plugin.writeSource {name, path: "src/lib.rs", contents}`, or your
   own file tools inside that folder).
4. `plugin.build {name}` until it is green; errors come back as `{file, line, message}`.
5. `plugin.publishLocal {name}`: the bundle is installed and loaded, no restart. Then try it:
   `filter.apply {filter: "plugin:<id>", params: {…}}` on a layer, and look with
   `page.look`.

## The crate

`Cargo.toml` (made by `plugin.new`):

```toml
[package]
name = "duotone"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
nori-plugin = { path = "…" }   # the SDK nori ships; or { git = "https://github.com/ludovic111/nori", tag = "v0.1.0" }
```

`src/lib.rs`:

```rust
use nori_plugin::prelude::*;

pub struct Duotone;

impl Filter for Duotone {
    const INFO: Info = Info::filter(
        "com.example.duotone",          // reverse-DNS id, unique
        "Duotone",                      // name in the Filter menu
        "Example",                      // vendor
        "color",                        // group: blur, sharpen, noise, stylize, color, distort, other
        "Maps dark tones to one colour and light tones to another.",
    );

    fn params() -> Vec<Param> {
        vec![
            Param::number("mix", "Mix", 0.0, 100.0, 100.0, "%"),
            Param::choice("palette", "Palette", &["ink", "sunset", "sea"], 0),
        ]
    }

    fn new() -> Self {
        Duotone
    }

    fn process(&mut self, tile: &mut Tile, values: &[f64]) {
        let mix = (values[0] / 100.0) as f32;
        let (dark, light) = match values[1] as usize {
            1 => (hex("#2b1055"), hex("#ff9a62")),
            2 => (hex("#00223e"), hex("#7fffd4")),
            _ => (hex("#111111"), hex("#f2efe6")),
        };
        for p in tile.pixels.iter_mut() {
            let (c, a) = unpremultiply(*p);
            if a <= 0.0 { continue; }
            let l = luma(c);
            let d = [0, 1, 2].map(|i| dark[i] + (light[i] - dark[i]) * l);
            let out = [0, 1, 2].map(|i| c[i] + (d[i] - c[i]) * mix);
            *p = premultiply(out, a);
        }
    }
}

export_plugins!(Duotone);
```

## Rules

- Pixels are **premultiplied** RGBA `f32` in 0..1, rows of `tile.width`. Use `unpremultiply`
  and `premultiply` around colour maths; what you return is clamped to valid colour.
- `tile.x`, `tile.y` are where the tile is on the page: use them for patterns and noise, so
  tiles line up.
- A filter that reads neighbours (blurs, edges) returns how far from `margin(&self, values)`;
  the host gives it that many extra pixels around the area it changes (and only keeps the
  inside). Keep it under a few hundred pixels.
- `values` come in the order of `params()`, already clamped to each range; choices are the
  option's index, switches 0 or 1.
- No threads, no files, no network, no printing: `process` must only compute. It runs off
  the interface thread; nori calls it as often as the picture needs.
- A panic doesn't crash nori: the plugin is switched off and the picture is left as it was.
  Fix the cause and publish again.
- Several filters can live in one library: `export_plugins!(A, B)`. The exported symbol is
  `nori_plugin_entry` (ABI 1, checked before anything is called).

## The bundle

`plugin.publishLocal` builds and installs a folder `~/.lsuite/plugins/nori/<id>/` with
`plugin.toml` and the library:

```toml
id = "com.example.duotone"
name = "Duotone"
version = "0.1.0"
app = "nori"
kind = "filter"
abi = 1
description = "Maps dark tones to one colour and light tones to another."
authors = ["Example"]

[library]
macos = "libduotone.dylib"
linux = "libduotone.so"
windows = "duotone.dll"
```

`plugin.install {path}` installs a bundle someone gave you; `plugin.remove {id}` removes it;
`plugin.disable` / `plugin.enable` switch it.
