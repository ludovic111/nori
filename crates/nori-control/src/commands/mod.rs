//! Every command's spec and handler. Specs are listed here in one table so the docs, the CLI
//! help and the MCP tools are generated in a stable order.

pub mod account;
pub mod agent;
pub mod app;
pub mod color;
pub mod doc;
pub mod export;
pub mod filter;
pub mod handoff;
pub mod history;
pub mod layer;
pub mod page;
pub mod plugin;
pub mod raster;
pub mod select;
pub mod smart;
pub mod text;
pub mod ui;
pub mod util;
pub mod vector;

use std::sync::Arc;

use crate::registry::Kind::*;
use crate::registry::{Args, COALESCE, Ctx, Param, Perm, Spec, edit, opt, query, req};
use crate::session::{CmdResult, Session};

const LAYER: Param = opt("layerId", String, "Layer id (L12) or unique name. Defaults to the active layer.");
const LAYER_REQ: Param = req("layerId", String, "Layer id (L12) or unique name, as layer.list shows.");
const PAGE: Param = opt("page", String, "Page id, name or number (from 1). Defaults to the active page.");
const COLOR: Param = opt("color", String, "Colour #rrggbb or #rrggbbaa. Defaults to the foreground colour.");
const ABOVE: Param = opt("above", String, "Put it above this layer (id or name). Defaults to above the active layer.");
const NAME: Param = opt("name", String, "Layer name. Defaults to one like \"Layer 3\".");
const COMBINE: Param = opt("combine", String, "How it combines with the selection there is: replace (default), add, subtract or intersect.");
const FILL: Param = opt("fill", Any, "Fill: \"#rrggbb\", \"none\", or a paint {\"type\": \"linear\", \"x1\", \"y1\", \"x2\", \"y2\", \"stops\": [{\"offset\": 0, \"color\": \"#000\"}, …]} / {\"type\": \"radial\", \"cx\", \"cy\", \"r\", \"stops\"}.");
const STROKE: Param = opt("stroke", Any, "Stroke colour \"#rrggbb\" or a paint, or \"none\".");
const STROKE_WIDTH: Param = opt("strokeWidth", Number, "Stroke width in pixels.");
const TS0: Param = opt("font", String, "Font family (text.fonts lists them; Manrope and IBM Plex Mono are always there).");
const TS1: Param = opt("size", Number, "Size in pixels.");
const TS2: Param = opt("weight", Integer, "100–900 (400 regular, 700 bold).");
const TS3: Param = opt("italic", Boolean, "Italic.");
const TS4: Param = opt("color", String, "Colour #rrggbb or #rrggbbaa.");
const TS5: Param = opt("align", String, "left, center or right.");
const TS6: Param = opt("lineHeight", Number, "Line spacing as a multiple of the size (1.2).");
const TS7: Param = opt("letterSpacing", Number, "Extra space between letters, pixels.");
const TS8: Param = opt("paragraphSpacing", Number, "Extra space after each paragraph, pixels.");

