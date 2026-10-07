# The .nori file

A `.nori` document is a zip archive. Any zip tool opens it; every picture inside is a PNG.

```text
mimetype            application/x-nori (stored, the first entry, like OpenRaster's)
document.json       the document: pages, layers, styles, settings
layers/<id>.png     each pixel layer's pixels (RGBA, its own size; placed at its x, y)
masks/<id>.png      each layer mask (grey, the size of the layer's page)
selection.png       the selection (grey, the size of the active page), when there is one
luts/<id>.cube      each Color Lookup adjustment's table, as a .cube file
preview.png         the active page, at most 512 px, for file browsers and the home screen
```

nori writes a temporary file and renames it over the old one, so a crash never leaves half a
file. Pixels are stored at full precision (8 bits a channel, straight alpha); vectors and text
are stored as their descriptions, never as pixels.

## Units

Everything is in **document pixels**: x to the right, y down, from each page's top-left
corner. `dpi` says how big a pixel is in print (72 for screen work, 300 for print); PDF export
turns pixels into points with it (`points = pixels × 72 / dpi`). Vectors and text use the same
units as numbers with decimals, so they stay sharp at any output size.

## document.json

```json
{
  "format": 1,
  "name": "Poster",
  "dpi": 300,
  "pages": [ { "id": "P1", "name": "Page 1", "width": 4961, "height": 7016, "layers": [ … ] } ],
  "masters": [ … ],
  "activePage": "P1",
  "active": "L4",
  "styles": { "paragraph": [ … ], "character": [ … ] },
  "units": "px",
  "nextId": 9
}
```

| Field | |
| --- | --- |
| `format` | 1. A file with a higher number was written by a newer nori and isn't opened. |
| `pages` | Pages (in a layout) or artboards (in a design), in order. |
| `masters` | Master pages: their layers are drawn under every page that names them in `master`. |
| `activePage`, `active` | The page and layer tools act on. |
| `selection` | `{width, height, fill}` when there is a selection (its pixels are `selection.png`). |
| `styles` | Paragraph styles (how a whole text layer is set) and character styles (what runs change). |
| `nextId` | The next number for a layer (`L<n>`) or page (`P<n>`) id. |

### Page

`id`, `name`, `width`, `height` (pixels), `x`, `y` (where it sits on the pasteboard), `layers`
(top first), `master` (a master page id), `margins` (`{top, right, bottom, left}`), `columns`,
`gutter`, `guides` (`[{vertical, at}]`), `bleed`.

### Layer

Every layer has `id`, `name`, `visible`, `locked`, `opacity` (0–1), `blend` (`normal`, `multiply`,
`screen`, `overlay`, `darken`, `lighten`, `color-dodge`, `color-burn`, `linear-burn`,
`linear-dodge`, `hard-light`, `soft-light`, `vivid-light`, `linear-light`, `pin-light`,
`difference`, `exclusion`, `subtract`, `divide`, `hue`, `saturation`, `color`, `luminosity`, and
`pass-through` for groups), `clipped` (shown only where the layer under it has pixels), an optional
`mask` (`{mask: {width, height, fill}, enabled}`, pixels in `masks/<id>.png`) and a `kind`:

| `kind` | Fields |
| --- | --- |
| `raster` | `x`, `y` (top-left on the page), `pixels: {width, height}` (pixels in `layers/<id>.png`) |
| `fill` | `color` (`#rrggbb` or `#rrggbbaa`): one colour over the whole page |
| `adjustment` | `adjustment`: `{type, …settings}` — `levels {inputBlack, inputWhite, gamma, outputBlack, outputWhite}` (0–255), `curves {rgb, red, green, blue}` (points `[[in, out], …]`, 0–255), `hueSaturation {hue, saturation, lightness, colorize}`, `exposure {exposure, offset, gamma}`, `brightnessContrast {brightness, contrast}`, `vibrance {vibrance, saturation}`, `invert`, `blackWhite {red, green, blue}`, `threshold {level}`, `posterize {levels}`, `lut {name, size, amount}` (table in `luts/<id>.cube`) |
| `text` | `text`: `{text, font, size, weight, italic, color, x, y, align, lineHeight, letterSpacing, paragraphSpacing, frame?: [w, h], next?: layer id, style?: paragraph style, runs?: [{start, end, style?, font?, size?, weight?, italic?, color?}]}`. A text with a `frame` wraps inside it; what doesn't fit flows on to the `next` frame. `{page}` and `{pages}` are replaced by the page number and count. |
| `vector` | `shape`: `{geometry, fill, stroke?, fillRule, transform?}`. `geometry` is `{type: rect, x, y, w, h, radius}`, `ellipse {cx, cy, rx, ry}`, `polygon {cx, cy, radius, sides, inner?, rotation}`, `line {x1, y1, x2, y2}` or `path {subpaths: [{nodes: [{x, y, in?: [x, y], out?: [x, y]}], closed}]}`. `fill` and `stroke.paint` are `{type: none}`, `{type: solid, color}`, `{type: linear, x1, y1, x2, y2, stops: [{offset, color}]}` or `{type: radial, cx, cy, r, stops}`. `stroke`: `{paint, width, cap, join, miterLimit, dash}`. `transform`: `[a, b, c, d, e, f]` (x' = a·x + c·y + e). |
| `group` | `children` (top first), `expanded` |

## Compatibility

Readers ignore fields they don't know. New fields get defaults, so files from older nori open in
newer ones; `format` changes only when an older nori would misread a newer file.

Smart objects keep a `smart_source` base64 native document alongside their cached raster content. The source is shared between undo snapshots and survives save/open; the cache can be replaced at any size without overwriting the original. External formats export the rendered cache where an embedded native source cannot be represented.
