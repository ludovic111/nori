# nori commands

Generated from the command registry (`crates/nori-control`) by `nori-cli docs`. Do not edit by hand.

Every command works the same from the window, the built-in agent, `nori-cli` and `nori-mcp` (where `family.verb` becomes the tool `family_verb`). Positions and sizes are document pixels on the page (x right, y down from the top-left corner); layer ids and unique names are accepted wherever a layer is expected, and page ids, names or numbers from 1 wherever a page is. Commands that edit the document also accept `coalesce` (consecutive edits from the same source and key fold into one undo step within about a second; a unique `gesture:` key keeps a gesture together across pauses). See [AI_CONTROL.md](AI_CONTROL.md).

## doc

### `doc.overview`

The open document in one bounded answer: pages (size, margins, columns, master), every layer on each page as a tree (kind, name, id, bounds, opacity, blend, visibility, text and shape summaries, adjustment settings), the active page and layer, the selection, styles, colours, undo history, whether it is saved, and problems (text that overflows its frames, hidden or empty layers). Read it first. _(read only)_

### `doc.get`

The complete document as JSON (the document.json of the .nori format; pixels are only sizes). _(read only)_

### `doc.new`

Make a new document (replacing the open one): a picture, a poster, a booklet. Give a size, or a preset: screen (1920×1080), square (2048), a4, a5, letter, poster-a2, story (1080×1920), instagram (1080×1350), card (1050×600). Print presets are 300 dpi. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `name` | string |  | Name (default "Untitled"). |
| `width` | integer |  | Width in pixels (default 1920). |
| `height` | integer |  | Height in pixels (default 1080). |
| `preset` | string |  | A size preset (overrides width and height). |
| `dpi` | number |  | Pixels per inch (72 screen, 300 print). |
| `background` | string |  | Background colour #rrggbb, or "transparent" (default white). |
| `pages` | integer |  | How many pages (default 1). |
| `margins` | number |  | Page margins in pixels, every side (default none). |
| `columns` | integer |  | Columns inside the margins (default 1). |

### `doc.open`

Open a file as the document, replacing the open one: .nori, PNG, JPEG, WebP, TIFF, BMP, GIF, Photoshop .psd (with its layers), OpenRaster .ora (Krita, GIMP) or SVG (as vector layers). _(changes things · permission: files)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `path` | string | required | The file. |

### `doc.save`

Save the document as a .nori file (where it was saved before, or path). _(changes things · permission: files)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `path` | string |  | A .nori file to save to (and save to from now on). |

### `doc.close`

Close the document (unsaved changes are lost unless saved first). _(changes things)_

### `doc.setInfo`

Change the document's name, resolution (dpi: print sizes and PDF points come from it; no pixel changes) or the units the window shows. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `name` | string |  | Name. |
| `dpi` | number |  | Pixels per inch. |
| `units` | string |  | px, pt, mm or in. |

### `doc.resize`

Image size: scale every page and everything on it (pixels resampled; vectors and text scaled, staying sharp). Give a width or height (the other follows), both, or a scale. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `width` | integer |  | New width of the active page in pixels. |
| `height` | integer |  | New height in pixels. |
| `scale` | number |  | Multiplier, e.g. 0.5 or 2. |
| `filter` | string |  | Resampling: bicubic (default), bilinear, lanczos or nearest. |

### `doc.resizeCanvas`

Canvas size: change a page's size without scaling what is on it (layers keep their place relative to the anchor). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `width` | integer | required | New width in pixels. |
| `height` | integer | required | New height in pixels. |
| `anchor` | string |  | Where the old picture stays: center (default), top-left, top, top-right, left, right, bottom-left, bottom, bottom-right. |
| `page` | string |  | Page id, name or number (from 1). Defaults to the active page. |

### `doc.crop`