pub static SPECS: &[Spec] = &[
    // ---- doc ------------------------------------------------------------------------------------
    query("doc.overview", "The open document in one bounded answer: pages (size, margins, columns, master), every layer on each page as a tree (kind, name, id, bounds, opacity, blend, visibility, text and shape summaries, adjustment settings), the active page and layer, the selection, styles, colours, undo history, whether it is saved, and problems (text that overflows its frames, hidden or empty layers). Read it first.", &[]),
    query("doc.get", "The complete document as JSON (the document.json of the .nori format; pixels are only sizes).", &[]),
    edit("doc.new", "Make a new document tab: a picture, a poster, a booklet. Give a size, or a preset: screen (1920×1080), square (2048), a4, a5, letter, poster-a2, story (1080×1920), instagram (1080×1350), card (1050×600). Print presets are 300 dpi.", &[
        opt("name", String, "Name (default \"Untitled\")."),
        opt("width", Integer, "Width in pixels (default 1920)."),
        opt("height", Integer, "Height in pixels (default 1080)."),
        opt("preset", String, "A size preset (overrides width and height)."),
        opt("dpi", Number, "Pixels per inch (72 screen, 300 print)."),
        opt("background", String, "Background colour #rrggbb, or \"transparent\" (default white)."),
        opt("pages", Integer, "How many pages (default 1)."),
        opt("margins", Number, "Page margins in pixels, every side (default none)."),
        opt("columns", Integer, "Columns inside the margins (default 1)."),
    ]),
    edit("doc.open", "Open a file in a new document tab: .nori, PNG, JPEG, WebP, TIFF, BMP, GIF, Photoshop .psd (with its layers), OpenRaster .ora (Krita, GIMP) or SVG (as vector layers).", &[req("path", String, "The file.")]).perm(Perm::Files),
    edit("doc.save", "Save the document as a .nori file (where it was saved before, or path).", &[opt("path", String, "A .nori file to save to (and save to from now on).")]).perm(Perm::Files),
    query("doc.list", "List open document tabs, their ids, names and unsaved changes.", &[]),
    edit("doc.select", "Switch to an open document tab, keeping its undo history.", &[req("id", Integer, "Document id from doc.list.")]),
    edit("doc.close", "Close the active tab. Unsaved changes are refused unless discard=true.", &[opt("discard", Boolean, "Explicitly discard unsaved changes.")]),
    edit("doc.setInfo", "Change the document's name, resolution (dpi: print sizes and PDF points come from it; no pixel changes) or the units the window shows.", &[
        opt("name", String, "Name."),
        opt("dpi", Number, "Pixels per inch."),
        opt("units", String, "px, pt, mm or in."),
    ]),
    edit("doc.resize", "Image size: scale every page and everything on it (pixels resampled; vectors and text scaled, staying sharp). Give a width or height (the other follows), both, or a scale.", &[
        opt("width", Integer, "New width of the active page in pixels."),
        opt("height", Integer, "New height in pixels."),
        opt("scale", Number, "Multiplier, e.g. 0.5 or 2."),
        opt("filter", String, "Resampling: bicubic (default), bilinear, lanczos or nearest."),
    ]),
    edit("doc.resizeCanvas", "Canvas size: change a page's size without scaling what is on it (layers keep their place relative to the anchor).", &[
        req("width", Integer, "New width in pixels."),
        req("height", Integer, "New height in pixels."),
        opt("anchor", String, "Where the old picture stays: center (default), top-left, top, top-right, left, right, bottom-left, bottom, bottom-right."),
        PAGE,
    ]),
    edit("doc.crop", "Crop the active page to a rectangle (or the selection's bounds): pixels outside are cut off, vectors and text move with the page.", &[
        opt("x", Integer, "Left edge."),
        opt("y", Integer, "Top edge."),
        opt("width", Integer, "Width."),
        opt("height", Integer, "Height."),
    ]),
    edit("doc.rotate", "Turn or flip the whole active page and everything on it: quarter turns clockwise (1, 2, 3 or -1), or flip horizontal/vertical.", &[
        opt("quarters", Integer, "Quarter turns clockwise."),
        opt("flip", String, "horizontal or vertical."),
    ]),
    edit("doc.batch", "Run several commands as one undo step. With atomic (the default) a failing command rolls back the ones before it.", &[
        req("commands", Array, "Array of {\"command\": \"layer.update\", \"params\": {…}}.").of(Object),
        opt("atomic", Boolean, "Roll everything back if one command fails (default true)."),
        opt("label", String, "Name of the undo step (default \"batch\")."),
    ]),
    query("doc.recent", "Files opened or saved recently, newest first.", &[]),
    // ---- page -----------------------------------------------------------------------------------
    query("page.list", "Pages (and master pages) with their size, margins, columns, master and layer count.", &[]),
    edit("page.add", "Add pages (a booklet's next spread, another artboard) after a page or at the end; the new one becomes active.", &[
        opt("count", Integer, "How many (default 1)."),
        opt("after", String, "After this page (id, name or number). Defaults to the end."),
        opt("width", Integer, "Width (default: the active page's)."),
        opt("height", Integer, "Height (default: the active page's)."),
        opt("name", String, "Name (default \"Page n\")."),
        opt("master", String, "Master page (id or name) to apply."),
        opt("duplicate", Boolean, "Copy the active page's layers onto it."),
    ]),
    edit("page.remove", "Delete a page and its layers.", &[req("page", String, "Page id, name or number.")]),
    edit("page.move", "Move a page to another position (1 is first).", &[req("page", String, "Page id, name or number."), req("to", Integer, "New position from 1.")]),
    edit("page.select", "Make a page the active one (the window shows it; commands act on it).", &[req("page", String, "Page id, name or number.")]),
    edit("page.update", "Change a page: name, size (without scaling its content), position on the pasteboard, margins, columns, gutter, bleed, master.", &[
        PAGE,
        opt("name", String, "Name."),
        opt("width", Integer, "Width in pixels."),
        opt("height", Integer, "Height in pixels."),
        opt("x", Number, "Position on the pasteboard (artboards side by side)."),
        opt("y", Number, "Position on the pasteboard."),
        opt("margins", Any, "A number for every side, or {top, right, bottom, left}."),
        opt("columns", Integer, "Columns inside the margins."),
        opt("gutter", Number, "Space between columns."),
        opt("bleed", Number, "Bleed past the trim, for print PDFs."),
        opt("master", String, "Master page id or name; \"\" removes it."),
        COALESCE,
    ]),
    edit("page.addGuide", "Add a guide line to a page (vertical at x, or horizontal at y).", &[req("at", Number, "Position in pixels."), opt("vertical", Boolean, "Vertical (default true)."), PAGE]),
    edit("page.clearGuides", "Remove a page's guides.", &[PAGE]),
    edit("page.addMaster", "Make a master page (what repeats on the pages it's applied to: page numbers, a frame, a logo), from scratch or from a page's layers. Edit it with page.select and the usual commands; apply it to pages with page.update master=<its id>.", &[
        opt("name", String, "Name (default \"A-Master\")."),
        opt("from", String, "Copy this page's layers and grid."),
    ]),
    query("page.look", "Draw a page (or a region of it) to a PNG and return its path, to look at the result. Agents that can see get the picture.", &[
        PAGE,
        opt("width", Integer, "Width of the picture in pixels (default 1024, at most the page's)."),
        opt("region", Array, "[x, y, width, height] of the page to show.").of(Number),
    ]),
    // ---- layer ----------------------------------------------------------------------------------
    query("layer.list", "The active page's layers (or a page's) as a tree, top first: id, name, kind, visible, locked, opacity, blend, bounds, mask, clipped.", &[PAGE]),
    query("layer.get", "One layer in full: its settings and content (text, shape, adjustment, pixel bounds).", &[LAYER_REQ]),
    edit("layer.add", "Add a layer above the active one: an empty pixel layer (raster), a solid colour fill, or an empty group. Becomes active.", &[
        opt("kind", String, "raster (default), fill or group."),
        NAME,
        COLOR,
        ABOVE,
    ]),
    edit("layer.addAdjustment", "Add an adjustment layer: it changes the colours of everything under it (in its group), without touching their pixels. Kinds: levels {inputBlack, inputWhite, gamma, outputBlack, outputWhite} (0–255), curves {rgb, red, green, blue: [[in, out], …] 0–255}, hueSaturation {hue −180…180, saturation, lightness −100…100, colorize}, exposure {exposure stops, offset, gamma}, brightnessContrast {brightness −150…150, contrast −50…100}, vibrance {vibrance, saturation}, invert, blackWhite {red, green, blue %}, threshold {level}, posterize {levels}.", &[
        req("kind", String, "The adjustment kind."),
        opt("settings", Object, "Its settings (the rest stay neutral)."),
        NAME,
        ABOVE,
    ]),
    edit("layer.addLut", "Add a Color Lookup adjustment layer from a .cube LUT file (a film look, a grade from Resolve or Premiere).", &[req("path", String, "The .cube file."), opt("amount", Number, "0–1 (default 1)."), ABOVE]).perm(Perm::Files),
    edit("layer.makeSmartObject", "Embed a layer’s editable native source and keep a rendered preview. Resizing always samples the original.", &[LAYER]),
    edit("layer.resizeSmartObject", "Resize a smart object from its original source, without cumulative resampling.", &[LAYER, req("width", Integer, "Width in pixels."), req("height", Integer, "Height in pixels.")]),
    edit("layer.replaceSmartObject", "Replace the embedded source with a supported file; keep displayed size and position.", &[LAYER, req("path", String, "Replacement file.")]).perm(Perm::Files),
    edit("layer.extractSmartObject", "Save the editable embedded source as a .nori file. Edit it, save, then replace the source.", &[LAYER, req("path", String, "New .nori file; existing files are refused.")]).perm(Perm::Files),
    edit("layer.place", "Place a file on the active page as a layer: a picture as pixels (centred, or at x, y), an SVG as a group of vector layers, a .nori/.psd/.ora as a group of its layers.", &[
        req("path", String, "The file."),
        opt("x", Integer, "Left edge (pictures)."),
        opt("y", Integer, "Top edge (pictures)."),
        opt("width", Integer, "Scale a picture to this width (keeps its proportions unless height is given)."),
        opt("height", Integer, "Scale a picture to this height."),
    ]).perm(Perm::Files),
    edit("layer.update", "Change a layer: name, visible, locked, opacity (0–1), blend mode, clipped to the layer below, a group's open state.", &[
        LAYER,
        opt("name", String, "Name."),
        opt("visible", Boolean, "Shown."),
        opt("locked", Boolean, "Protected from edits."),
        opt("opacity", Number, "0–1."),
        opt("blend", String, "normal, multiply, screen, overlay, darken, lighten, color-dodge, color-burn, linear-burn, linear-dodge, hard-light, soft-light, vivid-light, linear-light, pin-light, difference, exclusion, subtract, divide, hue, saturation, color, luminosity, pass-through (groups)."),
        opt("clipped", Boolean, "Clipped to the layer under it (shows only where that layer has pixels)."),
        opt("expanded", Boolean, "A group shown open in the Layers panel."),
        opt("color", String, "A colour fill layer's colour, #rrggbb or #rrggbbaa."),
        COALESCE,
    ]),
    edit("layer.select", "Make a layer the active one (tools and commands act on it); its page becomes active too.", &[LAYER_REQ]),
    edit("layer.move", "Move a layer (its pixels, text, shape or a whole group) by dx, dy pixels, or to x, y (its top-left corner).", &[
        LAYER,
        opt("dx", Number, "Pixels right (negative: left)."),
        opt("dy", Number, "Pixels down (negative: up)."),
        opt("x", Number, "Move its left edge here."),
        opt("y", Number, "Move its top edge here."),
        COALESCE,
    ]),
    edit("layer.align", "Line layers up with the page, the margins or each other: left, center, right, top, middle, bottom; or spread them evenly (distributeHorizontal, distributeVertical).", &[
        req("layerIds", Array, "Layers (ids or names).").of(String),
        req("to", String, "left, center, right, top, middle, bottom, distributeHorizontal or distributeVertical."),
        opt("relativeTo", String, "page (default), margins or selection (the layers' own bounds)."),
    ]),
    edit("layer.reorder", "Move a layer in the stack: to the top or bottom, up or down one, above or below another layer (into its group), or into a group.", &[
        LAYER,
        opt("to", String, "top, bottom, up or down."),
        opt("above", String, "Put it above this layer."),
        opt("below", String, "Put it below this layer."),
        opt("into", String, "Put it at the top of this group."),
    ]),
    edit("layer.duplicate", "Copy a layer (above it). The copy becomes active.", &[LAYER, NAME]),
    edit("layer.delete", "Delete layers.", &[opt("layerIds", Array, "Layers to delete (default: the active one).").of(String)]),
    edit("layer.group", "Put layers in a new group (in place of the topmost).", &[req("layerIds", Array, "Layers to group (ids or names).").of(String), NAME]),
    edit("layer.ungroup", "Take a group's layers out of it and remove the group.", &[LAYER]),
    edit("layer.merge", "Merge a layer down into the pixel layer under it (down), or every visible layer of the page into one (visible). The result is pixels.", &[LAYER, opt("mode", String, "down (default) or visible.")]),
    edit("layer.flatten", "Flatten the active page into one pixel layer (what you see).", &[]),
    edit("layer.rasterize", "Turn a text, vector, fill or adjustment... layer into pixels (as it looks now).", &[LAYER]),
    edit("layer.setAdjustment", "Change an adjustment layer's settings (only the given ones).", &[LAYER, req("settings", Object, "Settings to change, e.g. {\"gamma\": 1.2}."), COALESCE]),
    edit("layer.addMask", "Add a layer mask: reveal all (white), hide all (black), or from the selection.", &[LAYER, opt("from", String, "reveal (default), hide or selection.")]),
    edit("layer.mask", "Change a layer's mask: enable or disable it, invert it, apply it (bake it into the pixels) or delete it.", &[LAYER, req("action", String, "enable, disable, invert, apply or delete.")]),
    edit("layer.transform", "Scale, rotate and flip a layer about its centre (or a point), and move it. Pixels are resampled once; text and vectors stay sharp.", &[
        LAYER,
        opt("scale", Number, "Uniform scale (1 = same)."),
        opt("scaleX", Number, "Horizontal scale."),
        opt("scaleY", Number, "Vertical scale."),
        opt("rotate", Number, "Degrees clockwise."),
        opt("flip", String, "horizontal or vertical."),
        opt("dx", Number, "Then move right."),
        opt("dy", Number, "Then move down."),
        opt("originX", Number, "Turn and scale about this point (default: the layer's centre)."),
        opt("originY", Number, "Turn and scale about this point."),
        opt("filter", String, "Resampling for pixels: bicubic (default), bilinear, lanczos or nearest."),
    ]),
    query("layer.look", "Draw one layer on its own (or what an adjustment does) to a PNG and return its path.", &[LAYER, opt("width", Integer, "Width of the picture (default 768).")]),
    // ---- raster ---------------------------------------------------------------------------------
    edit("raster.stroke", "Paint a brush (or eraser) stroke on a pixel layer through points [[x, y, pressure], …] (pressure 0–1, optional). Inside the selection only. The window's brush and eraser call this.", &[
        req("points", Array, "Points along the stroke, [[x, y] or [x, y, pressure], …].").of(Array),
        LAYER,
        COLOR,
        opt("size", Number, "Brush diameter in pixels (default: the brush tool's)."),
        opt("hardness", Number, "0 soft … 1 hard."),
        opt("opacity", Number, "0–1: the most paint the stroke lays down."),
        opt("flow", Number, "0–1: paint per dab."),
        opt("spacing", Number, "Distance between dabs as a share of the size."),
        opt("pressureSize", Boolean, "Pen pressure controls brush size."),
        opt("pressureOpacity", Boolean, "Pen pressure controls brush flow."),
        opt("erase", Boolean, "Erase instead of painting."),
        opt("tip", String, "A brush tip by name (brushes.list); default round."),
    ]),
    edit("raster.fill", "Paint bucket: fill the area like the pixel at (x, y) with a colour (within tolerance, touching it or everywhere), inside the selection. Without x and y: fill the whole selection.", &[
        LAYER,
        opt("x", Integer, "Where to click."),
        opt("y", Integer, "Where to click."),
        COLOR,
        opt("tolerance", Integer, "0–255 (default 32)."),
        opt("contiguous", Boolean, "Only touching pixels (default true)."),
        opt("sampleAll", Boolean, "Judge by the whole picture, not just the layer."),
        opt("opacity", Number, "0–1."),
    ]),
    edit("raster.gradient", "Draw a gradient on a pixel layer from (x1, y1) to (x2, y2), from one colour to another (default foreground to background), inside the selection.", &[
        req("x1", Number, "Start."),
        req("y1", Number, "Start."),
        req("x2", Number, "End."),
        req("y2", Number, "End."),
        LAYER,
        opt("from", String, "Start colour."),
        opt("to", String, "End colour (\"transparent\" fades out)."),
        opt("kind", String, "linear (default), radial or reflected."),
        opt("opacity", Number, "0–1."),
    ]),
    edit("raster.clear", "Erase the selected pixels of a layer (all of it without a selection).", &[LAYER]),
    query("raster.pick", "The colour of the picture at a pixel (as the eyedropper sees it), or of one layer.", &[req("x", Integer, "x"), req("y", Integer, "y"), opt("layerId", String, "Only this layer."), opt("setForeground", Boolean, "Also make it the foreground colour.")]),
    query("brushes.list", "Brush tips: round, and the ones imported from .gbr files.", &[]),
    edit("brushes.import", "Import a GIMP brush (.gbr, also used by Krita and Photopea) as a brush tip.", &[req("path", String, "The .gbr file.")]).perm(Perm::Files),
    // ---- vector ---------------------------------------------------------------------------------
    edit("vector.addShape", "Add a vector shape layer: rect (live corners: radius), ellipse, polygon (sides), star (sides, inner 0–1), line. Stays sharp at any size.", &[
        req("shape", String, "rect, ellipse, polygon, star or line."),
        req("x", Number, "Left edge (line: start x)."),
        req("y", Number, "Top edge (line: start y)."),
        req("width", Number, "Width (line: end x − x)."),
        req("height", Number, "Height (line: end y − y)."),
        opt("radius", Number, "rect: corner radius."),
        opt("sides", Integer, "polygon or star: points (default 5 for stars, 6 for polygons)."),
        opt("inner", Number, "star: inner radius as a share of the outer (default 0.5)."),
        FILL,
        STROKE,
        STROKE_WIDTH,
        NAME,
        ABOVE,
    ]),
    edit("vector.addPath", "Add a vector path layer from points: subpaths of nodes {x, y, in: [x, y], out: [x, y]} (handles optional: corners), or an SVG path string d (\"M10 10 C 20 0 …\").", &[
        opt("subpaths", Array, "[{\"nodes\": [{\"x\":…, \"y\":…, \"in\": [..], \"out\": [..]}], \"closed\": true}, …].").of(Object),
        opt("d", String, "An SVG path string instead."),
        FILL,
        STROKE,
        STROKE_WIDTH,
        NAME,
        ABOVE,
    ]),
    edit("vector.update", "Change a vector layer's look: fill, stroke, stroke width, caps, joins, dashes, fill rule, corner radius (rects).", &[
        LAYER,
        FILL,
        STROKE,
        STROKE_WIDTH,
        opt("cap", String, "butt, round or square."),
        opt("join", String, "miter, round or bevel."),
        opt("dash", Array, "Dash and gap lengths, [] for solid.").of(Number),
        opt("fillRule", String, "nonZero or evenOdd."),
        opt("radius", Number, "rect: corner radius."),
        COALESCE,
    ]),
    edit("vector.setGeometry", "Replace a vector layer's geometry with JSON: {\"type\": \"rect\", x, y, w, h, radius} | ellipse {cx, cy, rx, ry} | polygon {cx, cy, radius, sides, inner?, rotation} | line {x1, y1, x2, y2} | path {subpaths}.", &[LAYER, req("geometry", Object, "The geometry."), COALESCE]),
    edit("vector.editNode", "Move one node of a path (and its handles), as the direct-selection tool does. A shape becomes a path first.", &[
        LAYER,
        opt("subpath", Integer, "Subpath index (default 0)."),
        req("index", Integer, "Node index."),
        opt("x", Number, "New x."),
        opt("y", Number, "New y."),
        opt("in", Array, "New in-handle [x, y], or [] for a corner.").of(Number),
        opt("out", Array, "New out-handle [x, y], or [] for a corner.").of(Number),
        opt("delete", Boolean, "Remove the node."),
        COALESCE,
    ]),
    edit("vector.toPath", "Turn a live shape (rect, ellipse, polygon, line) into an editable path.", &[LAYER]),
    edit("vector.combine", "Path operations: combine vector layers into one path layer: union, subtract (the bottom one minus the others), intersect or exclude. The result takes the bottom layer's look.", &[req("layerIds", Array, "Vector layers (ids or names), two or more.").of(String), req("op", String, "union, subtract, intersect or exclude."), opt("keep", Boolean, "Keep the originals (hidden).")]),
    // ---- text -----------------------------------------------------------------------------------
    edit("text.add", "Add text: point text at (x, y), or a text frame (frameWidth, frameHeight) whose words wrap and can flow on to other frames (text.thread). {page} and {pages} become page numbers.", &[
        req("text", String, "The words; \\n starts a new paragraph."),
        req("x", Number, "Left edge."),
        req("y", Number, "Top edge."),
        opt("frameWidth", Number, "Make a text frame this wide."),
        opt("frameHeight", Number, "… and this tall."),
        opt("style", String, "A paragraph style by name (text.styles)."),
        TS0, TS1, TS2, TS3, TS4, TS5, TS6, TS7, TS8,
        NAME,
        ABOVE,
    ]),
    edit("text.update", "Change a text layer: its words, its look, its position, its frame.", &[
        LAYER,
        opt("text", String, "The words."),
        TS0, TS1, TS2, TS3, TS4, TS5, TS6, TS7, TS8,
        opt("x", Number, "Left edge."),
        opt("y", Number, "Top edge."),
        opt("frameWidth", Number, "Frame width (0 makes it point text)."),
        opt("frameHeight", Number, "Frame height."),
        COALESCE,
    ]),
    edit("text.setRuns", "Set parts of a text layer differently: runs [{start, end, style?, font?, size?, weight?, italic?, color?}] over byte ranges of its text (replaces the runs it had).", &[LAYER, req("runs", Array, "The runs.").of(Object)]),
    edit("text.thread", "Thread text frames: words that don't fit in `from` continue in `to` (on any page). `to` gives up words of its own.", &[req("from", String, "The frame the words come from."), req("to", String, "The frame they continue in.")]),
    edit("text.unthread", "Break a thread after this frame: the words stop at its end.", &[LAYER]),
    query("text.styles", "Paragraph and character styles.", &[]),
    edit("text.defineStyle", "Make or change a paragraph style (how whole text layers are set) or a character style (what runs change). Changing a paragraph style re-sets every layer using it.", &[
        req("kind", String, "paragraph or character."),
        req("name", String, "Style name."),
        TS0, TS1, TS2, TS3, TS4, TS5, TS6, TS7, TS8,
    ]),
    edit("text.applyStyle", "Set text layers with a paragraph style.", &[req("style", String, "Paragraph style name."), opt("layerIds", Array, "Text layers (default: the active one).").of(String)]),
    edit("text.deleteStyle", "Delete a style (layers keep how they look).", &[req("kind", String, "paragraph or character."), req("name", String, "Style name.")]),
    query("text.fonts", "Font families nori can use: the bundled ones, then the system's.", &[opt("search", String, "Only families containing this.")]),
    // ---- select ---------------------------------------------------------------------------------
    query("select.get", "The selection: none, or its bounds and how much of the page it covers.", &[]),
    edit("select.all", "Select the whole page.", &[]),
    edit("select.none", "Deselect.", &[]),
    edit("select.invert", "Select what isn't selected.", &[]),
    edit("select.rect", "Select a rectangle.", &[req("x", Integer, "Left."), req("y", Integer, "Top."), req("width", Integer, "Width."), req("height", Integer, "Height."), COMBINE]),
    edit("select.ellipse", "Select an ellipse in a box.", &[req("x", Integer, "Left."), req("y", Integer, "Top."), req("width", Integer, "Width."), req("height", Integer, "Height."), COMBINE]),
    edit("select.polygon", "Select inside a polygon (the lasso).", &[req("points", Array, "[[x, y], …], three or more.").of(Array), COMBINE]),
    edit("select.color", "Magic wand: select pixels like the one at (x, y), within tolerance, touching it or everywhere.", &[
        req("x", Integer, "x"),
        req("y", Integer, "y"),
        opt("tolerance", Integer, "0–255 (default 32)."),
        opt("contiguous", Boolean, "Only touching pixels (default true)."),
        opt("sampleAll", Boolean, "Judge by the whole picture (default true), else the active layer."),
        COMBINE,
    ]),
    edit("select.layer", "Select a layer's pixels (its opacity).", &[LAYER, COMBINE]),
    edit("select.modify", "Grow, shrink or feather the selection by pixels.", &[opt("grow", Integer, "Pixels to grow (negative shrinks)."), opt("feather", Number, "Soften the edge by this radius.")]),
    // ---- filter ---------------------------------------------------------------------------------
    query("filter.list", "Filters: the stock ones (blurs, sharpen, noise, pixelate) and plugins (plugin:<id>), with their parameters (ranges, defaults).", &[]),
    edit("filter.apply", "Run a filter on a pixel layer, inside the selection: gaussianBlur {radius}, motionBlur {angle, distance}, sharpen {amount, radius, threshold}, noise {amount, distribution, monochromatic, seed}, pixelate {cell}, or a plugin (plugin:<id>, filter.list). Text, vector and fill layers are turned into pixels first.", &[
        req("filter", String, "Filter id."),
        opt("params", Object, "Its parameters (the rest take their defaults)."),
        LAYER,
    ]),
    edit("filter.adjust", "Change a pixel layer's colours for good (Image › Adjustments), inside the selection: the same kinds and settings as layer.addAdjustment.", &[req("kind", String, "levels, curves, hueSaturation, exposure, brightnessContrast, vibrance, invert, blackWhite, threshold or posterize."), opt("settings", Object, "Its settings."), LAYER]),
    // ---- color ----------------------------------------------------------------------------------
    query("color.get", "The foreground and background colours.", &[]),
    edit("color.set", "Set the foreground and/or background colour (what brushes, fills and new shapes use).", &[opt("foreground", String, "#rrggbb"), opt("background", String, "#rrggbb"), opt("swap", Boolean, "Swap them."), opt("reset", Boolean, "Back to black and white.")]),
    query("color.swatches", "Swatches imported from .ase palettes.", &[]),
    edit("color.importSwatches", "Import an Adobe Swatch Exchange (.ase) palette from Photoshop, Illustrator, InDesign or Affinity.", &[req("path", String, "The .ase file.")]).perm(Perm::Files),
    // ---- history --------------------------------------------------------------------------------
    query("history.list", "The undo history: each step's command and who made it (window, agent, cli, mcp), oldest first, and the steps that can be redone.", &[]),
    edit("history.undo", "Undo the last step (whoever made it).", &[]),
    edit("history.redo", "Redo the step last undone.", &[]),
    edit("history.goTo", "Go back or forward to a point in the history: `steps` steps left to undo (0: the document as it opened).", &[req("steps", Integer, "How many steps remain undoable.")]),
    edit("history.checkpoint", "Remember this state, to come back to it with history.revertTo.", &[]),
    edit("history.revertTo", "Back to a checkpoint, as one new undo step.", &[req("checkpoint", Integer, "From history.checkpoint.")]),
    // ---- export ---------------------------------------------------------------------------------
    query("export.formats", "File formats nori opens and writes, and the editors people come from (Photoshop, GIMP, Affinity, Pixelmator Pro, Krita, Photopea, Illustrator, Inkscape, Figma, Canva, Scribus) with which of their files nori opens.", &[]),
    edit("export.file", "Write the document to a file: PNG, JPEG, WebP, TIFF, BMP (a page drawn, at any scale), OpenRaster (layers, for Krita and GIMP), SVG (a page as vectors), PDF (every page, vectors and text kept as vectors) or .nori. The format comes from the extension unless given.", &[
        req("path", String, "Destination file."),
        opt("format", String, "png, jpeg, webp, tiff, bmp, ora, svg, pdf or nori."),
        opt("quality", Integer, "JPEG quality 1–100 (default 90)."),
        opt("scale", Number, "Picture size multiplier (2: twice the pixels; vectors and text stay sharp)."),
        PAGE,
        opt("pages", Array, "PDF: which pages (numbers from 1); default all.").of(Integer),
    ]).perm(Perm::Files),
    // ---- handoff --------------------------------------------------------------------------------
    query("handoff.apps", "The lsuite apps installed on this computer (from ~/.lsuite/apps) and whether they are running.", &[]),
    edit("handoff.toKimchi", "Send the picture to kimchi (lsuite's video editor): exported as PNG and imported into kimchi's open project (placed on its timeline at the playhead with place), through kimchi's bridge. kimchi must be running with a project open.", &[
        PAGE,
        opt("place", Boolean, "Also put it on kimchi's timeline (default true)."),
        opt("duration", Number, "Seconds on the timeline (default kimchi's for pictures)."),
        opt("scale", Number, "Size multiplier."),
    ]).perm(Perm::Files),
    // ---- account (lsuite AI) --------------------------------------------------------------------
    query("account.status", "The lsuite account on this computer (shared by every lsuite app): signed in or not, email, plan, the AI allowance used and when it resets.", &[opt("refresh", Boolean, "Ask the server again now.")]),
    edit("account.signIn", "Sign in to lsuite AI: opens the browser to sign in and connect nori (the account is shared with every lsuite app), or takes a key (lsk_…) shown on the account page.", &[opt("key", String, "An lsk_… key instead of the browser.")]).perm(Perm::PersonOnly),
    edit("account.signOut", "Sign out of lsuite AI (every lsuite app on this computer).", &[]).perm(Perm::PersonOnly),
    query("account.plans", "lsuite AI plans, prices (a demo: nothing is charged), models and monthly allowances, from the server.", &[]),
    // ---- plugin ---------------------------------------------------------------------------------
    query("plugin.list", "Plugins: stock filters, installed lsuite plugins (with format, version, path, enabled) and the formats nori loads (.cube LUTs, .gbr brushes, .ase swatches).", &[]),
    query("plugin.info", "One plugin: parameters, description, where it came from.", &[req("id", String, "Plugin id.")]),
    edit("plugin.enable", "Switch a plugin on.", &[req("id", String, "Plugin id.")]).perm(Perm::Plugins),
    edit("plugin.disable", "Switch a plugin off (never deletes it).", &[req("id", String, "Plugin id.")]).perm(Perm::Plugins),
    edit("plugin.rescan", "Look in the plugin folders again; changed plugins are reloaded.", &[]),
    edit("plugin.install", "Install a plugin bundle (a folder with plugin.toml and the library) and load it.", &[req("path", String, "The bundle folder.")]).perm(Perm::Plugins),
    edit("plugin.remove", "Remove an installed lsuite plugin (stock ones can only be disabled).", &[req("id", String, "Plugin id.")]).perm(Perm::Plugins),
    query("plugin.guide", "How to write a nori plugin, for an agent: the SDK, the kinds, the manifest, an example, the rules and the recipe.", &[]),
    query("plugin.toolchain", "Whether Rust (cargo, rustc) is installed to build plugins, and how to install it.", &[]),
    edit("plugin.new", "Make a plugin crate from the SDK template in ~/.lsuite/plugins-src/nori/<name>/; returns its path and files.", &[req("name", String, "Crate name (lowercase, dashes)."), opt("kind", String, "filter (the only kind for now).")]).perm(Perm::Plugins),
    edit("plugin.writeSource", "Write one file inside a plugin crate (paths outside the crate are refused).", &[req("name", String, "The crate's name."), req("path", String, "Path inside the crate, e.g. src/lib.rs."), req("contents", String, "The file's contents.")]).perm(Perm::Plugins),
    edit("plugin.build", "Build a plugin crate (cargo build --release); returns ok and the compiler's errors as {file, line, message}.", &[req("name", String, "The crate's name.")]).perm(Perm::Plugins),
    edit("plugin.publishLocal", "Build a plugin crate, bundle it and install it: it loads at once, no restart.", &[req("name", String, "The crate's name.")]).perm(Perm::Plugins),
    // ---- app ------------------------------------------------------------------------------------
    query("app.version", "nori's version, platform, and where it keeps its files.", &[]),
    query("app.commands", "Every command with its parameters (what this list is generated from).", &[opt("family", String, "Only this family (doc, layer, vector…).")]),
    query("app.settings", "Every setting and its value.", &[]),
    edit("app.setSetting", "Change a setting by its dotted key (appearance.mode, tools.brush.size…). The agent's provider and permissions stay with the person.", &[req("key", String, "Dotted key from app.settings."), req("value", Any, "New value, same type.")]).perm(Perm::Settings),
    query("app.checkUpdates", "Look for a newer nori on GitHub Releases.", &[]),
    query("app.updateStatus", "Read update availability, download progress and restart state.", &[]),
    edit("app.installUpdate", "Download, verify and install the available signed update.", &[]).perm(Perm::AppControl),
    edit("app.restart", "Restart nori to use an installed update.", &[]).perm(Perm::AppControl).window(),
    query("app.whatsNew", "Release notes: this version's, or every release's.", &[opt("all", Boolean, "Every release.")]),
    query("app.onboarding", "The first-run setup: whether it's done, the editors people come from (with their real logos in the window) and what nori opens from each, and the agent choices.", &[]),
    edit("app.finishOnboarding", "Finish (or skip) the first-run setup, remembering the editors the person came from.", &[opt("comingFrom", Array, "App ids from app.onboarding.").of(String)]),
    edit("app.notify", "Show a short message in the window.", &[req("text", String, "The message."), opt("kind", String, "info, success or error.")]).window(),
    edit("app.quit", "Quit nori.", &[]).perm(Perm::AppControl).window(),
    // ---- agent ----------------------------------------------------------------------------------
    query("agent.providers", "What can run the built-in agent: lsuite AI (no setup: sign in), Claude Code and Codex on this computer, the Anthropic and OpenAI APIs, Ollama and OpenAI-compatible servers; whether each is ready and what to do next.", &[]).window(),
    edit("agent.setProvider", "Choose what runs the built-in agent.", &[req("provider", String, "lsuite, claude-code, codex, anthropic, openai, ollama or openai-compatible."), opt("model", String, "Model id (empty: the provider's default)."), opt("baseUrl", String, "Server address for Ollama or OpenAI-compatible.")]).perm(Perm::PersonOnly),
    edit("agent.setKey", "Store an API key for a provider in the system keychain (or remove it with an empty key).", &[req("provider", String, "anthropic, openai or openai-compatible."), req("key", String, "The key; empty removes it.")]).perm(Perm::PersonOnly),
    edit("agent.send", "Ask the built-in agent (the Agent panel) to do something, in words. It runs commands like any client (permissions apply) and shows them as cards. Returns the run at once, or once it ends with wait.", &[
        req("prompt", String, "The request, e.g. \"Make a poster: a big title, a photo, a date\"."),
        opt("wait", Boolean, "Wait until the run ends (default false)."),
        opt("timeout", Number, "With wait: stop waiting after this many seconds (default 900)."),
    ]).perm(Perm::Agent).window(),
    query("agent.status", "One agent run (the latest by default): request, whether it's working, reply, commands it ran, changes.", &[opt("run", Integer, "Run id."), opt("wait", Boolean, "Wait until it ends."), opt("timeout", Number, "Seconds to wait.")]).window(),
    query("agent.conversation", "The Agent panel's conversation: requests, replies, one card per command.", &[opt("since", Integer, "Only entries from this index on.")]).window(),
    edit("agent.stop", "Stop the agent's run (finished edits stay; agent.revert removes them).", &[]).window(),
    edit("agent.revert", "Revert an agent run's changes, as one undo step.", &[opt("run", Integer, "Run id (default: the latest that changed something).")]).window(),
    edit("agent.newConversation", "Start a new conversation in the Agent panel.", &[]).window(),
    query("agent.models", "The models a provider offers for the agent (the chosen one by default): fetched from the provider's own list where it has one (kept for a few hours), else a short built-in list; models that can't use tools are marked tools=false.", &[opt("provider", String, "A provider id from agent.providers."), opt("refresh", Boolean, "Fetch the list again now.")]).window(),
    query("agent.runs", "The agent's runs on the open document, oldest first: request, provider, outcome, changes and whether agent.revert can undo them.", &[]).window(),
    query("agent.conversations", "Saved conversations about the open document, with the selected conversation's id.", &[]).window(),
    edit("agent.selectConversation", "Resume a saved conversation about this document. Stop a run first.", &[req("id", String, "Conversation id from agent.conversations.")]).window(),
    edit("agent.renameConversation", "Rename the selected conversation.", &[req("title", String, "Title, up to 120 characters.")]).window(),
    query("agent.memory", "The document's memory: notes the person keeps for every conversation about it.", &[]).window(),
    edit("agent.setMemory", "Replace the document's memory (sent with every request to the agent).", &[req("text", String, "Notes, up to 32,000 bytes; empty clears them.")]).perm(Perm::PersonOnly).window(),
    edit("agent.steer", "Redirect the agent's run in progress with a follow-up message, keeping its conversation and finished edits.", &[req("prompt", String, "More instructions for the run.")]).perm(Perm::Agent).window(),
    // ---- ui -------------------------------------------------------------------------------------
    query("ui.state", "What the window shows: home or editor, the tool, zoom, open panels and dialogs, theme.", &[]),
    edit("ui.setTool", "Pick a tool in the window: move, select (rectangle), ellipseSelect, lasso, wand, crop, eyedropper, brush, eraser, fill, gradient, pen, direct, shape, text, frame, hand, zoom.", &[req("tool", String, "The tool.")]).window(),
    edit("ui.zoom", "Zoom the canvas: a level (1 = 100 %), fit, or 100 %.", &[opt("zoom", Number, "Zoom level (0.02–64)."), opt("fit", Boolean, "Fit the page in the window.")]).window(),
    edit("ui.showPanel", "Open or close a panel or dialog: agent, plugins, settings, export, shortcuts, whatsNew, onboarding, pages, layers; or home.", &[req("panel", String, "Panel name."), opt("open", Boolean, "false closes it."), opt("section", String, "For settings: agent, appearance, plugins, account, about.")]).window(),
    edit("ui.action", "Do what a keyboard shortcut or menu item does, by its action name (Undo, Redo, ZoomIn, ToggleAgent, NewDocument, Export…).", &[req("action", String, "Action name.")]).window(),
    edit("ui.screenshot", "Save a PNG of the window and return its path (macOS).", &[opt("path", String, "Destination .png.")]).perm(Perm::Files).window(),
];

