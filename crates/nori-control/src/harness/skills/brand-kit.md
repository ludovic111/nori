---
name: brand-kit
title: Build a brand kit or style sheet
when: A brand board, style guide, colour palette, type specimen, or a set of on-brand templates.
---
# Build a brand kit

Logo, palette, type and examples on one clear board.

## Steps

1. `doc_new preset=a4-landscape margins=180` (or 1920×1080 for screen), `page_update columns=12
   gutter=40`.
2. Palette: 1 primary, 1–2 secondary, 1 accent, 2 neutrals. For each, `vector_addShape
   shape=rect` swatch (same size, on the grid, `layer_align to=distributeHorizontal`), and under
   it a caption `text_add` with the name and hex (`#1f6feb`). Tints: lighten the primary in 2–3
   steps rather than adding hues.
3. Type: specimen lines for each style — `text_defineStyle` H1, H2, Body, Caption and a text
   layer set in each showing its name, family, size and weight ("H1 — Manrope 800, 96 px").
4. Logo: place or draw it (see the logo skill), on light and dark backgrounds, with clear space
   shown by a dashed rectangle (`vector_update dash=[12,8]`).
5. Examples: a small card or social tile using only the kit's colours and styles.
6. Headings for each section (Colour, Type, Logo, In use) in one style, aligned to the grid.
7. Export if asked: PDF for the board, PNG for sharing.

## Checks

- `harness_check`: every caption readable on its background (contrast ≥ 4.5), nothing off the
  page.
- Swatches equal in size and evenly spaced; hex captions match the swatches' fills
  (`layer_get`); styles listed exist in `text_styles`.
