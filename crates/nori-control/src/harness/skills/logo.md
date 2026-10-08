---
name: logo
title: Draw a logo, icon or vector illustration
when: A logo, wordmark, icon, badge, pictogram or flat vector illustration that must stay sharp at any size.
---
# Draw a logo or vector illustration

Vectors only, few shapes, works in one colour and at 32 px.

## Steps

1. Canvas: `doc_new width=1024 height=1024 background=transparent` for a mark (or a wider page
   for a lockup with the name). Guides on the centre: `page_addGuide at=512` twice.
2. Build from primitives: `vector_addShape` (ellipse, rect with radius, polygon, star, line),
   whole-pixel coordinates, symmetric about the guides. Curves: `vector_addPath d="M… C…"` or
   nodes with handles.
3. Compound forms: `vector_combine op=union|subtract|intersect|exclude layerIds=[…]` (subtract
   keeps the bottom layer minus the others). Edit points with `vector_editNode`.
4. Strokes: one or two widths for the whole mark; `vector_update cap=round join=round` for a
   friendly line style. Convert to paths before combining (`vector_toPath`).
5. Colour: two or three flat colours; then check the one-colour version (`vector_update
   fill=#000000` on a duplicate group, look, delete it).
6. Wordmark: `text_add` with a chosen family and weight, letterSpacing tuned; align to the mark
   (`layer_align to=middle relativeTo=selection`); spacing between mark and name ≈ the height of
   the name's lowercase.
7. Export if asked: `export_file path=….svg` (vectors), plus PNGs (`scale=…`).

## Checks

- `doc_overview`: only vector and text layers (no raster), named (Mark, Wordmark…).
- `page_look width=64` and `width=32`: still recognisable, no hairlines that vanish.
- Balanced: optical centre, even spacing, nothing touching the page edge (≥ 8 % padding).
