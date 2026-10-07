//! The canvas: the active page drawn from tiles, zoomed and panned, and every tool's gestures.
//!
//! Drawing: the page is composited in 256-pixel tiles (nori-render, in parallel, off the
//! interface thread) and kept; zoomed out, a pyramid of half-size tiles is built from them, so
//! panning and zooming only move pictures the GPU already has (60 fps on big images). When the
//! document changes, only the tiles the change touched are composited again (`dirty`: pixel
//! tiles that are no longer the same, and the boxes of layers whose settings changed). A brush
//! stroke is painted into a copy of its layer as the pointer moves, with the same brush engine
//! the `raster.stroke` command runs, so the result never jumps when the command lands.
//!
//! Gestures end in commands (`select.rect`, `layer.move`, `raster.stroke`, `vector.addShape`…):
//! the canvas never changes the document itself.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gpui::{
    App, Bounds, ContentMask, Context, Corners, Entity, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathBuilder, PinchEvent, Pixels, Point,
    RenderImage, ScrollDelta, ScrollWheelEvent, Task, Window, canvas, div, point, prelude::*, px, size,
};
use nori_core::vector::{Geometry, Node, Seg};
use nori_core::{Content, Document, Layer, Rect, TILE};
use nori_render::Prepared;
use serde_json::json;

use crate::store::{ShapeKind, Store, StoreEvent, StoreExt, Tool};
use crate::theme::ActiveTheme;

type Key = (u8, u32, u32);

/// A tile's pixels (straight RGBA, `TILE`×`TILE`, transparent outside the page).
type Buf = Arc<Vec<u8>>;

#[derive(Clone)]
enum Drag {
    Pan { from: Point<Pixels>, pan: (f32, f32) },
    Marquee { start: (f32, f32), now: (f32, f32), ellipse: bool, combine: &'static str },
    Lasso { points: Vec<[f32; 2]>, combine: &'static str },
    Move { id: String, start: (f32, f32), sent: (i32, i32), gesture: String },
    Gradient { start: (f32, f32), now: (f32, f32) },
    Crop { start: (f32, f32), now: (f32, f32) },
    Shape { start: (f32, f32), now: (f32, f32), square: bool },
    Frame { start: (f32, f32), now: (f32, f32) },
    Node { id: String, sub: usize, index: usize, part: u8, gesture: String },
    Stroke,
    PenHandle,
}

/// A brush or eraser stroke being painted.
struct Live {
    layer: String,
    stroke: nori_render::brush::Stroke,
    raster: nori_core::Raster,
    doc: Arc<Document>,
    points: Vec<[f32; 3]>,
    erase: bool,
    color: nori_core::Color,
    brush: nori_render::brush::Brush,
}

pub struct CanvasView {
    store: Entity<Store>,
    /// What the tiles were drawn from: document, its id and revision, and the page.
    doc: Option<Arc<Document>>,
    key: Option<(u64, u64, String)>,
    prep: Option<Arc<Prepared>>,
    tiles0: HashMap<(u32, u32), Buf>,
    pyramid: HashMap<Key, Buf>,
    images: HashMap<Key, Arc<RenderImage>>,
    garbage: Vec<Arc<RenderImage>>,
    job: Option<Task<()>>,
    generation: u64,
    checker: Option<(bool, f32, Arc<RenderImage>)>,
    ants: HashMap<Key, Arc<RenderImage>>,
    ants_for: usize,
    pub zoom: f32,
    /// Where the page's top-left corner is, in canvas pixels.
    pan: (f32, f32),
    bounds: Bounds<Pixels>,
    fit_pending: bool,
    drag: Option<Drag>,
    live: Option<Live>,
    /// The pen's path in progress.
    pen: Vec<Node>,
    pen_closed: bool,
    pointer: Option<(f32, f32)>,
    gesture: u64,
    _subs: Vec<gpui::Subscription>,
}

impl CanvasView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let store = cx.store();
        let subs = vec![
            cx.observe(&store, |_, _, cx| cx.notify()),
            cx.subscribe(&store, |c: &mut Self, _, e: &StoreEvent, cx| match e {
                StoreEvent::FitView => {
                    c.fit_pending = true;
                    cx.notify();
                }
                StoreEvent::Zoom { by, to } => {
                    let center = (f32::from(c.bounds.size.width) / 2.0, f32::from(c.bounds.size.height) / 2.0);
                    let z = to.unwrap_or(c.zoom * by);
                    c.zoom_about(z, center, cx);
                }
                _ => {}
            }),
        ];
        Self {
            store,
            doc: None,
            key: None,
            prep: None,
            tiles0: HashMap::new(),
            pyramid: HashMap::new(),
            images: HashMap::new(),
            garbage: vec![],
            job: None,
            generation: 0,
            checker: None,
            ants: HashMap::new(),
            ants_for: 0,
            zoom: 1.0,
            pan: (40.0, 40.0),
            bounds: Bounds::default(),
            fit_pending: true,
            drag: None,
            live: None,
            pen: vec![],
            pen_closed: false,
            pointer: None,
            gesture: 0,
            _subs: subs,
        }
    }

    // ---- view ------------------------------------------------------------------------------

    fn page_size(&self) -> (u32, u32) {
        self.doc.as_ref().map(|d| (d.width(), d.height())).unwrap_or((1, 1))
    }

    /// Fits the page in the view (with a margin).
    pub fn fit(&mut self, cx: &mut Context<Self>) {
        let (w, h) = self.page_size();
        let (bw, bh) = (f32::from(self.bounds.size.width), f32::from(self.bounds.size.height));
        if bw < 10.0 || bh < 10.0 {
            self.fit_pending = true;
            return;
        }
        let z = ((bw - 80.0) / w as f32).min((bh - 80.0) / h as f32).clamp(0.01, 16.0);
        self.zoom = z;
        self.pan = ((bw - w as f32 * z) / 2.0, (bh - h as f32 * z) / 2.0);
        self.fit_pending = false;
        self.publish_zoom(cx);
        cx.notify();
    }

    pub fn actual_size(&mut self, cx: &mut Context<Self>) {
        let c = (f32::from(self.bounds.size.width) / 2.0, f32::from(self.bounds.size.height) / 2.0);
        self.zoom_about(1.0, c, cx);
    }

    fn zoom_about(&mut self, z: f32, at: (f32, f32), cx: &mut Context<Self>) {
        let z = z.clamp(0.02, 64.0);
        let (dx, dy) = ((at.0 - self.pan.0) / self.zoom, (at.1 - self.pan.1) / self.zoom);
        self.zoom = z;
        self.pan = (at.0 - dx * z, at.1 - dy * z);
        self.publish_zoom(cx);
        cx.notify();
    }

    fn publish_zoom(&self, cx: &mut Context<Self>) {
        let z = self.zoom;
        self.store.update(cx, |s, cx| {
            if (s.zoom - z).abs() > 1e-4 {
                s.zoom = z;
                s.sync_ui(cx);
                cx.notify();
            }
        });
    }

    /// Canvas pixels → page pixels.
    fn to_page(&self, p: Point<Pixels>) -> (f32, f32) {
        let x = f32::from(p.x - self.bounds.origin.x);
        let y = f32::from(p.y - self.bounds.origin.y);
        ((x - self.pan.0) / self.zoom, (y - self.pan.1) / self.zoom)
    }

    /// Page pixels → window pixels.
    fn to_window(&self, x: f32, y: f32) -> Point<Pixels> {
        point(self.bounds.origin.x + px(self.pan.0 + x * self.zoom), self.bounds.origin.y + px(self.pan.1 + y * self.zoom))
    }

    fn level(&self) -> u8 {
        if self.zoom >= 1.0 { 0 } else { ((1.0 / self.zoom).log2().floor() as u8).min(8) }
    }

    // ---- tiles -----------------------------------------------------------------------------

