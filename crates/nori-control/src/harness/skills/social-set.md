---
name: social-set
title: Make a set of social graphics
when: The same message in several social formats (square post, portrait post, story, banner, thumbnail) or a carousel.
---
# Make a set of social graphics

One design system, adapted per format; big type; safe zones.

## Steps

1. One page per format, side by side: `doc_new width=1080 height=1080 name=…`, then
   `page_add width=1080 height=1350 name=Portrait`, `page_add width=1080 height=1920
   name=Story`, `page_add width=1500 height=500 name=Banner`… Name pages by format.
   `page_update x=… ` places them apart on the pasteboard.
2. Styles shared by all: `text_defineStyle` Headline (80–120 px, weight 800), Body (≥ 36 px),
   Small (≥ 28 px: nothing smaller on a 1080-wide graphic).
3. Design the square first: background, headline, visual, logo or handle, call to action.
4. Adapt each other page (`page_select`, then add): same colours, styles and order, re-composed
   for the shape — not squashed. Story: keep text out of the top 250 px and bottom 300 px (app
   chrome); banner: text in the central safe area.
5. Carousels: same grid on every page, a page number or arrow, the hook on page 1.
6. Export each page if asked: `export_file path=…/square.png page=1`, `page=2`… (PNG for flat
   graphics, JPEG quality 85–90 for photos).

## Checks

- `harness_look page=…` on every page; `harness_check`: contrast ≥ 4.5, nothing outside a page.
- Same headline text and colours on every format; nothing cramped against the edges (margins
  ≥ 60 px); text at least 28 px.
- One file per format when exports were asked, with the sizes asked.
