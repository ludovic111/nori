---
name: booklet
title: Set a booklet, magazine spread or multi-page document
when: Several pages with flowing text — a booklet, zine, report, menu, magazine article or spread.
---
# Set a booklet or magazine spread

Master pages, styles, threaded frames, a grid; nothing overflows.

## Steps

1. `doc_new preset=a5 pages=8 margins=…` (A5 at 300 dpi: margins 150–180 px; inner margin a
   little bigger than outer), `page_update columns=2 gutter=50` on each page for magazines
   (one column for a booklet), `bleed=35` if images run off the edge.
2. Styles: `text_defineStyle kind=paragraph` for Body (40–46 px at 300 dpi ≈ 10–11 pt,
   lineHeight 1.4), Heading (1.6–2× body, weight 700), Caption (≈ 0.8× body), Folio (page
   number, 30 px).
3. Master: `page_addMaster name=A-Master`, `page_select page=A-Master`, `text_add text="{page}"
   style=Folio` in a small frame at the bottom outer corner (and a running head if wanted); apply
   it: `page_update page=N master=A-Master` for the inside pages (not the cover).
4. Body text: on each page a frame inside the margins (`text_add frameWidth frameHeight
   style=Body`; the words go in the first frame only), then `text_thread from=<frame p2>
   to=<frame p3>` in order so the words flow. Headings as separate text layers above the frames.
5. Pictures: `layer_place` with width = column or spread width; captions under them.
6. Cover: page 1, a large title and an image; no folio.
7. Export if asked: `export_file path=….pdf` (all pages).

## Checks

- `doc_overview` problems: **no text overflows** (add a page and a frame, or tighten), no hidden
  or empty layers; every inside page has the master.
- `harness_look` page by page: consistent margins and baseline of body text; headings never at
  the bottom of a page alone; folios on the right pages.
- `harness_check`: nothing outside the page or margins, images ≥ 300 ppi.