    /// Follows the store's document: what changed is drawn again.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let s = self.store.read(cx);
        let Some(doc) = s.doc.clone() else {
            if self.doc.is_some() {
                self.reset_tiles();
                self.doc = None;
                self.key = None;
            }
            return;
        };
        let key = (s.doc_id.unwrap_or(0), s.revision, doc.page().id.clone());
        if self.key.as_ref() == Some(&key) {
            return;
        }
        // A stroke in progress keeps its own picture until its command lands.
        if let Some(live) = &self.live
            && self.drag.is_none()
        {
            let _ = live;
            self.live = None;
        }
        let same_doc = self.key.as_ref().is_some_and(|k| k.0 == key.0 && k.2 == key.2);
        let dirty = match (&self.doc, same_doc) {
            (Some(old), true) => dirty(old, &doc),
            _ => Dirty::All,
        };
        if !same_doc {
            if self.key.as_ref().is_none_or(|k| k.0 != key.0) {
                self.fit_pending = true;
            }
            self.pen.clear();
        }
        self.apply_dirty(&dirty);
        self.doc = Some(doc.clone());
        self.key = Some(key);
        self.prep = None;
        self.schedule(cx);
    }

    fn reset_tiles(&mut self) {
        self.tiles0.clear();
        self.pyramid.clear();
        self.garbage.extend(self.images.drain().map(|(_, v)| v));
        self.garbage.extend(self.ants.drain().map(|(_, v)| v));
        self.generation += 1;
        self.job = None;
    }

    fn apply_dirty(&mut self, d: &Dirty) {
        match d {
            Dirty::All => self.reset_tiles(),
            Dirty::Rects(rects) => {
                let mut hit: HashSet<(u32, u32)> = HashSet::new();
                for r in rects {
                    if r.is_empty() {
                        continue;
                    }
                    let (x0, y0) = (r.x.max(0) as u32 / TILE, r.y.max(0) as u32 / TILE);
                    let (x1, y1) = ((r.right().max(1) - 1).max(0) as u32 / TILE, (r.bottom().max(1) - 1).max(0) as u32 / TILE);
                    for ty in y0..=y1.min(4096) {
                        for tx in x0..=x1.min(4096) {
                            hit.insert((tx, ty));
                        }
                    }
                }
                for (tx, ty) in hit {
                    self.tiles0.remove(&(tx, ty));
                    for l in 0..=8u8 {
                        let k = (l, tx >> l, ty >> l);
                        if l > 0 {
                            self.pyramid.remove(&k);
                        }
                        if let Some(img) = self.images.remove(&k) {
                            self.garbage.push(img);
                        }
                    }
                }
                self.generation += 1;
                self.job = None;
            }
        }
    }

    /// Composites the page's missing tiles in the background.
    fn schedule(&mut self, cx: &mut Context<Self>) {
        let Some(doc) = self.doc.clone() else { return };
        if self.job.is_some() {
            return;
        }
        let (w, h) = (doc.width(), doc.height());
        let (gx, gy) = (w.div_ceil(TILE), h.div_ceil(TILE));
        // Visible tiles first.
        let vis = self.visible_range(0);
        let mut missing: Vec<(u32, u32)> = (0..gy).flat_map(|y| (0..gx).map(move |x| (x, y))).filter(|k| !self.tiles0.contains_key(k)).collect();
        if missing.is_empty() {
            return;
        }
        missing.sort_by_key(|(x, y)| {
            let inside = vis.is_some_and(|(x0, y0, x1, y1)| *x >= x0 && *x <= x1 && *y >= y0 && *y <= y1);
            (!inside, *y, *x)
        });
        let generation = self.generation;
        let prep = self.prep.clone();
        let live = self.live.as_ref().map(|l| l.doc.clone());
        let doc2 = live.unwrap_or(doc);
        let task = cx.background_spawn(async move {
            use rayon::prelude::*;
            let prep = prep.unwrap_or_else(|| Arc::new(nori_render::prepare(&doc2)));
            let page = doc2.page().clone();
            let out: Vec<((u32, u32), Buf)> = missing.par_iter().map(|(x, y)| ((*x, *y), Arc::new(composite_tile(&doc2, &page, &prep, *x, *y)))).collect();
            (out, prep)
        });
        self.job = Some(cx.spawn(async move |this, cx| {
            let (out, prep) = task.await;
            this.update(cx, |c, cx| {
                if c.generation != generation {
                    c.job = None;
                    c.schedule(cx);
                    return;
                }
                c.prep = Some(prep);
                for (k, buf) in out {
                    c.tiles0.insert(k, buf);
                }
                c.job = None;
                cx.notify();
            })
            .ok();
        }));
    }

    /// The tile range at `level` the view shows: (x0, y0, x1, y1) inclusive.
    fn visible_range(&self, level: u8) -> Option<(u32, u32, u32, u32)> {
        let (w, h) = self.page_size();
        let span = (TILE << level) as f32;
        let (bw, bh) = (f32::from(self.bounds.size.width), f32::from(self.bounds.size.height));
        if bw <= 0.0 {
            return None;
        }
        let x0 = ((-self.pan.0) / self.zoom / span).floor().max(0.0) as u32;
        let y0 = ((-self.pan.1) / self.zoom / span).floor().max(0.0) as u32;
        let x1 = ((bw - self.pan.0) / self.zoom / span).floor().max(0.0) as u32;
        let y1 = ((bh - self.pan.1) / self.zoom / span).floor().max(0.0) as u32;
        let (gx, gy) = (w.div_ceil(TILE << level), h.div_ceil(TILE << level));
        if gx == 0 || gy == 0 || x0 >= gx || y0 >= gy {
            return None;
        }
        Some((x0, y0, x1.min(gx - 1), y1.min(gy - 1)))
    }

    /// A tile's pixels at a level (built from the level below), if its parts are ready.
    fn buffer(&mut self, k: Key) -> Option<Buf> {
        let (l, x, y) = k;
        if l == 0 {
            return self.tiles0.get(&(x, y)).cloned();
        }
        if let Some(b) = self.pyramid.get(&k) {
            return Some(b.clone());
        }
        let (w, h) = self.page_size();
        let below = l - 1;
        let (gx, gy) = (w.div_ceil(TILE << below), h.div_ceil(TILE << below));
        let mut parts = [None, None, None, None];
        for (i, (dx, dy)) in [(0, 0), (1, 0), (0, 1), (1, 1)].into_iter().enumerate() {
            let (cx_, cy_) = (x * 2 + dx, y * 2 + dy);
            if cx_ >= gx || cy_ >= gy {
                continue;
            }
            parts[i] = Some(self.buffer((below, cx_, cy_))?);
        }
        let buf = Arc::new(downsample(&parts));
        self.pyramid.insert(k, buf.clone());
        Some(buf)
    }

    fn image(&mut self, k: Key) -> Option<Arc<RenderImage>> {
        if let Some(i) = self.images.get(&k) {
            return Some(i.clone());
        }
        let buf = self.buffer(k)?;
        let img = to_image(&buf, TILE, TILE)?;
        self.images.insert(k, img.clone());
        Some(img)
    }

    // ---- painting a stroke -----------------------------------------------------------------

    fn start_stroke(&mut self, p: (f32, f32), erase: bool, cx: &mut Context<Self>) -> bool {
        let s = self.store.read(cx);
        let Some(doc) = s.doc.clone() else { return false };
        let Some(id) = doc.active_id() else { return false };
        let tools = s.settings.tools.clone();
        let color = s.colors.foreground;
        let brushes = s.session.brushes.read().clone();
        let kind = doc.layer(&id).map(|l| l.content.kind()).unwrap_or("");
        if kind != "raster" {
            self.store.update(cx, |s, cx| s.flash("The brush paints on pixel layers: pick one, or add one with Layer › New Layer.", cx));
            return false;
        }
        if doc.layer(&id).is_some_and(|l| l.locked || !l.visible) {
            self.store.update(cx, |s, cx| s.flash("That layer is locked or hidden.", cx));
            return false;
        }
        // The same growth the command does, on a copy.
        let mut d = (*doc).clone();
        let Ok(origin) = nori_control::commands::util::prepare_paint(&mut d, &id) else { return false };
        let Ok(raster) = nori_control::commands::util::raster_of(&mut d, &id).map(|r| r.clone()) else { return false };
        let brush = if erase { tools.eraser } else { tools.brush };
        let tip = brush.tip.as_ref().and_then(|n| brushes.iter().find(|t| &t.name == n).cloned());
        let mode = if erase { nori_render::brush::Mode::Erase } else { nori_render::brush::Mode::Paint(color) };
        let stroke = nori_render::brush::Stroke::new(&raster, origin, brush.clone(), mode, d.selection.clone(), tip);
        self.live = Some(Live { layer: id, stroke, raster, doc: Arc::new(d), points: vec![], erase, color, brush });
        self.stroke_to(p, cx);
        true
    }

    fn stroke_to(&mut self, p: (f32, f32), cx: &mut Context<Self>) {
        let Some(live) = &mut self.live else { return };
        let pt = [p.0, p.1, 1.0];
        if live.points.last().is_some_and(|q| (q[0] - pt[0]).abs() < 0.25 && (q[1] - pt[1]).abs() < 0.25) {
            return;
        }
        live.points.push(pt);
        live.stroke.add(pt);
        let changed = live.stroke.apply(&mut live.raster);
        if changed.is_empty() {
            return;
        }
        // The live picture: the document with this layer's pixels.
        let mut d = (*live.doc).clone();
        if let Ok(r) = nori_control::commands::util::raster_of(&mut d, &live.layer) {
            *r = live.raster.clone();
        }
        let d = Arc::new(d);
        live.doc = d.clone();
        // Draw the touched tiles now (a dab touches a few).
        self.apply_dirty(&Dirty::Rects(vec![changed]));
        let prep = self.prep.clone().unwrap_or_else(|| Arc::new(nori_render::prepare(&d)));
        let page = d.page().clone();
        let (w, h) = (d.width(), d.height());
        let (gx, gy) = (w.div_ceil(TILE), h.div_ceil(TILE));
        let need: Vec<(u32, u32)> = (0..gy).flat_map(|y| (0..gx).map(move |x| (x, y))).filter(|k| !self.tiles0.contains_key(k)).filter(|(x, y)| changed.intersect(&Rect::new((*x * TILE) as i32, (*y * TILE) as i32, TILE, TILE)) != Rect::default()).collect();
        use rayon::prelude::*;
        let out: Vec<((u32, u32), Buf)> = need.par_iter().map(|(x, y)| ((*x, *y), Arc::new(composite_tile(&d, &page, &prep, *x, *y)))).collect();
        for (k, b) in out {
            self.tiles0.insert(k, b);
        }
        cx.notify();
    }

    fn finish_stroke(&mut self, cx: &mut Context<Self>) {
        let Some(live) = &self.live else { return };
        let b = &live.brush;
        let params = json!({
            "layerId": live.layer,
            "points": live.points,
            "color": live.color.hex(),
            "size": b.size,
            "hardness": b.hardness,
            "opacity": b.opacity,
            "flow": b.flow,
            "spacing": b.spacing,
            "erase": live.erase,
            "tip": b.tip,
        });
        self.store.update(cx, |s, cx| s.run("raster.stroke", params, cx));
    }

    // ---- hit tests -------------------------------------------------------------------------

    /// The topmost visible layer with something at page point `p`.
    fn layer_at(&self, p: (f32, f32)) -> Option<String> {
        let doc = self.doc.as_ref()?;
        fn find(list: &[Layer], p: (f32, f32)) -> Option<String> {
            for l in list {
                if !l.visible {
                    continue;
                }
                let hit = match &l.content {
                    Content::Group { children, .. } => {
                        if let Some(id) = find(children, p) {
                            return Some(id);
                        }
                        false
                    }
                    Content::Raster { x, y, pixels } => pixels.get(p.0 as i32 - x, p.1 as i32 - y)[3] > 16,
                    Content::Vector { shape } => nori_render::shapes::hit(shape, p.0, p.1, 3.0),
                    Content::Text { text } => nori_render::text::measure(text).contains(p.0 as i32, p.1 as i32),
                    _ => false,
                };
                if hit && !l.locked {
                    return Some(l.id.clone());
                }
            }
            None
        }
        find(&doc.page().layers, p)
    }

    /// The node (or handle: part 1 in, 2 out) of the active vector layer under the pointer.
    fn node_at(&self, p: (f32, f32)) -> Option<(String, usize, usize, u8)> {
        let doc = self.doc.as_ref()?;
        let id = doc.active_id()?;
        let Content::Vector { shape } = &doc.layer(&id)?.content else { return None };
        let path = shape.geometry.to_path();
        let Geometry::Path { subpaths } = &path else { return None };
        let m = shape.transform;
        let slop = 7.0 / self.zoom;
        for (si, sp) in subpaths.iter().enumerate() {
            for (ni, n) in sp.nodes.iter().enumerate() {
                for (part, at) in [(0u8, Some([n.x, n.y])), (1, n.handle_in), (2, n.handle_out)] {
                    if let Some(a) = at {
                        let q = nori_core::vector::apply(&m, a);
                        if (q[0] - p.0).abs() <= slop && (q[1] - p.1).abs() <= slop {
                            return Some((id, si, ni, part));
                        }
                    }
                }
            }
        }
        None
    }

    // ---- pointer ---------------------------------------------------------------------------

    fn combine_of(&self, e_shift: bool, e_alt: bool, cx: &App) -> &'static str {
        if e_shift {
            "add"
        } else if e_alt {
            "subtract"
        } else {
            self.store.read(cx).combine
        }
    }

    fn down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.doc.is_none() {
            return;
        }
        let p = self.to_page(e.position);
        let tool = self.store.read(cx).tool;
        if e.button == MouseButton::Middle || tool == Tool::Hand {
            self.drag = Some(Drag::Pan { from: e.position, pan: self.pan });
            cx.notify();
            return;
        }
        if e.button != MouseButton::Left {
            return;
        }
        let combine = self.combine_of(e.modifiers.shift, e.modifiers.alt, cx);
        self.gesture += 1;
        let gesture = format!("gesture:canvas-{}", self.gesture);
        match tool {
            Tool::Zoom => {
                let at = (f32::from(e.position.x - self.bounds.origin.x), f32::from(e.position.y - self.bounds.origin.y));
                let z = if e.modifiers.alt { self.zoom / 2.0 } else { self.zoom * 2.0 };
                self.zoom_about(z, at, cx);
            }
            Tool::Move => {
                let doc = self.doc.clone().expect("doc");
                let hit = self.layer_at(p);
                let id = match (hit, doc.active_id()) {
                    (Some(h), _) => {
                        if doc.active_id().as_deref() != Some(&h) {
                            self.store.update(cx, |s, cx| s.run("layer.select", json!({ "layerId": h }), cx));
                        }
                        Some(h)
                    }
                    (None, a) => a,
                };
                if let Some(id) = id {
                    self.drag = Some(Drag::Move { id, start: p, sent: (0, 0), gesture });
                }
            }
            Tool::Direct => {
                if let Some((id, sub, index, part)) = self.node_at(p) {
                    self.drag = Some(Drag::Node { id, sub, index, part, gesture });
                } else if let Some(h) = self.layer_at(p) {
                    self.store.update(cx, |s, cx| s.run("layer.select", json!({ "layerId": h }), cx));
                }
            }
            Tool::Select | Tool::EllipseSelect => self.drag = Some(Drag::Marquee { start: p, now: p, ellipse: tool == Tool::EllipseSelect, combine }),
            Tool::Lasso => self.drag = Some(Drag::Lasso { points: vec![[p.0, p.1]], combine }),
            Tool::Wand => {
                let s = self.store.read(cx);
                let params = json!({ "x": p.0 as i64, "y": p.1 as i64, "tolerance": s.settings.tools.tolerance, "contiguous": s.settings.tools.contiguous, "combine": combine });
                self.store.update(cx, |s, cx| s.run("select.color", params, cx));
            }
            Tool::Crop => self.drag = Some(Drag::Crop { start: p, now: p }),
            Tool::Eyedropper => {
                let bg = e.modifiers.alt;
                self.store.update(cx, |s, cx| {
                    s.run_then("raster.pick", json!({ "x": p.0 as i64, "y": p.1 as i64 }), cx, move |s, v, cx| {
                        if let Some(c) = v["color"].as_str() {
                            let c = c.chars().take(7).collect::<String>();
                            s.run("color.set", if bg { json!({ "background": c }) } else { json!({ "foreground": c }) }, cx);
                        }
                    })
                });
            }
            Tool::Brush | Tool::Eraser => {
                if self.start_stroke(p, tool == Tool::Eraser, cx) {
                    self.drag = Some(Drag::Stroke);
                }
            }
            Tool::Fill => {
                let s = self.store.read(cx);
                let params = json!({ "x": p.0 as i64, "y": p.1 as i64, "tolerance": s.settings.tools.tolerance, "contiguous": s.settings.tools.contiguous, "sampleAll": s.settings.tools.sample_all });
                self.store.update(cx, |s, cx| s.run("raster.fill", params, cx));
            }
            Tool::Gradient => self.drag = Some(Drag::Gradient { start: p, now: p }),
            Tool::Shape => self.drag = Some(Drag::Shape { start: p, now: p, square: e.modifiers.shift }),
            Tool::Frame => self.drag = Some(Drag::Frame { start: p, now: p }),
            Tool::Text => self.drag = Some(Drag::Frame { start: p, now: p }),
            Tool::Pen => {
                // Clicking the first point closes the path.
                if let Some(first) = self.pen.first()
                    && self.pen.len() >= 2
                    && (first.x - p.0).abs() * self.zoom < 8.0
                    && (first.y - p.1).abs() * self.zoom < 8.0
                {
                    self.pen_closed = true;
                    self.finish_pen(cx);
                    return;
                }
                if e.click_count >= 2 && self.pen.len() >= 2 {
                    self.finish_pen(cx);
                    return;
                }
                self.pen.push(Node::corner(p.0, p.1));
                self.drag = Some(Drag::PenHandle);
            }
            Tool::Hand => {}
        }
        window.prevent_default();
        cx.notify();
    }

    fn finish_pen(&mut self, cx: &mut Context<Self>) {
        let nodes = std::mem::take(&mut self.pen);
        let closed = std::mem::take(&mut self.pen_closed);
        if nodes.len() < 2 {
            return;
        }
        let s = self.store.read(cx);
        let fg = s.colors.foreground.hex();
        let width = s.settings.tools.shape_stroke_width;
        let params = json!({ "subpaths": [{ "nodes": nodes, "closed": closed }], "stroke": fg, "strokeWidth": width, "fill": if closed { json!(s.settings.tools.shape_fill) } else { json!("none") } });
        self.store.update(cx, |s, cx| s.run("vector.addPath", params, cx));
        cx.notify();
    }

    /// Esc: drop what the current tool was doing.
    pub fn cancel(&mut self, cx: &mut Context<Self>) -> bool {
        let busy = !self.pen.is_empty() || self.drag.is_some();
        self.pen.clear();
        self.drag = None;
        cx.notify();
        busy
    }

    /// Enter: finish the pen's path.
    pub fn confirm(&mut self, cx: &mut Context<Self>) -> bool {
        if self.pen.len() >= 2 {
            self.finish_pen(cx);
            return true;
        }
        false
    }

    fn moved(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let p = self.to_page(e.position);
        self.pointer = Some(p);
        let Some(drag) = self.drag.clone() else {
            if self.store.read(cx).tool == Tool::Pen && !self.pen.is_empty() {
                cx.notify();
            }
            return;
        };
        match drag {
            Drag::Pan { from, pan } => {
                self.pan = (pan.0 + f32::from(e.position.x - from.x), pan.1 + f32::from(e.position.y - from.y));
            }
            Drag::Marquee { start, ellipse, combine, .. } => self.drag = Some(Drag::Marquee { start, now: p, ellipse, combine }),
            Drag::Lasso { mut points, combine } => {
                if points.last().is_none_or(|q| (q[0] - p.0).abs() + (q[1] - p.1).abs() > 1.0 / self.zoom) {
                    points.push([p.0, p.1]);
                }
                self.drag = Some(Drag::Lasso { points, combine });
            }
            Drag::Move { id, start, sent, gesture } => {
                let (dx, dy) = ((p.0 - start.0).round() as i32, (p.1 - start.1).round() as i32);
                let (dx, dy) = if e.modifiers.shift { if dx.abs() > dy.abs() { (dx, 0) } else { (0, dy) } } else { (dx, dy) };
                if (dx, dy) != sent {
                    let step = json!({ "layerId": id, "dx": dx - sent.0, "dy": dy - sent.1, "coalesce": gesture });
                    self.store.update(cx, |s, cx| s.run("layer.move", step, cx));
                    self.drag = Some(Drag::Move { id, start, sent: (dx, dy), gesture });
                }
            }
            Drag::Gradient { start, .. } => self.drag = Some(Drag::Gradient { start, now: p }),
            Drag::Crop { start, .. } => self.drag = Some(Drag::Crop { start, now: p }),
            Drag::Shape { start, .. } => self.drag = Some(Drag::Shape { start, now: p, square: e.modifiers.shift }),
            Drag::Frame { start, .. } => self.drag = Some(Drag::Frame { start, now: p }),
            Drag::Node { id, sub, index, part, gesture } => {
                let key = match part {
                    0 => "x",
                    1 => "in",
                    _ => "out",
                };
                let params = if part == 0 { json!({ "layerId": id, "subpath": sub, "index": index, "x": p.0, "y": p.1, "coalesce": gesture }) } else { json!({ "layerId": id, "subpath": sub, "index": index, key: [p.0, p.1], "coalesce": gesture }) };
                self.store.update(cx, |s, cx| s.run("vector.editNode", params, cx));
            }
            Drag::Stroke => self.stroke_to(p, cx),
            Drag::PenHandle => {
                if let Some(n) = self.pen.last_mut() {
                    // Dragging out a smooth point: the out handle follows the pointer, the in
                    // handle mirrors it.
                    let (dx, dy) = (p.0 - n.x, p.1 - n.y);
                    if dx.abs() + dy.abs() > 2.0 / self.zoom {
                        n.handle_out = Some([n.x + dx, n.y + dy]);
                        n.handle_in = Some([n.x - dx, n.y - dy]);
                    }
                }
            }
        }
        cx.notify();
    }

    fn up(&mut self, e: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else { return };
        let tiny = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 2.0 && (a.1 - b.1).abs() < 2.0;
        let rect = |a: (f32, f32), b: (f32, f32)| (a.0.min(b.0).round(), a.1.min(b.1).round(), (a.0 - b.0).abs().round(), (a.1 - b.1).abs().round());
        let _ = e;
        match drag {
            Drag::Marquee { start, now, ellipse, combine } => {
                if tiny(start, now) {
                    self.store.update(cx, |s, cx| s.run("select.none", json!({}), cx));
                } else {
                    let (x, y, w, h) = rect(start, now);
                    let cmd = if ellipse { "select.ellipse" } else { "select.rect" };
                    self.store.update(cx, |s, cx| s.run(cmd, json!({ "x": x, "y": y, "width": w, "height": h, "combine": combine }), cx));
                }
            }
            Drag::Lasso { points, combine } => {
                if points.len() >= 3 {
                    self.store.update(cx, |s, cx| s.run("select.polygon", json!({ "points": points, "combine": combine }), cx));
                }
            }
            Drag::Gradient { start, now } => {
                if !tiny(start, now) {
                    let kind = self.store.read(cx).settings.tools.gradient.clone();
                    self.store.update(cx, |s, cx| s.run("raster.gradient", json!({ "x1": start.0, "y1": start.1, "x2": now.0, "y2": now.1, "kind": kind }), cx));
                }
            }
            Drag::Crop { start, now } => {
                if !tiny(start, now) {
                    let (x, y, w, h) = rect(start, now);
                    self.store.update(cx, |s, cx| s.run("doc.crop", json!({ "x": x, "y": y, "width": w, "height": h }), cx));
                }
            }
            Drag::Shape { start, now, square } => {
                let (mut x, mut y, mut w, mut h) = rect(start, now);
                let kind = self.store.read(cx).shape;
                if kind == ShapeKind::Line {
                    let params = json!({ "shape": "line", "x": start.0, "y": start.1, "width": now.0 - start.0, "height": now.1 - start.1 });
                    if !tiny(start, now) {
                        self.store.update(cx, |s, cx| s.run("vector.addShape", params, cx));
                    }
                } else {
                    if tiny(start, now) {
                        // A click: a shape of a default size there.
                        (x, y, w, h) = (start.0 - 100.0, start.1 - 100.0, 200.0, 200.0);
                    }
                    if square {
                        let m = w.max(h);
                        (w, h) = (m, m);
                    }
                    self.store.update(cx, |s, cx| s.run("vector.addShape", json!({ "shape": kind.id(), "x": x, "y": y, "width": w, "height": h }), cx));
                }
            }
            Drag::Frame { start, now } => {
                let tool = self.store.read(cx).tool;
                let size = self.store.read(cx).settings.tools.font_size as f32;
                let params = if tiny(start, now) {
                    json!({ "text": if tool == Tool::Frame { "Text frame" } else { "Text" }, "x": start.0, "y": start.1 - size * 0.2, "size": size })
                } else {
                    let (x, y, w, h) = rect(start, now);
                    json!({ "text": "Text frame: type or paste the words in Properties, or thread it to another frame.", "x": x, "y": y, "frameWidth": w.max(20.0), "frameHeight": h.max(20.0), "size": (size / 2.0).max(12.0) })
                };
                self.store.update(cx, |s, cx| {
                    s.run_then("text.add", params, cx, |_, _, cx| cx.emit(StoreEvent::EditText));
                    s.set_tool(Tool::Move, cx);
                });
            }
            Drag::Stroke => self.finish_stroke(cx),
            Drag::Pan { .. } | Drag::Move { .. } | Drag::Node { .. } | Drag::PenHandle => {}
        }
        cx.notify();
    }

    fn scroll(&mut self, e: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let (dx, dy) = match e.delta {
            ScrollDelta::Pixels(p) => (f32::from(p.x), f32::from(p.y)),
            ScrollDelta::Lines(l) => (l.x * 40.0, l.y * 40.0),
        };
        if e.modifiers.secondary() || e.modifiers.control {
            let at = (f32::from(e.position.x - self.bounds.origin.x), f32::from(e.position.y - self.bounds.origin.y));
            let z = self.zoom * (1.0 + dy * 0.01).clamp(0.5, 2.0);
            self.zoom_about(z, at, cx);
        } else {
            self.pan = (self.pan.0 + dx, self.pan.1 + dy);
            cx.notify();
        }
    }

    fn pinch(&mut self, e: &PinchEvent, _: &mut Window, cx: &mut Context<Self>) {
        let at = (f32::from(e.position.x - self.bounds.origin.x), f32::from(e.position.y - self.bounds.origin.y));
        let z = self.zoom * (1.0 + e.delta);
        self.zoom_about(z, at, cx);
    }

    // ---- paint -----------------------------------------------------------------------------

    fn checker(&mut self, dark: bool, scale: f32) -> Arc<RenderImage> {
        if let Some((d, s, img)) = &self.checker
            && *d == dark
            && *s == scale
        {
            return img.clone();
        }
        let n = (256.0 * scale).round() as u32;
        let cell = (8.0 * scale).round().max(1.0) as u32;
        let (a, b) = if dark { (0x2e, 0x26) } else { (0xff, 0xe6) };
        let mut rgba = vec![0u8; (n * n * 4) as usize];
        for y in 0..n {
            for x in 0..n {
                let v = if ((x / cell) + (y / cell)) % 2 == 0 { a } else { b };
                let o = ((y * n + x) * 4) as usize;
                rgba[o..o + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        let img = to_image(&rgba, n, n).expect("checker");
        if let Some((_, _, old)) = self.checker.replace((dark, scale, img.clone())) {
            self.garbage.push(old);
        }
        img
    }

    /// The selection's outline at a level, for one tile: black and white dashes on its edge.
    fn ants(&mut self, k: Key) -> Option<Arc<RenderImage>> {
        let doc = self.doc.clone()?;
        let sel = doc.selection.as_ref()?;
        let id = sel.tile_arc(0, 0).map(|a| Arc::as_ptr(a) as usize).unwrap_or(0) ^ (sel.used_tiles() << 1) ^ (sel.fill()[0] as usize) ^ (self.key.as_ref().map_or(0, |k| k.1 as usize) << 8);
        if id != self.ants_for {
            self.garbage.extend(self.ants.drain().map(|(_, v)| v));
            self.ants_for = id;
        }
        if let Some(i) = self.ants.get(&k) {
            return Some(i.clone());
        }
        let (l, tx, ty) = k;
        let step = 1u32 << l;
        let (ox, oy) = ((tx * TILE) << l, (ty * TILE) << l);
        let inside = |x: i64, y: i64| -> bool { x >= 0 && y >= 0 && sel.get((x * step as i64) as i32, (y * step as i64) as i32)[0] >= 128 };
        let mut rgba = vec![0u8; (TILE * TILE * 4) as usize];
        let mut any = false;
        for y in 0..TILE {
            for x in 0..TILE {
                let (gx, gy) = ((ox / step + x) as i64, (oy / step + y) as i64);
                let me = inside(gx, gy);
                if me && (!inside(gx - 1, gy) || !inside(gx + 1, gy) || !inside(gx, gy - 1) || !inside(gx, gy + 1)) {
                    let v = if ((gx + gy) / 4) % 2 == 0 { 0 } else { 255 };
                    let o = ((y * TILE + x) * 4) as usize;
                    rgba[o..o + 4].copy_from_slice(&[v, v, v, 255]);
                    any = true;
                }
            }
        }
        if !any {
            return None;
        }
        let img = to_image(&rgba, TILE, TILE)?;
        self.ants.insert(k, img.clone());
        Some(img)
    }
}

/// What changed between two states of a document, for drawing.
pub enum Dirty {
    All,
    Rects(Vec<Rect>),
}

fn layer_box(l: &Layer) -> Option<Rect> {
    match &l.content {
        Content::Raster { x, y, pixels } => Some(Rect::new(*x, *y, pixels.width(), pixels.height())),
        Content::Text { text } => {
            // The frame or the words, with room for what hangs past them.
            let r = nori_render::text::measure(text);
            let pad = (text.size * 0.6) as i32 + 4;
            Some(r.inflate(pad))
        }
        Content::Vector { shape } => Some(shape.bounds()),
        Content::Group { children, .. } => {
            let mut r = Rect::default();
            for c in children {
                r = r.union(&layer_box(c)?);
            }
            Some(r)
        }
        Content::Fill { .. } | Content::Adjustment { .. } => None,
    }
}

/// The page boxes a change between `old` and `new` touched (`All` when it can't say).
pub fn dirty(old: &Document, new: &Document) -> Dirty {
    let (a, b) = (old.page(), new.page());
    if a.id != b.id || a.width != b.width || a.height != b.height || a.master != b.master {
        return Dirty::All;
    }
    // A master page in use changed, or a text thread did: everything (cheap to say, rare).
    if serde_json::to_value(&old.masters).ok() != serde_json::to_value(&new.masters).ok() || serde_json::to_value(&old.styles).ok() != serde_json::to_value(&new.styles).ok() {
        return Dirty::All;
    }
    let mut rects = vec![];
    fn walk(a: &[Layer], b: &[Layer], rects: &mut Vec<Rect>) -> bool {
        if a.len() != b.len() || a.iter().zip(b).any(|(x, y)| x.id != y.id) {
            // Layers added, removed or reordered: the boxes of all of them.
            for l in a.iter().chain(b.iter()) {
                match layer_box(l) {
                    Some(r) => rects.push(r),
                    None => return false,
                }
            }
            return true;
        }
        for (x, y) in a.iter().zip(b) {
            let same_meta = serde_json::to_value(x).ok() == serde_json::to_value(y).ok();
            match (&x.content, &y.content) {
                (Content::Raster { x: ax, y: ay, pixels: pa }, Content::Raster { x: bx, y: by, pixels: pb }) if same_meta && ax == bx && ay == by => {
                    let (gx, gy) = pa.grid();
                    for ty in 0..gy {
                        for tx in 0..gx {
                            if !pa.same_tile(pb, tx, ty) {
                                rects.push(pa.tile_rect(tx, ty).translate(*ax, *ay));
                            }
                        }
                    }
                }
                (Content::Group { children: ca, .. }, Content::Group { children: cb, .. }) if x.opacity == y.opacity && x.blend == y.blend && x.visible == y.visible && x.mask.is_none() && y.mask.is_none() => {
                    if !walk(ca, cb, rects) {
                        return false;
                    }
                }
                _ if same_meta => {}
                _ => {
                    // Text frames reflow their thread: other frames change too.
                    if matches!(x.content, Content::Text { .. }) || matches!(x.content, Content::Adjustment { .. } | Content::Fill { .. }) {
                        return false;
                    }
                    match (layer_box(x), layer_box(y)) {
                        (Some(r1), Some(r2)) => {
                            rects.push(r1);
                            rects.push(r2);
                        }
                        _ => return false,
                    }
                }
            }
            // Masks.
            if let (Some(ma), Some(mb)) = (&x.mask, &y.mask) {
                let (gx, gy) = ma.mask.grid();
                for ty in 0..gy {
                    for tx in 0..gx {
                        if !ma.mask.same_tile(&mb.mask, tx, ty) {
                            rects.push(ma.mask.tile_rect(tx, ty));
                        }
                    }
                }
            } else if x.mask.is_some() != y.mask.is_some() {
                match layer_box(y).or(layer_box(x)) {
                    Some(r) => rects.push(r),
                    None => return false,
                }
            }
        }
        true
    }
    if walk(&a.layers, &b.layers, &mut rects) { Dirty::Rects(rects) } else { Dirty::All }
}

/// One tile of a page composited: straight RGBA, `TILE`×`TILE`, transparent past the page.
pub fn composite_tile(doc: &Document, page: &nori_core::Page, prep: &Prepared, tx: u32, ty: u32) -> Vec<u8> {
    let r = Rect::new((tx * TILE) as i32, (ty * TILE) as i32, TILE.min(page.width.saturating_sub(tx * TILE)), TILE.min(page.height.saturating_sub(ty * TILE)));
    let mut out = vec![0u8; (TILE * TILE * 4) as usize];
    if r.is_empty() {
        return out;
    }
    let px = nori_render::composite::to_rgba8(&nori_render::composite_page(doc, page, prep, r));
    for row in 0..r.h {
        let src = (row * r.w * 4) as usize;
        let dst = (row * TILE * 4) as usize;
        out[dst..dst + (r.w * 4) as usize].copy_from_slice(&px[src..src + (r.w * 4) as usize]);
    }
    out
}

/// Four tiles (top-left, top-right, bottom-left, bottom-right) into one at half size.
fn downsample(parts: &[Option<Buf>; 4]) -> Vec<u8> {
    let mut out = vec![0u8; (TILE * TILE * 4) as usize];
    let half = TILE / 2;
    for (i, part) in parts.iter().enumerate() {
        let Some(src) = part else { continue };
        let (ox, oy) = ((i as u32 % 2) * half, (i as u32 / 2) * half);
        for y in 0..half {
            for x in 0..half {
                let mut acc = [0u32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let o = (((y * 2 + dy) * TILE + x * 2 + dx) * 4) as usize;
                    let a = src[o + 3] as u32;
                    acc[0] += src[o] as u32 * a;
                    acc[1] += src[o + 1] as u32 * a;
                    acc[2] += src[o + 2] as u32 * a;
                    acc[3] += a;
                }
                let d = (((oy + y) * TILE + ox + x) * 4) as usize;
                if acc[3] > 0 {
                    out[d] = (acc[0] / acc[3]) as u8;
                    out[d + 1] = (acc[1] / acc[3]) as u8;
                    out[d + 2] = (acc[2] / acc[3]) as u8;
                    out[d + 3] = (acc[3] / 4) as u8;
                }
            }
        }
    }
    out
}

/// Straight RGBA → a GPUI picture (BGRA).
pub fn to_image(rgba: &[u8], w: u32, h: u32) -> Option<Arc<RenderImage>> {
    let mut bgra = rgba.to_vec();
    for p in bgra.chunks_exact_mut(4) {
        p.swap(0, 2);
    }
    let buf = image::RgbaImage::from_raw(w, h, bgra)?;
    Some(Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(buf)])))
}