/// Runs the handler for a validated command.
pub async fn dispatch(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    match cx.spec.family() {
        "doc" => Box::pin(doc::run(s, cx, a)).await,
        "page" => Box::pin(page::run(s, cx, a)).await,
        "layer" => Box::pin(layer::run(s, cx, a)).await,
        "raster" | "brushes" => Box::pin(raster::run(s, cx, a)).await,
        "vector" => Box::pin(vector::run(s, cx, a)).await,
        "text" => Box::pin(text::run(s, cx, a)).await,
        "select" => Box::pin(select::run(s, cx, a)).await,
        "filter" => Box::pin(filter::run(s, cx, a)).await,
        "color" => Box::pin(color::run(s, cx, a)).await,
        "history" => Box::pin(history::run(s, cx, a)).await,
        "export" => Box::pin(export::run(s, cx, a)).await,
        "handoff" => Box::pin(handoff::run(s, cx, a)).await,
        "account" => Box::pin(account::run(s, cx, a)).await,
        "plugin" => Box::pin(plugin::run(s, cx, a)).await,
        "app" => Box::pin(app::run(s, cx, a)).await,
        "agent" => Box::pin(agent::run(s, cx, a)).await,
        "ui" => Box::pin(ui::run(s, cx, a)).await,
        _ => Err(unhandled(cx)),
    }
}

pub(crate) fn unhandled(cx: &Ctx) -> std::string::String {
    format!("`{}` is not implemented", cx.spec.name)
}
