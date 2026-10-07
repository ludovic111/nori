---
name: export-print-web
title: Export for print and web
when: Prepare and write files — a print-ready PDF, web images, several sizes or formats, "send to the printer".
---
# Export for print and web

Preflight first, then the right format and size for each use.

## Steps

1. Preflight: `harness_check` on every page. Fix errors before exporting: overflowing text,
   text off the page, low contrast, pictures under 300 ppi at print size (replace them or print
   smaller).
2. Print PDF:
   - document at 300 dpi (`doc_setInfo dpi=300` only if the pixels are already there; it
     changes the print size, not the pixels);
   - `page_update bleed=35` (3 mm at 300 dpi) on every page whose background or pictures touch
     the edge, and extend those layers 35 px past the trim;
   - text inside the margins (≥ 3–5 mm from the trim);
   - `export_file path=….pdf` (every page; vectors and text stay vectors; the bleed is included
     and the trim is marked in the PDF). Mention that nori writes RGB: the printer converts to
     CMYK, and very bright greens, blues or violets will print duller.
3. Web images:
   - photos: `export_file path=….jpg quality=82–88`; flat graphics and text: PNG; WebP for both
     when the person's site takes it;
   - size: `scale` so the long side is what is needed (2× the display size for sharp screens,
     e.g. a 1200 px wide hero from a 2400 px page: scale=0.5 for 1× or 1 for 2×);
   - one file per page: `page=N`.
4. Several sizes: repeat `export_file` with different `scale`; name files by size
   (`hero-1200.jpg`, `hero-2400.jpg`).
5. Vectors for others: `export_file path=….svg page=N`.

## Checks

- Each file exists where the person asked and has the size asked (the result of `export_file`
  gives the path and pixel size).
- The print PDF has every page; `doc_overview` shows the bleed on the pages that need it.
- Report the files with their sizes and what they are for.