struct Frame {
    images: Vec<(Bounds<Pixels>, Arc<RenderImage>)>,
    checker: Arc<RenderImage>,
    checker_size: f32,
    page: Bounds<Pixels>,
    overlays: Vec<Overlay>,
    edge: gpui::Hsla,
}

enum Overlay {
    Image(Bounds<Pixels>, Arc<RenderImage>),
    Path(Vec<Point<Pixels>>, bool, gpui::Hsla, f32, bool),
    Curve(Vec<Seg>, gpui::Hsla, f32, bool),
    Rect(Bounds<Pixels>, gpui::Hsla, bool),
    Dot(Point<Pixels>, gpui::Hsla, f32),
}

impl Render for CanvasView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        for old in self.garbage.drain(..) {
            let _ = window.drop_image(old);
        }
        self.sync(cx);
        if self.fit_pending && f32::from(self.bounds.size.width) > 10.0 {
            self.fit(cx);
        }
        let t = cx.theme().clone();
        let scale = window.scale_factor();
        let dark = t.is_dark();
        let pasteboard = match self.store.read(cx).settings.appearance.pasteboard.as_str() {
            "light" => crate::theme::grey(0.80),
            "mid" => crate::theme::grey(0.40),
            _ => if dark { crate::theme::grey(0.09) } else { crate::theme::grey(0.78) },
        };
        let entity = cx.entity();
        let mut frame = None;
        if let Some(doc) = self.doc.clone() {
            let (w, h) = (doc.width() as f32, doc.height() as f32);
            let o = self.bounds.origin;
            let page = Bounds::new(point(o.x + px(self.pan.0), o.y + px(self.pan.1)), size(px(w * self.zoom), px(h * self.zoom)));
            let level = self.level();
            let mut images = vec![];
            if let Some((x0, y0, x1, y1)) = self.visible_range(level) {
                let span = (TILE << level) as f32 * self.zoom;
                for ty in y0..=y1 {
                    for tx in x0..=x1 {
                        // This level's tile, or the full-size ones under it while it builds.
                        let b = Bounds::new(point(page.origin.x + px(tx as f32 * span), page.origin.y + px(ty as f32 * span)), size(px(span), px(span)));
                        if let Some(img) = self.image((level, tx, ty)) {
                            images.push((b, img));
                        }
                    }
                }
            }
            if self.job.is_none() && self.tiles0.len() < (doc.width().div_ceil(TILE) * doc.height().div_ceil(TILE)) as usize {
                self.schedule(cx);
            }
            let mut overlays = vec![];
            // Selection.
            if doc.selection.is_some()
                && let Some((x0, y0, x1, y1)) = self.visible_range(level)
            {
                let span = (TILE << level) as f32 * self.zoom;
                for ty in y0..=y1 {
                    for tx in x0..=x1 {
                        if let Some(img) = self.ants((level, tx, ty)) {
                            let b = Bounds::new(point(page.origin.x + px(tx as f32 * span), page.origin.y + px(ty as f32 * span)), size(px(span), px(span)));
                            overlays.push(Overlay::Image(b, img));
                        }
                    }
                }
            }
            let accent = t.accent;
            let guide = gpui::rgb(0x2fa8ff).into();
            let p = doc.page();
            // Margins and columns, guides.
            if p.margins.top + p.margins.left + p.margins.right + p.margins.bottom > 0.0 || p.columns > 1 {
                for c in p.column_boxes() {
                    let a = self.to_window(c[0], c[1]);
                    let b = self.to_window(c[0] + c[2], c[1] + c[3]);
                    overlays.push(Overlay::Rect(Bounds::from_corners(a, b), gpui::Hsla::from(gpui::rgb(0xd04fd0)).opacity(0.7), false));
                }
            }
            for g in &p.guides {
                let pts = if g.vertical { vec![self.to_window(g.at, -100000.0), self.to_window(g.at, 100000.0)] } else { vec![self.to_window(-100000.0, g.at), self.to_window(100000.0, g.at)] };
                overlays.push(Overlay::Path(pts, false, guide, 1.0, false));
            }
            // Text frames show their box; the active layer its bounds.
            for l in p.all() {
                if let Content::Text { text } = &l.content
                    && let Some([fw, fh]) = text.frame
                {
                    let a = self.to_window(text.x, text.y);
                    let b = self.to_window(text.x + fw, text.y + fh);
                    overlays.push(Overlay::Rect(Bounds::from_corners(a, b), t.text_3.opacity(0.6), true));
                }
            }
            let tool = self.store.read(cx).tool;
            if let Some(id) = doc.active_id()
                && let Some(l) = doc.layer(&id)
                && matches!(tool, Tool::Move | Tool::Direct | Tool::Text | Tool::Frame | Tool::Shape)
                && let Some(r) = layer_box(l).filter(|_| !matches!(l.content, Content::Raster { .. }) || tool == Tool::Move)
            {
                let r = if let Content::Raster { .. } = l.content { l.raster().and_then(|(x, y, px_)| px_.content_bounds().map(|b| b.translate(x, y))).unwrap_or(r) } else if let Content::Vector { shape } = &l.content { shape.bounds().inflate(-2) } else if let Content::Text { text } = &l.content { nori_render::text::measure(text) } else { r };
                let a = self.to_window(r.x as f32, r.y as f32);
                let b = self.to_window(r.right() as f32, r.bottom() as f32);
                overlays.push(Overlay::Rect(Bounds::from_corners(a, b), accent.opacity(0.9), false));
                if tool == Tool::Direct
                    && let Content::Vector { shape } = &l.content
                {
                    overlays.push(Overlay::Curve(shape.transformed().iter().map(|s| self.map_seg(*s)).collect(), accent, 1.0, false));
                    if let Geometry::Path { subpaths } = shape.geometry.to_path() {
                        for sp in subpaths {
                            for n in sp.nodes {
                                let q = nori_core::vector::apply(&shape.transform, [n.x, n.y]);
                                let at = self.to_window(q[0], q[1]);
                                for h in [n.handle_in, n.handle_out].into_iter().flatten() {
                                    let hq = nori_core::vector::apply(&shape.transform, h);
                                    let hp = self.to_window(hq[0], hq[1]);
                                    overlays.push(Overlay::Path(vec![at, hp], false, accent.opacity(0.6), 1.0, false));
                                    overlays.push(Overlay::Dot(hp, accent, 3.0));
                                }
                                overlays.push(Overlay::Dot(at, accent, 4.0));
                            }
                        }
                    }
                }
            }
            // Gestures in progress.
            match &self.drag {
                Some(Drag::Marquee { start, now, ellipse, .. }) => {
                    let (a, b) = (self.to_window(start.0, start.1), self.to_window(now.0, now.1));
                    let r = Bounds::from_corners(point(a.x.min(b.x), a.y.min(b.y)), point(a.x.max(b.x), a.y.max(b.y)));
                    if *ellipse {
                        let g = Geometry::Ellipse { cx: (start.0 + now.0) / 2.0, cy: (start.1 + now.1) / 2.0, rx: (now.0 - start.0).abs() / 2.0, ry: (now.1 - start.1).abs() / 2.0 };
                        overlays.push(Overlay::Curve(g.segments().iter().map(|s| self.map_seg(*s)).collect(), accent, 1.0, true));
                    } else {
                        overlays.push(Overlay::Rect(r, accent, true));
                    }
                }
                Some(Drag::Crop { start, now }) => {
                    let (a, b) = (self.to_window(start.0, start.1), self.to_window(now.0, now.1));
                    overlays.push(Overlay::Rect(Bounds::from_corners(point(a.x.min(b.x), a.y.min(b.y)), point(a.x.max(b.x), a.y.max(b.y))), accent, false));
                }
                Some(Drag::Shape { start, now, .. }) | Some(Drag::Frame { start, now }) => {
                    let (a, b) = (self.to_window(start.0, start.1), self.to_window(now.0, now.1));
                    overlays.push(Overlay::Rect(Bounds::from_corners(point(a.x.min(b.x), a.y.min(b.y)), point(a.x.max(b.x), a.y.max(b.y))), accent, true));
                }
                Some(Drag::Lasso { points, .. }) => {
                    let pts: Vec<Point<Pixels>> = points.iter().map(|p| self.to_window(p[0], p[1])).collect();
                    overlays.push(Overlay::Path(pts, true, accent, 1.0, true));
                }
                Some(Drag::Gradient { start, now }) => {
                    overlays.push(Overlay::Path(vec![self.to_window(start.0, start.1), self.to_window(now.0, now.1)], false, accent, 1.5, false));
                    overlays.push(Overlay::Dot(self.to_window(start.0, start.1), accent, 4.0));
                }
                _ => {}
            }
            // The pen's path, and where the next point would go.
            if !self.pen.is_empty() {
                let mut nodes = self.pen.clone();
                if let (Some(p), None) = (self.pointer, &self.drag) {
                    nodes.push(Node::corner(p.0, p.1));
                }
                let g = Geometry::Path { subpaths: vec![nori_core::vector::SubPath { nodes, closed: false }] };
                overlays.push(Overlay::Curve(g.segments().iter().map(|s| self.map_seg(*s)).collect(), accent, 1.5, false));
                for n in &self.pen {
                    overlays.push(Overlay::Dot(self.to_window(n.x, n.y), accent, 4.0));
                }
            }
            // The brush's size under the pointer.
            if let (Some(p), true) = (self.pointer, matches!(tool, Tool::Brush | Tool::Eraser)) {
                let s = self.store.read(cx);
                let size = if tool == Tool::Brush { s.settings.tools.brush.size } else { s.settings.tools.eraser.size };
                let r = size / 2.0;
                let g = Geometry::Ellipse { cx: p.0, cy: p.1, rx: r, ry: r };
                overlays.push(Overlay::Curve(g.segments().iter().map(|s| self.map_seg(*s)).collect(), if dark { gpui::white().opacity(0.8) } else { gpui::black().opacity(0.8) }, 1.0, false));
            }
            frame = Some(Frame { images, checker: self.checker(dark, scale), checker_size: 256.0, page, overlays, edge: t.line });
        }
        let cursor = match self.store.read(cx).tool {
            Tool::Hand => gpui::CursorStyle::OpenHand,
            Tool::Text | Tool::Frame => gpui::CursorStyle::IBeam,
            Tool::Move | Tool::Direct => gpui::CursorStyle::Arrow,
            _ => gpui::CursorStyle::Crosshair,
        };
        let e1 = entity.clone();
        div()
            .id("canvas")
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(pasteboard)
            .cursor(cursor)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::down))
            .on_mouse_move(cx.listener(Self::moved))
            // A release before the next frame (so before the window-wide tracker is there) ends the drag too.
            .on_mouse_up(MouseButton::Left, cx.listener(Self::up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::up))
            .on_scroll_wheel(cx.listener(Self::scroll))
            .on_pinch(cx.listener(Self::pinch))
            .when(self.drag.is_some(), |d| d.child(crate::ui::drag::track(entity.clone(), |c, e, w, cx| c.moved(e, w, cx), |c, e, w, cx| c.up(e, w, cx))))
            .child(
                canvas(
                    move |bounds, _, cx| {
                        e1.update(cx, |c, cx| {
                            if c.bounds != bounds {
                                c.bounds = bounds;
                                cx.notify();
                            }
                        });
                    },
                    move |_, (), window, _| {
                        let Some(f) = frame else { return };
                        // The page: a hard shadow, the checkerboard, the pictures.
                        let shadow = Bounds::new(point(f.page.origin.x + px(4.), f.page.origin.y + px(4.)), f.page.size);
                        window.paint_quad(gpui::fill(shadow, gpui::black().opacity(0.35)));
                        window.with_content_mask(Some(ContentMask { bounds: f.page }), |window| {
                            let cs = px(f.checker_size);
                            let (mut y, end_y) = (f.page.origin.y, f.page.origin.y + f.page.size.height);
                            while y < end_y {
                                let mut x = f.page.origin.x;
                                while x < f.page.origin.x + f.page.size.width {
                                    let b = Bounds::new(point(x, y), size(cs, cs));
                                    let _ = window.paint_image(b, b, Corners::default(), f.checker.clone(), 0, false);
                                    x += cs;
                                }
                                y += cs;
                            }
                            for (b, img) in &f.images {
                                let _ = window.paint_image(*b, *b, Corners::default(), img.clone(), 0, false);
                            }
                        });
                        window.paint_quad(gpui::outline(f.page, f.edge, gpui::BorderStyle::Solid));
                        for o in f.overlays {
                            match o {
                                Overlay::Image(b, img) => {
                                    let _ = window.paint_image(b, b, Corners::default(), img, 0, false);
                                }
                                Overlay::Rect(b, c, dashed) => {
                                    window.paint_quad(gpui::outline(b, c, if dashed { gpui::BorderStyle::Dashed } else { gpui::BorderStyle::Solid }));
                                }
                                Overlay::Path(pts, closed, c, w, dashed) => {
                                    if pts.len() < 2 {
                                        continue;
                                    }
                                    let mut pb = PathBuilder::stroke(px(w));
                                    if dashed {
                                        pb = pb.dash_array(&[px(4.), px(4.)]);
                                    }
                                    pb.move_to(pts[0]);
                                    for p in &pts[1..] {
                                        pb.line_to(*p);
                                    }
                                    if closed {
                                        pb.close();
                                    }
                                    if let Ok(path) = pb.build() {
                                        window.paint_path(path, c);
                                    }
                                }
                                Overlay::Curve(segs, c, w, dashed) => {
                                    let mut pb = PathBuilder::stroke(px(w));
                                    if dashed {
                                        pb = pb.dash_array(&[px(4.), px(4.)]);
                                    }
                                    for s in segs {
                                        match s {
                                            Seg::Move(p) => pb.move_to(point(px(p[0]), px(p[1]))),
                                            Seg::Line(p) => pb.line_to(point(px(p[0]), px(p[1]))),
                                            Seg::Cubic(a, b, p) => pb.cubic_bezier_to(point(px(p[0]), px(p[1])), point(px(a[0]), px(a[1])), point(px(b[0]), px(b[1]))),
                                            Seg::Close => pb.close(),
                                        }
                                    }
                                    if let Ok(path) = pb.build() {
                                        window.paint_path(path, c);
                                    }
                                }
                                Overlay::Dot(p, c, r) => {
                                    let b = Bounds::new(point(p.x - px(r), p.y - px(r)), size(px(r * 2.0), px(r * 2.0)));
                                    window.paint_quad(gpui::fill(b, gpui::white()));
                                    window.paint_quad(gpui::outline(b, c, gpui::BorderStyle::Solid));
                                }
                            }
                        }
                    },
                )
                .absolute()
                .inset_0(),
            )
    }
}

