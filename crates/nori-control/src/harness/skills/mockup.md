---
name: mockup
title: Make a mockup
when: Show a design in context — on a phone or laptop screen, a poster on a wall, a card, a package, a framed print.
---
# Make a mockup

The design placed exactly into a device or object, with believable light.

## Steps

1. Scene: `doc_new width=2400 height=1600` (or the size asked) with a calm background (a fill, a
   soft radial gradient shape, or a placed photo the person gave).
2. Object: draw the device or object with vectors — a phone is a rect with radius 9 % of its
   width, a darker bezel rect, a screen rect inset by 3–4 %; a poster is a rect with a 1 px
   lighter edge; name the layers (Device, Screen).
3. Design: `layer_place path=<the design>` (or `layer_duplicate` / group from this document),
   `layer_transform scale=…` and `layer_move` to cover the screen rect exactly, then
   `layer_reorder above=Screen` and `layer_update clipped=true` so it shows only inside the screen.
   For tilted objects rotate the design and the object together (`layer_group`, then
   `layer_transform rotate=…`).
4. Light: a soft shadow under the object (ellipse, black, multiply, 25 %, gaussianBlur 20–40);
   a subtle diagonal highlight over the screen (white → transparent gradient shape, screen blend,
   10–15 %).
5. Export if asked: PNG or JPEG at the size asked.

## Checks

- `page_look region=[…]` on the screen edges: the design fills the screen, no gap, nothing
  spilling over the bezel (clipping on).
- The design keeps its proportions (no squash): compare its width/height ratio before and after.
- `harness_check`: the design's resolution is enough at the mockup size.