Crop the active page to a rectangle (or the selection's bounds): pixels outside are cut off, vectors and text move with the page. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `x` | integer |  | Left edge. |
| `y` | integer |  | Top edge. |
| `width` | integer |  | Width. |
| `height` | integer |  | Height. |

### `doc.rotate`

Turn or flip the whole active page and everything on it: quarter turns clockwise (1, 2, 3 or -1), or flip horizontal/vertical. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `quarters` | integer |  | Quarter turns clockwise. |
| `flip` | string |  | horizontal or vertical. |

### `doc.batch`

Run several commands as one undo step. With atomic (the default) a failing command rolls back the ones before it. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `commands` | array of objects | required | Array of {"command": "layer.update", "params": {…}}. |
| `atomic` | boolean |  | Roll everything back if one command fails (default true). |
| `label` | string |  | Name of the undo step (default "batch"). |

### `doc.recent`

Files opened or saved recently, newest first. _(read only)_

## page

### `page.list`

Pages (and master pages) with their size, margins, columns, master and layer count. _(read only)_

### `page.add`

Add pages (a booklet's next spread, another artboard) after a page or at the end; the new one becomes active. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `count` | integer |  | How many (default 1). |
| `after` | string |  | After this page (id, name or number). Defaults to the end. |
| `width` | integer |  | Width (default: the active page's). |
| `height` | integer |  | Height (default: the active page's). |
| `name` | string |  | Name (default "Page n"). |
| `master` | string |  | Master page (id or name) to apply. |
| `duplicate` | boolean |  | Copy the active page's layers onto it. |

### `page.remove`

Delete a page and its layers. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string | required | Page id, name or number. |

### `page.move`

Move a page to another position (1 is first). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string | required | Page id, name or number. |
| `to` | integer | required | New position from 1. |

### `page.select`

Make a page the active one (the window shows it; commands act on it). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string | required | Page id, name or number. |

### `page.update`

Change a page: name, size (without scaling its content), position on the pasteboard, margins, columns, gutter, bleed, master. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or number (from 1). Defaults to the active page. |
| `name` | string |  | Name. |
| `width` | integer |  | Width in pixels. |
| `height` | integer |  | Height in pixels. |
| `x` | number |  | Position on the pasteboard (artboards side by side). |
| `y` | number |  | Position on the pasteboard. |
| `margins` | any |  | A number for every side, or {top, right, bottom, left}. |
| `columns` | integer |  | Columns inside the margins. |
| `gutter` | number |  | Space between columns. |
| `bleed` | number |  | Bleed past the trim, for print PDFs. |
| `master` | string |  | Master page id or name; "" removes it. |
| `coalesce` | string |  | Consecutive edits from the same source with the same key within ~1 s fold into one undo step. Prefix a unique per-gesture key with gesture: to keep that gesture together across pauses. A different edit, undo or redo ends the group. |

### `page.addGuide`

Add a guide line to a page (vertical at x, or horizontal at y). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `at` | number | required | Position in pixels. |
| `vertical` | boolean |  | Vertical (default true). |
| `page` | string |  | Page id, name or number (from 1). Defaults to the active page. |

### `page.clearGuides`

Remove a page's guides. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or number (from 1). Defaults to the active page. |

### `page.addMaster`

Make a master page (what repeats on the pages it's applied to: page numbers, a frame, a logo), from scratch or from a page's layers. Edit it with page.select and the usual commands; apply it to pages with page.update master=<its id>. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `name` | string |  | Name (default "A-Master"). |
| `from` | string |  | Copy this page's layers and grid. |

### `page.look`

Draw a page (or a region of it) to a PNG and return its path, to look at the result. Agents that can see get the picture. _(read only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or number (from 1). Defaults to the active page. |
| `width` | integer |  | Width of the picture in pixels (default 1024, at most the page's). |
| `region` | array of numbers |  | [x, y, width, height] of the page to show. |

## layer

### `layer.list`

The active page's layers (or a page's) as a tree, top first: id, name, kind, visible, locked, opacity, blend, bounds, mask, clipped. _(read only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or number (from 1). Defaults to the active page. |

### `layer.get`

One layer in full: its settings and content (text, shape, adjustment, pixel bounds). _(read only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `layerId` | string | required | Layer id (L12) or unique name, as layer.list shows. |

### `layer.add`

Add a layer above the active one: an empty pixel layer (raster), a solid colour fill, or an empty group. Becomes active. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `kind` | string |  | raster (default), fill or group. |
| `name` | string |  | Layer name. Defaults to one like "Layer 3". |
| `color` | string |  | Colour #rrggbb or #rrggbbaa. Defaults to the foreground colour. |
| `above` | string |  | Put it above this layer (id or name). Defaults to above the active layer. |

### `layer.addAdjustment`

Add an adjustment layer: it changes the colours of everything under it (in its group), without touching their pixels. Kinds: levels {inputBlack, inputWhite, gamma, outputBlack, outputWhite} (0–255), curves {rgb, red, green, blue: [[in, out], …] 0–255}, hueSaturation {hue −180…180, saturation, lightness −100…100, colorize}, exposure {exposure stops, offset, gamma}, brightnessContrast {brightness −150…150, contrast −50…100}, vibrance {vibrance, saturation}, invert, blackWhite {red, green, blue %}, threshold {level}, posterize {levels}. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `kind` | string | required | The adjustment kind. |
| `settings` | object |  | Its settings (the rest stay neutral). |
| `name` | string |  | Layer name. Defaults to one like "Layer 3". |
| `above` | string |  | Put it above this layer (id or name). Defaults to above the active layer. |

### `layer.addLut`

Add a Color Lookup adjustment layer from a .cube LUT file (a film look, a grade from Resolve or Premiere). _(changes things · permission: files)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `path` | string | required | The .cube file. |
| `amount` | number |  | 0–1 (default 1). |
| `above` | string |  | Put it above this layer (id or name). Defaults to above the active layer. |

### `layer.place`

Place a file on the active page as a layer: a picture as pixels (centred, or at x, y), an SVG as a group of vector layers, a .nori/.psd/.ora as a group of its layers. _(changes things · permission: files)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `path` | string | required | The file. |
| `x` | integer |  | Left edge (pictures). |
| `y` | integer |  | Top edge (pictures). |
| `width` | integer |  | Scale a picture to this width (keeps its proportions unless height is given). |
| `height` | integer |  | Scale a picture to this height. |

### `layer.update`

Change a layer: name, visible, locked, opacity (0–1), blend mode, clipped to the layer below, a group's open state. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `layerId` | string |  | Layer id (L12) or unique name. Defaults to the active layer. |
| `name` | string |  | Name. |
| `visible` | boolean |  | Shown. |
| `locked` | boolean |  | Protected from edits. |
| `opacity` | number |  | 0–1. |
| `blend` | string |  | normal, multiply, screen, overlay, darken, lighten, color-dodge, color-burn, linear-burn, linear-dodge, hard-light, soft-light, vivid-light, linear-light, pin-light, difference, exclusion, subtract, divide, hue, saturation, color, luminosity, pass-through (groups). |
| `clipped` | boolean |  | Clipped to the layer under it (shows only where that layer has pixels). |
| `expanded` | boolean |  | A group shown open in the Layers panel. |
| `color` | string |  | A colour fill layer's colour, #rrggbb or #rrggbbaa. |
| `coalesce` | string |  | Consecutive edits from the same source with the same key within ~1 s fold into one undo step. Prefix a unique per-gesture key with gesture: to keep that gesture together across pauses. A different edit, undo or redo ends the group. |

### `layer.select`

Make a layer the active one (tools and commands act on it); its page becomes active too. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `layerId` | string | required | Layer id (L12) or unique name, as layer.list shows. |

### `layer.move`

Move a layer (its pixels, text, shape or a whole group) by dx, dy pixels, or to x, y (its top-left corner). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `layerId` | string |  | Layer id (L12) or unique name. Defaults to the active layer. |
| `dx` | number |  | Pixels right (negative: left). |
| `dy` | number |  | Pixels down (negative: up). |
| `x` | number |  | Move its left edge here. |
| `y` | number |  | Move its top edge here. |
| `coalesce` | string |  | Consecutive edits from the same source with the same key within ~1 s fold into one undo step. Prefix a unique per-gesture key with gesture: to keep that gesture together across pauses. A different edit, undo or redo ends the group. |

### `layer.align`

Line layers up with the page, the margins or each other: left, center, right, top, middle, bottom; or spread them evenly (distributeHorizontal, distributeVertical). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `layerIds` | array of strings | required | Layers (ids or names). |
| `to` | string | required | left, center, right, top, middle, bottom, distributeHorizontal or distributeVertical. |
| `relativeTo` | string |  | page (default), margins or selection (the layers' own bounds). |

### `layer.reorder`

Move a layer in the stack: to the top or bottom, up or down one, above or below another layer (into its group), or into a group. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `layerId` | string |  | Layer id (L12) or unique name. Defaults to the active layer. |
| `to` | string |  | top, bottom, up or down. |
| `above` | string |  | Put it above this layer. |
| `below` | string |  | Put it below this layer. |
| `into` | string |  | Put it at the top of this group. |

### `layer.duplicate`

Copy a layer (above it). The copy becomes active. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `layerId` | string |  | Layer id (L12) or unique name. Defaults to the active layer. |
| `name` | string |  | Layer name. Defaults to one like "Layer 3". |

### `layer.delete`

Delete layers. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `layerIds` | array of strings |  | Layers to delete (default: the active one). |

### `layer.group`

Put layers in a new group (in place of the topmost). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `layerIds` | array of strings | required | Layers to group (ids or names). |
| `name` | string |  | Layer name. Defaults to one like "Layer 3". |

### `layer.ungroup`

Take a group's layers out of it and remove the group. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `layerId` | string |  | Layer id (L12) or unique name. Defaults to the active layer. |

### `layer.merge`

Merge a layer down into the pixel layer under it (down), or every visible layer of the page into one (visible). The result is pixels. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `layerId` | string |  | Layer id (L12) or unique name. Defaults to the active layer. |
| `mode` | string |  | down (default) or visible. |

### `layer.flatten`

Flatten the active page into one pixel layer (what you see). _(changes things)_

### `layer.rasterize`

Turn a text, vector, fill or adjustment... layer into pixels (as it looks now). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `layerId` | string |  | Layer id (L12) or unique name. Defaults to the active layer. |

### `layer.setAdjustment`

Change an adjustment layer's settings (only the given ones). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `layerId` | string |  | Layer id (L12) or unique name. Defaults to the active layer. |
| `settings` | object | required | Settings to change, e.g. {"gamma": 1.2}. |
| `coalesce` | string |  | Consecutive edits from the same source with the same key within ~1 s fold into one undo step. Prefix a unique per-gesture key with gesture: to keep that gesture together across pauses. A different edit, undo or redo ends the group. |

### `layer.addMask`

Add a layer mask: reveal all (white), hide all (black), or from the selection. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `layerId` | string |  | Layer id (L12) or unique name. Defaults to the active layer. |
| `from` | string |  | reveal (default), hide or selection. |

### `layer.mask`

Change a layer's mask: enable or disable it, invert it, apply it (bake it into the pixels) or delete it. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `layerId` | string |  | Layer id (L12) or unique name. Defaults to the active layer. |
| `action` | string | required | enable, disable, invert, apply or delete. |

### `layer.transform`

Scale, rotate and flip a layer about its centre (or a point), and move it. Pixels are resampled once; text and vectors stay sharp. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `layerId` | string |  | Layer id (L12) or unique name. Defaults to the active layer. |
| `scale` | number |  | Uniform scale (1 = same). |
| `scaleX` | number |  | Horizontal scale. |
| `scaleY` | number |  | Vertical scale. |
| `rotate` | number |  | Degrees clockwise. |
| `flip` | string |  | horizontal or vertical. |
| `dx` | number |  | Then move right. |
| `dy` | number |  | Then move down. |
| `originX` | number |  | Turn and scale about this point (default: the layer's centre). |
| `originY` | number |  | Turn and scale about this point. |
| `filter` | string |  | Resampling for pixels: bicubic (default), bilinear, lanczos or nearest. |

### `layer.look`

Draw one layer on its own (or what an adjustment does) to a PNG and return its path. _(read only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `layerId` | string |  | Layer id (L12) or unique name. Defaults to the active layer. |
| `width` | integer |  | Width of the picture (default 768). |

## raster

### `raster.stroke`

Paint a brush (or eraser) stroke on a pixel layer through points [[x, y, pressure], …] (pressure 0–1, optional). Inside the selection only. The window's brush and eraser call this. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `points` | array of arrays | required | Points along the stroke, [[x, y] or [x, y, pressure], …]. |
| `layerId` | string |  | Layer id (L12) or unique name. Defaults to the active layer. |
| `color` | string |  | Colour #rrggbb or #rrggbbaa. Defaults to the foreground colour. |
| `size` | number |  | Brush diameter in pixels (default: the brush tool's). |
| `hardness` | number |  | 0 soft … 1 hard. |
| `opacity` | number |  | 0–1: the most paint the stroke lays down. |
| `flow` | number |  | 0–1: paint per dab. |
| `spacing` | number |  | Distance between dabs as a share of the size. |
| `erase` | boolean |  | Erase instead of painting. |
| `tip` | string |  | A brush tip by name (brushes.list); default round. |

### `raster.fill`

Paint bucket: fill the area like the pixel at (x, y) with a colour (within tolerance, touching it or everywhere), inside the selection. Without x and y: fill the whole selection. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `layerId` | string |  | Layer id (L12) or unique name. Defaults to the active layer. |
| `x` | integer |  | Where to click. |
| `y` | integer |  | Where to click. |
| `color` | string |  | Colour #rrggbb or #rrggbbaa. Defaults to the foreground colour. |
| `tolerance` | integer |  | 0–255 (default 32). |
| `contiguous` | boolean |  | Only touching pixels (default true). |
| `sampleAll` | boolean |  | Judge by the whole picture, not just the layer. |
| `opacity` | number |  | 0–1. |

### `raster.gradient`

Draw a gradient on a pixel layer from (x1, y1) to (x2, y2), from one colour to another (default foreground to background), inside the selection. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `x1` | number | required | Start. |
| `y1` | number | required | Start. |
| `x2` | number | required | End. |
| `y2` | number | required | End. |
| `layerId` | string |  | Layer id (L12) or unique name. Defaults to the active layer. |
| `from` | string |  | Start colour. |
| `to` | string |  | End colour ("transparent" fades out). |
| `kind` | string |  | linear (default), radial or reflected. |
| `opacity` | number |  | 0–1. |

### `raster.clear`

Erase the selected pixels of a layer (all of it without a selection). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `layerId` | string |  | Layer id (L12) or unique name. Defaults to the active layer. |

### `raster.pick`

The colour of the picture at a pixel (as the eyedropper sees it), or of one layer. _(read only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `x` | integer | required | x |
| `y` | integer | required | y |
| `layerId` | string |  | Only this layer. |
| `setForeground` | boolean |  | Also make it the foreground colour. |

## brushes

### `brushes.list`

Brush tips: round, and the ones imported from .gbr files. _(read only)_

### `brushes.import`

Import a GIMP brush (.gbr, also used by Krita and Photopea) as a brush tip. _(changes things · permission: files)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `path` | string | required | The .gbr file. |

## vector

### `vector.addShape`

Add a vector shape layer: rect (live corners: radius), ellipse, polygon (sides), star (sides, inner 0–1), line. Stays sharp at any size. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `shape` | string | required | rect, ellipse, polygon, star or line. |
| `x` | number | required | Left edge (line: start x). |
| `y` | number | required | Top edge (line: start y). |
| `width` | number | required | Width (line: end x − x). |
| `height` | number | required | Height (line: end y − y). |
| `radius` | number |  | rect: corner radius. |
| `sides` | integer |  | polygon or star: points (default 5 for stars, 6 for polygons). |
| `inner` | number |  | star: inner radius as a share of the outer (default 0.5). |
| `fill` | any |  | Fill: "#rrggbb", "none", or a paint {"type": "linear", "x1", "y1", "x2", "y2", "stops": [{"offset": 0, "color": "#000"}, …]} / {"type": "radial", "cx", "cy", "r", "stops"}. |
| `stroke` | any |  | Stroke colour "#rrggbb" or a paint, or "none". |
| `strokeWidth` | number |  | Stroke width in pixels. |
| `name` | string |  | Layer name. Defaults to one like "Layer 3". |
| `above` | string |  | Put it above this layer (id or name). Defaults to above the active layer. |

### `vector.addPath`

Add a vector path layer from points: subpaths of nodes {x, y, in: [x, y], out: [x, y]} (handles optional: corners), or an SVG path string d ("M10 10 C 20 0 …"). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `subpaths` | array of objects |  | [{"nodes": [{"x":…, "y":…, "in": [..], "out": [..]}], "closed": true}, …]. |
| `d` | string |  | An SVG path string instead. |
| `fill` | any |  | Fill: "#rrggbb", "none", or a paint {"type": "linear", "x1", "y1", "x2", "y2", "stops": [{"offset": 0, "color": "#000"}, …]} / {"type": "radial", "cx", "cy", "r", "stops"}. |
| `stroke` | any |  | Stroke colour "#rrggbb" or a paint, or "none". |
| `strokeWidth` | number |  | Stroke width in pixels. |
| `name` | string |  | Layer name. Defaults to one like "Layer 3". |
| `above` | string |  | Put it above this layer (id or name). Defaults to above the active layer. |

### `vector.update`

Change a vector layer's look: fill, stroke, stroke width, caps, joins, dashes, fill rule, corner radius (rects). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `layerId` | string |  | Layer id (L12) or unique name. Defaults to the active layer. |
| `fill` | any |  | Fill: "#rrggbb", "none", or a paint {"type": "linear", "x1", "y1", "x2", "y2", "stops": [{"offset": 0, "color": "#000"}, …]} / {"type": "radial", "cx", "cy", "r", "stops"}. |
| `stroke` | any |  | Stroke colour "#rrggbb" or a paint, or "none". |
| `strokeWidth` | number |  | Stroke width in pixels. |
| `cap` | string |  | butt, round or square. |
| `join` | string |  | miter, round or bevel. |
| `dash` | array of numbers |  | Dash and gap lengths, [] for solid. |
| `fillRule` | string |  | nonZero or evenOdd. |
| `radius` | number |  | rect: corner radius. |
| `coalesce` | string |  | Consecutive edits from the same source with the same key within ~1 s fold into one undo step. Prefix a unique per-gesture key with gesture: to keep that gesture together across pauses. A different edit, undo or redo ends the group. |

### `vector.setGeometry`

Replace a vector layer's geometry with JSON: {"type": "rect", x, y, w, h, radius} | ellipse {cx, cy, rx, ry} | polygon {cx, cy, radius, sides, inner?, rotation} | line {x1, y1, x2, y2} | path {subpaths}. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `layerId` | string |  | Layer id (L12) or unique name. Defaults to the active layer. |
| `geometry` | object | required | The geometry. |
| `coalesce` | string |  | Consecutive edits from the same source with the same key within ~1 s fold into one undo step. Prefix a unique per-gesture key with gesture: to keep that gesture together across pauses. A different edit, undo or redo ends the group. |

### `vector.editNode`

Move one node of a path (and its handles), as the direct-selection tool does. A shape becomes a path first. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `layerId` | string |  | Layer id (L12) or unique name. Defaults to the active layer. |
| `subpath` | integer |  | Subpath index (default 0). |
| `index` | integer | required | Node index. |
| `x` | number |  | New x. |
| `y` | number |  | New y. |
| `in` | array of numbers |  | New in-handle [x, y], or [] for a corner. |
| `out` | array of numbers |  | New out-handle [x, y], or [] for a corner. |
| `delete` | boolean |  | Remove the node. |
| `coalesce` | string |  | Consecutive edits from the same source with the same key within ~1 s fold into one undo step. Prefix a unique per-gesture key with gesture: to keep that gesture together across pauses. A different edit, undo or redo ends the group. |

### `vector.toPath`

Turn a live shape (rect, ellipse, polygon, line) into an editable path. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `layerId` | string |  | Layer id (L12) or unique name. Defaults to the active layer. |

### `vector.combine`

Path operations: combine vector layers into one path layer: union, subtract (the bottom one minus the others), intersect or exclude. The result takes the bottom layer's look. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `layerIds` | array of strings | required | Vector layers (ids or names), two or more. |
| `op` | string | required | union, subtract, intersect or exclude. |
| `keep` | boolean |  | Keep the originals (hidden). |

## text

### `text.add`

Add text: point text at (x, y), or a text frame (frameWidth, frameHeight) whose words wrap and can flow on to other frames (text.thread). {page} and {pages} become page numbers. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `text` | string | required | The words; \n starts a new paragraph. |
| `x` | number | required | Left edge. |
| `y` | number | required | Top edge. |
| `frameWidth` | number |  | Make a text frame this wide. |
| `frameHeight` | number |  | … and this tall. |
| `style` | string |  | A paragraph style by name (text.styles). |
| `font` | string |  | Font family (text.fonts lists them; Manrope and IBM Plex Mono are always there). |
| `size` | number |  | Size in pixels. |
| `weight` | integer |  | 100–900 (400 regular, 700 bold). |
| `italic` | boolean |  | Italic. |
| `color` | string |  | Colour #rrggbb or #rrggbbaa. |
| `align` | string |  | left, center or right. |
| `lineHeight` | number |  | Line spacing as a multiple of the size (1.2). |
| `letterSpacing` | number |  | Extra space between letters, pixels. |
| `paragraphSpacing` | number |  | Extra space after each paragraph, pixels. |
| `name` | string |  | Layer name. Defaults to one like "Layer 3". |
| `above` | string |  | Put it above this layer (id or name). Defaults to above the active layer. |

### `text.update`

Change a text layer: its words, its look, its position, its frame. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `layerId` | string |  | Layer id (L12) or unique name. Defaults to the active layer. |
| `text` | string |  | The words. |
| `font` | string |  | Font family (text.fonts lists them; Manrope and IBM Plex Mono are always there). |
| `size` | number |  | Size in pixels. |
| `weight` | integer |  | 100–900 (400 regular, 700 bold). |
| `italic` | boolean |  | Italic. |
| `color` | string |  | Colour #rrggbb or #rrggbbaa. |
| `align` | string |  | left, center or right. |
| `lineHeight` | number |  | Line spacing as a multiple of the size (1.2). |
| `letterSpacing` | number |  | Extra space between letters, pixels. |
| `paragraphSpacing` | number |  | Extra space after each paragraph, pixels. |
| `x` | number |  | Left edge. |
| `y` | number |  | Top edge. |
| `frameWidth` | number |  | Frame width (0 makes it point text). |
| `frameHeight` | number |  | Frame height. |
| `coalesce` | string |  | Consecutive edits from the same source with the same key within ~1 s fold into one undo step. Prefix a unique per-gesture key with gesture: to keep that gesture together across pauses. A different edit, undo or redo ends the group. |

### `text.setRuns`

Set parts of a text layer differently: runs [{start, end, style?, font?, size?, weight?, italic?, color?}] over byte ranges of its text (replaces the runs it had). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `layerId` | string |  | Layer id (L12) or unique name. Defaults to the active layer. |
| `runs` | array of objects | required | The runs. |

### `text.thread`

Thread text frames: words that don't fit in `from` continue in `to` (on any page). `to` gives up words of its own. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `from` | string | required | The frame the words come from. |
| `to` | string | required | The frame they continue in. |

### `text.unthread`

Break a thread after this frame: the words stop at its end. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `layerId` | string |  | Layer id (L12) or unique name. Defaults to the active layer. |

### `text.styles`

Paragraph and character styles. _(read only)_

### `text.defineStyle`

Make or change a paragraph style (how whole text layers are set) or a character style (what runs change). Changing a paragraph style re-sets every layer using it. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `kind` | string | required | paragraph or character. |
| `name` | string | required | Style name. |
| `font` | string |  | Font family (text.fonts lists them; Manrope and IBM Plex Mono are always there). |
| `size` | number |  | Size in pixels. |
| `weight` | integer |  | 100–900 (400 regular, 700 bold). |
| `italic` | boolean |  | Italic. |
| `color` | string |  | Colour #rrggbb or #rrggbbaa. |
| `align` | string |  | left, center or right. |
| `lineHeight` | number |  | Line spacing as a multiple of the size (1.2). |
| `letterSpacing` | number |  | Extra space between letters, pixels. |
| `paragraphSpacing` | number |  | Extra space after each paragraph, pixels. |

### `text.applyStyle`

Set text layers with a paragraph style. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `style` | string | required | Paragraph style name. |
| `layerIds` | array of strings |  | Text layers (default: the active one). |

### `text.deleteStyle`

Delete a style (layers keep how they look). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `kind` | string | required | paragraph or character. |
| `name` | string | required | Style name. |

### `text.fonts`

Font families nori can use: the bundled ones, then the system's. _(read only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `search` | string |  | Only families containing this. |

## select

### `select.get`

The selection: none, or its bounds and how much of the page it covers. _(read only)_

### `select.all`

Select the whole page. _(changes things)_

### `select.none`

Deselect. _(changes things)_

### `select.invert`

Select what isn't selected. _(changes things)_

### `select.rect`

Select a rectangle. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `x` | integer | required | Left. |
| `y` | integer | required | Top. |
| `width` | integer | required | Width. |
| `height` | integer | required | Height. |
| `combine` | string |  | How it combines with the selection there is: replace (default), add, subtract or intersect. |

### `select.ellipse`

Select an ellipse in a box. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `x` | integer | required | Left. |
| `y` | integer | required | Top. |
| `width` | integer | required | Width. |
| `height` | integer | required | Height. |
| `combine` | string |  | How it combines with the selection there is: replace (default), add, subtract or intersect. |

### `select.polygon`

Select inside a polygon (the lasso). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `points` | array of arrays | required | [[x, y], …], three or more. |
| `combine` | string |  | How it combines with the selection there is: replace (default), add, subtract or intersect. |

### `select.color`

Magic wand: select pixels like the one at (x, y), within tolerance, touching it or everywhere. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `x` | integer | required | x |
| `y` | integer | required | y |
| `tolerance` | integer |  | 0–255 (default 32). |
| `contiguous` | boolean |  | Only touching pixels (default true). |
| `sampleAll` | boolean |  | Judge by the whole picture (default true), else the active layer. |
| `combine` | string |  | How it combines with the selection there is: replace (default), add, subtract or intersect. |

### `select.layer`

Select a layer's pixels (its opacity). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `layerId` | string |  | Layer id (L12) or unique name. Defaults to the active layer. |
| `combine` | string |  | How it combines with the selection there is: replace (default), add, subtract or intersect. |

### `select.modify`

Grow, shrink or feather the selection by pixels. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `grow` | integer |  | Pixels to grow (negative shrinks). |
| `feather` | number |  | Soften the edge by this radius. |

## filter

### `filter.list`

Filters: the stock ones (blurs, sharpen, noise, pixelate) and plugins (plugin:<id>), with their parameters (ranges, defaults). _(read only)_

### `filter.apply`

Run a filter on a pixel layer, inside the selection: gaussianBlur {radius}, motionBlur {angle, distance}, sharpen {amount, radius, threshold}, noise {amount, distribution, monochromatic, seed}, pixelate {cell}, or a plugin (plugin:<id>, filter.list). Text, vector and fill layers are turned into pixels first. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `filter` | string | required | Filter id. |
| `params` | object |  | Its parameters (the rest take their defaults). |
| `layerId` | string |  | Layer id (L12) or unique name. Defaults to the active layer. |

### `filter.adjust`

Change a pixel layer's colours for good (Image › Adjustments), inside the selection: the same kinds and settings as layer.addAdjustment. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `kind` | string | required | levels, curves, hueSaturation, exposure, brightnessContrast, vibrance, invert, blackWhite, threshold or posterize. |
| `settings` | object |  | Its settings. |
| `layerId` | string |  | Layer id (L12) or unique name. Defaults to the active layer. |

## color

### `color.get`

The foreground and background colours. _(read only)_

### `color.set`

Set the foreground and/or background colour (what brushes, fills and new shapes use). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `foreground` | string |  | #rrggbb |
| `background` | string |  | #rrggbb |
| `swap` | boolean |  | Swap them. |
| `reset` | boolean |  | Back to black and white. |

### `color.swatches`

Swatches imported from .ase palettes. _(read only)_

### `color.importSwatches`

Import an Adobe Swatch Exchange (.ase) palette from Photoshop, Illustrator, InDesign or Affinity. _(changes things · permission: files)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `path` | string | required | The .ase file. |

## history

### `history.list`

The undo history: each step's command and who made it (window, agent, cli, mcp), oldest first, and the steps that can be redone. _(read only)_

### `history.undo`

Undo the last step (whoever made it). _(changes things)_

### `history.redo`

Redo the step last undone. _(changes things)_

### `history.goTo`

Go back or forward to a point in the history: `steps` steps left to undo (0: the document as it opened). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `steps` | integer | required | How many steps remain undoable. |

### `history.checkpoint`

Remember this state, to come back to it with history.revertTo. _(changes things)_

### `history.revertTo`

Back to a checkpoint, as one new undo step. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `checkpoint` | integer | required | From history.checkpoint. |

## export

### `export.formats`

File formats nori opens and writes, and the editors people come from (Photoshop, GIMP, Affinity, Pixelmator Pro, Krita, Photopea, Illustrator, Inkscape, Figma, Canva, Scribus) with which of their files nori opens. _(read only)_

### `export.file`

Write the document to a file: PNG, JPEG, WebP, TIFF, BMP (a page drawn, at any scale), OpenRaster (layers, for Krita and GIMP), SVG (a page as vectors), PDF (every page, vectors and text kept as vectors) or .nori. The format comes from the extension unless given. _(changes things · permission: files)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `path` | string | required | Destination file. |
| `format` | string |  | png, jpeg, webp, tiff, bmp, ora, svg, pdf or nori. |
| `quality` | integer |  | JPEG quality 1–100 (default 90). |
| `scale` | number |  | Picture size multiplier (2: twice the pixels; vectors and text stay sharp). |
| `page` | string |  | Page id, name or number (from 1). Defaults to the active page. |
| `pages` | array of integers |  | PDF: which pages (numbers from 1); default all. |

## handoff

### `handoff.apps`

The lsuite apps installed on this computer (from ~/.lsuite/apps) and whether they are running. _(read only)_

### `handoff.toKimchi`

Send the picture to kimchi (lsuite's video editor): exported as PNG and imported into kimchi's open project (placed on its timeline at the playhead with place), through kimchi's bridge. kimchi must be running with a project open. _(changes things · permission: files)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or number (from 1). Defaults to the active page. |
| `place` | boolean |  | Also put it on kimchi's timeline (default true). |
| `duration` | number |  | Seconds on the timeline (default kimchi's for pictures). |
| `scale` | number |  | Size multiplier. |

## account

### `account.status`

The lsuite account on this computer (shared by every lsuite app): signed in or not, email, plan, the AI allowance used and when it resets. _(read only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `refresh` | boolean |  | Ask the server again now. |

### `account.signIn`

Sign in to lsuite AI: opens the browser to sign in and connect nori (the account is shared with every lsuite app), or takes a key (lsk_…) shown on the account page. _(changes things · person only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `key` | string |  | An lsk_… key instead of the browser. |

### `account.signOut`

Sign out of lsuite AI (every lsuite app on this computer). _(changes things · person only)_

### `account.plans`

lsuite AI plans, prices (a demo: nothing is charged), models and monthly allowances, from the server. _(read only)_

## plugin

### `plugin.list`

Plugins: stock filters, installed lsuite plugins (with format, version, path, enabled) and the formats nori loads (.cube LUTs, .gbr brushes, .ase swatches). _(read only)_

### `plugin.info`

One plugin: parameters, description, where it came from. _(read only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `id` | string | required | Plugin id. |

### `plugin.enable`

Switch a plugin on. _(changes things · permission: plugins)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `id` | string | required | Plugin id. |

### `plugin.disable`

Switch a plugin off (never deletes it). _(changes things · permission: plugins)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `id` | string | required | Plugin id. |

### `plugin.rescan`

Look in the plugin folders again; changed plugins are reloaded. _(changes things)_

### `plugin.install`

Install a plugin bundle (a folder with plugin.toml and the library) and load it. _(changes things · permission: plugins)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `path` | string | required | The bundle folder. |

### `plugin.remove`

Remove an installed lsuite plugin (stock ones can only be disabled). _(changes things · permission: plugins)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `id` | string | required | Plugin id. |

### `plugin.guide`

How to write a nori plugin, for an agent: the SDK, the kinds, the manifest, an example, the rules and the recipe. _(read only)_

### `plugin.toolchain`

Whether Rust (cargo, rustc) is installed to build plugins, and how to install it. _(read only)_

### `plugin.new`

Make a plugin crate from the SDK template in ~/.lsuite/plugins-src/nori/<name>/; returns its path and files. _(changes things · permission: plugins)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `name` | string | required | Crate name (lowercase, dashes). |
| `kind` | string |  | filter (the only kind for now). |

### `plugin.writeSource`

Write one file inside a plugin crate (paths outside the crate are refused). _(changes things · permission: plugins)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `name` | string | required | The crate's name. |
| `path` | string | required | Path inside the crate, e.g. src/lib.rs. |
| `contents` | string | required | The file's contents. |

### `plugin.build`

Build a plugin crate (cargo build --release); returns ok and the compiler's errors as {file, line, message}. _(changes things · permission: plugins)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `name` | string | required | The crate's name. |

### `plugin.publishLocal`

Build a plugin crate, bundle it and install it: it loads at once, no restart. _(changes things · permission: plugins)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `name` | string | required | The crate's name. |

## app

### `app.version`

nori's version, platform, and where it keeps its files. _(read only)_

### `app.commands`

Every command with its parameters (what this list is generated from). _(read only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `family` | string |  | Only this family (doc, layer, vector…). |

### `app.settings`

Every setting and its value. _(read only)_

### `app.setSetting`

Change a setting by its dotted key (appearance.mode, tools.brush.size…). The agent's provider and permissions stay with the person. _(changes things · permission: settings)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `key` | string | required | Dotted key from app.settings. |
| `value` | any | required | New value, same type. |

### `app.checkUpdates`

Look for a newer nori on GitHub Releases. _(read only)_

### `app.whatsNew`

Release notes: this version's, or every release's. _(read only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `all` | boolean |  | Every release. |

### `app.onboarding`

The first-run setup: whether it's done, the editors people come from (with their real logos in the window) and what nori opens from each, and the agent choices. _(read only)_

### `app.finishOnboarding`

Finish (or skip) the first-run setup, remembering the editors the person came from. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `comingFrom` | array of strings |  | App ids from app.onboarding. |

### `app.notify`

Show a short message in the window. _(changes things · needs the window)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `text` | string | required | The message. |
| `kind` | string |  | info, success or error. |

### `app.quit`

Quit nori. _(changes things · permission: app control · needs the window)_

## agent

### `agent.providers`

What can run the built-in agent: lsuite AI (no setup: sign in), Claude Code and Codex on this computer, the Anthropic and OpenAI APIs, Ollama and OpenAI-compatible servers; whether each is ready and what to do next. _(read only · needs the window)_

### `agent.setProvider`

Choose what runs the built-in agent. _(changes things · person only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `provider` | string | required | lsuite, claude-code, codex, anthropic, openai, ollama or openai-compatible. |
| `model` | string |  | Model id (empty: the provider's default). |
| `baseUrl` | string |  | Server address for Ollama or OpenAI-compatible. |

### `agent.setKey`

Store an API key for a provider in the system keychain (or remove it with an empty key). _(changes things · person only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `provider` | string | required | anthropic, openai or openai-compatible. |
| `key` | string | required | The key; empty removes it. |

### `agent.send`

Ask the built-in agent (the Agent panel) to do something, in words. It runs commands like any client (permissions apply) and shows them as cards. Returns the run at once, or once it ends with wait. _(changes things · uses the agent's model · needs the window)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `prompt` | string | required | The request, e.g. "Make a poster: a big title, a photo, a date". |
| `wait` | boolean |  | Wait until the run ends (default false). |
| `timeout` | number |  | With wait: stop waiting after this many seconds (default 900). |

### `agent.status`

One agent run (the latest by default): request, whether it's working, reply, commands it ran, changes. _(read only · needs the window)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `run` | integer |  | Run id. |
| `wait` | boolean |  | Wait until it ends. |
| `timeout` | number |  | Seconds to wait. |

### `agent.conversation`

The Agent panel's conversation: requests, replies, one card per command. _(read only · needs the window)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `since` | integer |  | Only entries from this index on. |

### `agent.stop`

Stop the agent's run (finished edits stay; agent.revert removes them). _(changes things · needs the window)_

### `agent.revert`

Revert an agent run's changes, as one undo step. _(changes things · needs the window)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `run` | integer |  | Run id (default: the latest that changed something). |

### `agent.newConversation`

Start a new conversation in the Agent panel. _(changes things · needs the window)_

### `agent.models`

The models a provider offers for the agent (the chosen one by default): fetched from the provider's own list where it has one (kept for a few hours), else a short built-in list; models that can't use tools are marked tools=false. _(read only · needs the window)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `provider` | string |  | A provider id from agent.providers. |
| `refresh` | boolean |  | Fetch the list again now. |

### `agent.runs`

The agent's runs on the open document, oldest first: request, provider, outcome, changes and whether agent.revert can undo them. _(read only · needs the window)_

### `agent.conversations`

Saved conversations about the open document, with the selected conversation's id. _(read only · needs the window)_

### `agent.selectConversation`

Resume a saved conversation about this document. Stop a run first. _(changes things · needs the window)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `id` | string | required | Conversation id from agent.conversations. |

### `agent.renameConversation`

Rename the selected conversation. _(changes things · needs the window)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `title` | string | required | Title, up to 120 characters. |

### `agent.memory`

The document's memory: notes the person keeps for every conversation about it. _(read only · needs the window)_

### `agent.setMemory`

Replace the document's memory (sent with every request to the agent). _(changes things · person only · needs the window)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `text` | string | required | Notes, up to 32,000 bytes; empty clears them. |

### `agent.steer`

Redirect the agent's run in progress with a follow-up message, keeping its conversation and finished edits. _(changes things · uses the agent's model · needs the window)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `prompt` | string | required | More instructions for the run. |

## ui

### `ui.state`

What the window shows: home or editor, the tool, zoom, open panels and dialogs, theme. _(read only)_

### `ui.setTool`

Pick a tool in the window: move, select (rectangle), ellipseSelect, lasso, wand, crop, eyedropper, brush, eraser, fill, gradient, pen, direct, shape, text, frame, hand, zoom. _(changes things · needs the window)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `tool` | string | required | The tool. |

### `ui.zoom`

Zoom the canvas: a level (1 = 100 %), fit, or 100 %. _(changes things · needs the window)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `zoom` | number |  | Zoom level (0.02–64). |
| `fit` | boolean |  | Fit the page in the window. |

### `ui.showPanel`

Open or close a panel or dialog: agent, plugins, settings, export, shortcuts, whatsNew, onboarding, pages, layers; or home. _(changes things · needs the window)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `panel` | string | required | Panel name. |
| `open` | boolean |  | false closes it. |
| `section` | string |  | For settings: agent, appearance, plugins, account, about. |

### `ui.action`

Do what a keyboard shortcut or menu item does, by its action name (Undo, Redo, ZoomIn, ToggleAgent, NewDocument, Export…). _(changes things · needs the window)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `action` | string | required | Action name. |

### `ui.screenshot`

Save a PNG of the window and return its path (macOS). _(changes things · permission: files · needs the window)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `path` | string |  | Destination .png. |