impl CanvasView {
    /// A segment in page pixels → window pixels (as plain numbers).
    fn map_seg(&self, s: Seg) -> Seg {
        let m = |p: [f32; 2]| {
            let q = self.to_window(p[0], p[1]);
            [f32::from(q.x), f32::from(q.y)]
        };
        match s {
            Seg::Move(p) => Seg::Move(m(p)),
            Seg::Line(p) => Seg::Line(m(p)),
            Seg::Cubic(a, b, p) => Seg::Cubic(m(a), m(b), m(p)),
            Seg::Close => Seg::Close,
        }
    }

    /// Where the page's top-left corner is in the window.
    #[cfg(test)]
    pub fn page_origin(&self) -> Point<Pixels> {
        self.to_window(0.0, 0.0)
    }

    /// For tests and `ui.zoom`.
    pub fn set_zoom(&mut self, z: f32, cx: &mut Context<Self>) {
        let c = (f32::from(self.bounds.size.width) / 2.0, f32::from(self.bounds.size.height) / 2.0);
        self.zoom_about(z, c, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nori_core::{Color, Raster};

    #[test]
    fn dirty_follows_pixels_and_settings() {
        let mut a = Document::new("t", 1000, 600, Some(Color::WHITE));
        let id = a.new_id();
        a.insert_above(Layer::new(id.clone(), "x", Content::Raster { x: 0, y: 0, pixels: Raster::transparent(1000, 600) }), None);
        let mut b = a.clone();
        b.layer_mut(&id).unwrap().raster_mut().unwrap().2.set(700, 300, [1, 2, 3, 255]);
        match dirty(&a, &b) {
            Dirty::Rects(r) => assert_eq!(r, vec![Rect::new(512, 256, 256, 256)]),
            Dirty::All => panic!("one tile changed"),
        }
        let mut c = b.clone();
        c.layer_mut(&id).unwrap().opacity = 0.5;
        assert!(matches!(dirty(&b, &c), Dirty::Rects(r) if r.iter().any(|r| r.w == 1000)));
        let mut d = c.clone();
        d.insert_above(Layer::new("L99", "fill", Content::Fill { color: Color::BLACK }), None);
        assert!(matches!(dirty(&c, &d), Dirty::All));
    }

    #[test]
    fn tiles_and_pyramid() {
        let d = Document::new("t", 300, 200, Some(Color::rgb(1.0, 0.0, 0.0)));
        let prep = nori_render::prepare(&d);
        let t = composite_tile(&d, d.page(), &prep, 1, 0);
        // x 256..300 is red, past the page transparent.
        assert_eq!(&t[0..4], &[255, 0, 0, 255]);
        assert_eq!(t[(50 * 4 + 3) as usize], 0);
        let half = downsample(&[Some(Arc::new(t)), None, None, None]);
        assert_eq!(&half[0..4], &[255, 0, 0, 255]);
    }
}
