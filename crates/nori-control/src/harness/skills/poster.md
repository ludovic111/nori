---
name: poster
title: Lay out a poster or flyer
when: A poster, flyer, announcement or one-page print piece with a headline, an image or graphic, and details.
---
# Lay out a poster

One idea, one focal point, a grid, three levels of type.

## Steps

1. Size: `doc_new preset=a3` (or a4, poster-a2, letter; 300 dpi) `margins=…` (≥ 5 % of the
   short side: A3 → 200–300 px). `page_update columns=6 gutter=60` gives a grid;
   `page_update bleed=35` if anything will touch the edge.
2. Background and image: a fill or full-bleed shape (`vector_addShape shape=rect x=-35 y=-35
   width=page+70 height=page+70`), or `layer_place` a picture the person gave you, scaled to cover.
3. Type styles first: `text_defineStyle kind=paragraph name=Headline font=… size=… weight=800
   lineHeight=1.0 letterSpacing=-…`, `Subhead` (size ÷ 2.5–3), `Details` (body, 40–60 px at
   300 dpi, lineHeight 1.35). Two families at most.
4. Headline: `text_add` with a frame inside the margins (frameWidth = column span), style
   Headline; the most important words biggest. Supporting line and details below, aligned to the
   same left margin or column edge (`layer_align relativeTo=margins`).
5. Accent: one colour for the one thing to remember (date, call to action).
6. Look at it small: `page_look width=300` — the headline and image must read at thumbnail size.
7. Export if asked: `export_file path=….pdf` for print, `path=….png scale=0.5` for a preview.

## Checks

- `harness_check`: no overflow, nothing off the page, every text layer's contrast ≥ 4.5 (3 for
  the headline), images ≥ 300 ppi (≥ 200 for big posters).
- Clear order of reading: headline → image → details; all edges aligned to the grid; at most
  three type sizes; margins equal on the sides.
- Everything the brief asked for (who, what, when, where) is on the page, spelled right.
