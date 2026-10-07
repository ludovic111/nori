//! SVG in and out. In: every path, shape and text of the file becomes a vector layer (groups
//! stay groups, pictures become raster layers), through `usvg`, so files from Illustrator,
//! Inkscape, Figma, Affinity Designer, Canva or Scribus come in editable. Out: vector layers as
//! paths with their fills, strokes and gradients, text as outlines, pixels as embedded PNGs.

use std::fmt::Write as _;

use nori_core::vector::{Cap, FillRule, Geometry, Join, Node as VNode, Paint, Seg, Shape, Stop, Stroke, SubPath};
use nori_core::{BlendMode, Color, Content, Document, Layer, Page, Raster};

fn color(c: usvg::Color, opacity: f32) -> Color {
    Color { r: c.red as f32 / 255.0, g: c.green as f32 / 255.0, b: c.blue as f32 / 255.0, a: opacity.clamp(0.0, 1.0) }
}

fn tf(t: usvg::Transform) -> [f32; 6] {
    [t.sx, t.ky, t.kx, t.sy, t.tx, t.ty]
}

fn map(t: &[f32; 6], x: f32, y: f32) -> (f32, f32) {
    (t[0] * x + t[2] * y + t[4], t[1] * x + t[3] * y + t[5])
}

fn paint(p: &usvg::Paint, opacity: f32, abs: &[f32; 6]) -> Paint {
    let stops = |s: &[usvg::Stop]| -> Vec<Stop> { s.iter().map(|st| Stop { offset: st.offset().get(), color: color(st.color(), st.opacity().get() * opacity) }).collect() };
    match p {
        usvg::Paint::Color(c) => Paint::Solid { color: color(*c, opacity) },
        usvg::Paint::LinearGradient(g) => {
            let m = mul(abs, &tf(g.transform()));
            let (x1, y1) = map(&m, g.x1(), g.y1());
            let (x2, y2) = map(&m, g.x2(), g.y2());
            Paint::Linear { x1, y1, x2, y2, stops: stops(g.stops()) }
        }
        usvg::Paint::RadialGradient(g) => {
            let m = mul(abs, &tf(g.transform()));
            let (cx, cy) = map(&m, g.cx(), g.cy());
            let k = (m[0] * m[3] - m[1] * m[2]).abs().sqrt();
            Paint::Radial { cx, cy, r: g.r().get() * k, stops: stops(g.stops()) }
        }
        // Patterns aren't vector paints in nori: their first colour stands in.
        usvg::Paint::Pattern(_) => Paint::Solid { color: Color::rgb(0.5, 0.5, 0.5).with_alpha(opacity) },
    }
}

/// `a` after `b` (apply `b` first).
fn mul(a: &[f32; 6], b: &[f32; 6]) -> [f32; 6] {
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4],
        a[1] * b[4] + a[3] * b[5] + a[5],
    ]
}

fn geometry(data: &usvg::tiny_skia_path::Path) -> Geometry {
    use usvg::tiny_skia_path::PathSegment as S;
    let mut subpaths: Vec<SubPath> = vec![];
    let mut last = (0.0f32, 0.0f32);
    for seg in data.segments() {
        match seg {
            S::MoveTo(p) => {
                subpaths.push(SubPath { nodes: vec![VNode::corner(p.x, p.y)], closed: false });
                last = (p.x, p.y);
            }
            S::LineTo(p) => {
                if let Some(sp) = subpaths.last_mut() {
                    sp.nodes.push(VNode::corner(p.x, p.y));
                }
                last = (p.x, p.y);
            }
            S::QuadTo(c, p) => {
                // As a cubic: control points two thirds of the way to the quadratic's.
                let c1 = [last.0 + 2.0 / 3.0 * (c.x - last.0), last.1 + 2.0 / 3.0 * (c.y - last.1)];
                let c2 = [p.x + 2.0 / 3.0 * (c.x - p.x), p.y + 2.0 / 3.0 * (c.y - p.y)];
                if let Some(sp) = subpaths.last_mut() {
                    if let Some(n) = sp.nodes.last_mut() {
                        n.handle_out = Some(c1);
                    }
                    sp.nodes.push(VNode { x: p.x, y: p.y, handle_in: Some(c2), handle_out: None });
                }
                last = (p.x, p.y);
            }
            S::CubicTo(c1, c2, p) => {
                if let Some(sp) = subpaths.last_mut() {
                    if let Some(n) = sp.nodes.last_mut() {
                        n.handle_out = Some([c1.x, c1.y]);
                    }
                    sp.nodes.push(VNode { x: p.x, y: p.y, handle_in: Some([c2.x, c2.y]), handle_out: None });
                }
                last = (p.x, p.y);
            }
            S::Close => {
                if let Some(sp) = subpaths.last_mut() {
                    sp.closed = true;
                    if sp.nodes.len() > 1 {
                        let (f, l) = (sp.nodes[0], *sp.nodes.last().expect("not empty"));
                        if (f.x - l.x).abs() < 1e-3 && (f.y - l.y).abs() < 1e-3 {
                            let end = sp.nodes.pop().expect("not empty");
                            sp.nodes[0].handle_in = end.handle_in;
                        }
                    }
                    last = (sp.nodes[0].x, sp.nodes[0].y);
                }
            }
        }
    }
    Geometry::Path { subpaths }
}

fn blend(b: usvg::BlendMode) -> BlendMode {
    use usvg::BlendMode as B;
    match b {
        B::Normal => BlendMode::Normal,
        B::Multiply => BlendMode::Multiply,
        B::Screen => BlendMode::Screen,
        B::Overlay => BlendMode::Overlay,
        B::Darken => BlendMode::Darken,
        B::Lighten => BlendMode::Lighten,
        B::ColorDodge => BlendMode::ColorDodge,
        B::ColorBurn => BlendMode::ColorBurn,
        B::HardLight => BlendMode::HardLight,
        B::SoftLight => BlendMode::SoftLight,
        B::Difference => BlendMode::Difference,
        B::Exclusion => BlendMode::Exclusion,
        B::Hue => BlendMode::Hue,
        B::Saturation => BlendMode::Saturation,
        B::Color => BlendMode::Color,
        B::Luminosity => BlendMode::Luminosity,
    }
}

fn convert(doc: &mut Document, g: &usvg::Group, out: &mut Vec<Layer>) {
    for node in g.children() {
        match node {
            usvg::Node::Group(child) => {
                let mut kids = vec![];
                convert(doc, child, &mut kids);
                if kids.is_empty() {
                    continue;
                }
                let plain = child.opacity().get() >= 1.0 && child.blend_mode() == usvg::BlendMode::Normal;
                // Groups that only carry a transform add nothing: their children go up a level.
                if plain && child.id().is_empty() && kids.len() == 1 {
                    out.extend(kids);
                    continue;
                }
                let id = doc.new_id();
                let name = if child.id().is_empty() { doc.new_name("Group") } else { child.id().to_string() };
                let mut l = Layer::new(id, name, Content::Group { children: kids, expanded: false });
                l.opacity = child.opacity().get();
                l.blend = if plain { BlendMode::PassThrough } else { blend(child.blend_mode()) };
                out.push(l);
            }
            usvg::Node::Path(p) => {
                if !p.is_visible() {
                    continue;
                }
                let abs = tf(p.abs_transform());
                let fill = p.fill().map(|f| paint(f.paint(), f.opacity().get(), &abs)).unwrap_or(Paint::None);
                let stroke = p.stroke().map(|s| Stroke {
                    paint: paint(s.paint(), s.opacity().get(), &abs),
                    width: s.width().get() * (abs[0] * abs[3] - abs[1] * abs[2]).abs().sqrt(),
                    cap: match s.linecap() {
                        usvg::LineCap::Butt => Cap::Butt,
                        usvg::LineCap::Round => Cap::Round,
                        usvg::LineCap::Square => Cap::Square,
                    },
                    join: match s.linejoin() {
                        usvg::LineJoin::Round => Join::Round,
                        usvg::LineJoin::Bevel => Join::Bevel,
                        _ => Join::Miter,
                    },
                    miter_limit: s.miterlimit().get(),
                    dash: s.dasharray().map(|d| d.to_vec()).unwrap_or_default(),
                });
                let mut shape = Shape::new(geometry(p.data()), fill, stroke);
                shape.transform = abs;
                shape.fill_rule = match p.fill().map(|f| f.rule()) {
                    Some(usvg::FillRule::EvenOdd) => FillRule::EvenOdd,
                    _ => FillRule::NonZero,
                };
                let id = doc.new_id();
                let name = if p.id().is_empty() { doc.new_name("Path") } else { p.id().to_string() };
                out.push(Layer::new(id, name, Content::Vector { shape }));
            }
            usvg::Node::Text(t) => {
                let mut kids = vec![];
                convert(doc, t.flattened(), &mut kids);
                let id = doc.new_id();
                let name = if t.id().is_empty() { doc.new_name("Text") } else { t.id().to_string() };
                out.push(Layer::new(id, name, Content::Group { children: kids, expanded: false }));
            }
            usvg::Node::Image(img) => {
                let data = match img.kind() {
                    usvg::ImageKind::PNG(d) | usvg::ImageKind::JPEG(d) | usvg::ImageKind::GIF(d) | usvg::ImageKind::WEBP(d) => d.clone(),
                    usvg::ImageKind::SVG(_) => continue,
                };
                let Ok(pic) = image::load_from_memory(&data) else { continue };
                let pic = pic.to_rgba8();
                let b = img.abs_bounding_box();
                let (w, h) = ((b.width().round() as u32).max(1), (b.height().round() as u32).max(1));
                let rgba = nori_render::transform::resample_rgba(pic.as_raw(), pic.width(), pic.height(), w, h, nori_render::transform::Filter::Bicubic);
                let id = doc.new_id();
                let name = if img.id().is_empty() { doc.new_name("Picture") } else { img.id().to_string() };
                out.push(Layer::new(id, name, Content::Raster { x: b.x().round() as i32, y: b.y().round() as i32, pixels: Raster::from_rgba(w, h, &rgba) }));
            }
        }
    }
}

/// Reads an SVG into a one-page document of vector layers.
pub fn read(bytes: &[u8], name: &str) -> Result<Document, String> {
    let mut opt = usvg::Options::default();
    opt.fontdb_mut().load_system_fonts();
    let tree = usvg::Tree::from_data(bytes, &opt).map_err(|e| format!("not an SVG nori can read: {e}"))?;
    let size = tree.size();
    let (w, h) = ((size.width().ceil() as u32).clamp(1, nori_core::MAX_SIDE), (size.height().ceil() as u32).clamp(1, nori_core::MAX_SIDE));
    let mut doc = Document::empty(name, w, h);
    let mut layers = vec![];
    convert(&mut doc, tree.root(), &mut layers);
    // SVG draws in order; layers list the top first.
    layers.reverse();
    fn reverse_groups(list: &mut [Layer]) {
        for l in list {
            if let Some(c) = l.children_mut() {
                c.reverse();
                reverse_groups(c);
            }
        }
    }
    reverse_groups(&mut layers);
    doc.pages[0].layers = layers;
    doc.active = doc.pages[0].layers.first().map(|l| l.id.clone());
    Ok(doc)
}

/// Places an SVG's layers into `doc` as a group on the active page (`place`).
pub fn place(doc: &mut Document, bytes: &[u8], name: &str) -> Result<String, String> {
    let other = read(bytes, name)?;
    let mut layers = other.pages.into_iter().next().map(|p| p.layers).unwrap_or_default();
    // New ids in this document.
    fn renumber(doc: &mut Document, list: &mut [Layer]) {
        for l in list {
            l.id = doc.new_id();
            if let Some(c) = l.children_mut() {
                renumber(doc, c);
            }
        }
    }
    renumber(doc, &mut layers);
    let id = doc.new_id();
    let mut g = Layer::new(id.clone(), name, Content::Group { children: layers, expanded: false });
    g.blend = BlendMode::PassThrough;
    let above = doc.active_id();
    doc.insert_above(g, above.as_deref());
    Ok(id)
}

// ---- writing ----------------------------------------------------------------------------------

fn hex(c: &Color) -> String {
    let [r, g, b, _] = c.to_u8();
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn d_of(segs: &[Seg]) -> String {
    let mut d = String::new();
    for s in segs {
        let _ = match *s {
            Seg::Move(p) => write!(d, "M{:.2} {:.2}", p[0], p[1]),
            Seg::Line(p) => write!(d, "L{:.2} {:.2}", p[0], p[1]),
            Seg::Cubic(a, b, p) => write!(d, "C{:.2} {:.2} {:.2} {:.2} {:.2} {:.2}", a[0], a[1], b[0], b[1], p[0], p[1]),
            Seg::Close => write!(d, "Z"),
        };
    }
    d
}

fn skia_d(p: &tiny_skia::Path) -> String {
    use tiny_skia::PathSegment as S;
    let mut d = String::new();
    for s in p.segments() {
        let _ = match s {
            S::MoveTo(p) => write!(d, "M{:.2} {:.2}", p.x, p.y),
            S::LineTo(p) => write!(d, "L{:.2} {:.2}", p.x, p.y),
            S::QuadTo(c, p) => write!(d, "Q{:.2} {:.2} {:.2} {:.2}", c.x, c.y, p.x, p.y),
            S::CubicTo(a, b, p) => write!(d, "C{:.2} {:.2} {:.2} {:.2} {:.2} {:.2}", a.x, a.y, b.x, b.y, p.x, p.y),
            S::Close => write!(d, "Z"),
        };
    }
    d
}

struct Writer {
    defs: String,
    body: String,
    next: usize,
}

impl Writer {
    fn paint(&mut self, p: &Paint) -> (String, f32) {
        let stops = |s: &[Stop]| s.iter().map(|st| format!("<stop offset=\"{:.4}\" stop-color=\"{}\" stop-opacity=\"{:.3}\"/>", st.offset, hex(&st.color), st.color.a)).collect::<String>();
        match p {
            Paint::None => ("none".into(), 1.0),
            Paint::Solid { color } => (hex(color), color.a),
            Paint::Linear { x1, y1, x2, y2, stops: s } => {
                self.next += 1;
                let id = format!("g{}", self.next);
                let _ = write!(self.defs, "<linearGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" x1=\"{x1}\" y1=\"{y1}\" x2=\"{x2}\" y2=\"{y2}\">{}</linearGradient>", stops(s));
                (format!("url(#{id})"), 1.0)
            }
            Paint::Radial { cx, cy, r, stops: s } => {
                self.next += 1;
                let id = format!("g{}", self.next);
                let _ = write!(self.defs, "<radialGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\">{}</radialGradient>", stops(s));
                (format!("url(#{id})"), 1.0)
            }
        }
    }

    fn style(l: &Layer) -> String {
        let mut s = String::new();
        if l.opacity < 1.0 {
            let _ = write!(s, " opacity=\"{:.3}\"", l.opacity);
        }
        if !matches!(l.blend, BlendMode::Normal | BlendMode::PassThrough) {
            let _ = write!(s, " style=\"mix-blend-mode:{}\"", l.blend.id());
        }
        if !l.visible {
            s.push_str(" display=\"none\"");
        }
        s
    }

    fn layers(&mut self, doc: &Document, page: &Page, list: &[Layer]) -> Result<(), String> {
        for l in list.iter().rev() {
            let attrs = Self::style(l);
            let name = esc(&l.name);
            match &l.content {
                Content::Group { children, .. } => {
                    let _ = write!(self.body, "<g id=\"{}\" data-name=\"{name}\"{attrs}>", l.id);
                    self.layers(doc, page, children)?;
                    self.body.push_str("</g>");
                }
                Content::Vector { shape } => {
                    let (fill, fo) = self.paint(&shape.fill);
                    let mut a = format!("fill=\"{fill}\"");
                    if fo < 1.0 {
                        let _ = write!(a, " fill-opacity=\"{fo:.3}\"");
                    }
                    if shape.fill_rule == FillRule::EvenOdd {
                        a.push_str(" fill-rule=\"evenodd\"");
                    }
                    if let Some(st) = &shape.stroke {
                        let (sp, so) = self.paint(&st.paint);
                        let _ = write!(a, " stroke=\"{sp}\" stroke-width=\"{}\"", st.width);
                        if so < 1.0 {
                            let _ = write!(a, " stroke-opacity=\"{so:.3}\"");
                        }
                        let _ = write!(a, " stroke-linecap=\"{}\" stroke-linejoin=\"{}\"", match st.cap { Cap::Butt => "butt", Cap::Round => "round", Cap::Square => "square" }, match st.join { Join::Miter => "miter", Join::Round => "round", Join::Bevel => "bevel" });
                        if !st.dash.is_empty() {
                            let _ = write!(a, " stroke-dasharray=\"{}\"", st.dash.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" "));
                        }
                    }
                    let _ = write!(self.body, "<path id=\"{}\" data-name=\"{name}\" d=\"{}\" {a}{attrs}/>", l.id, d_of(&shape.transformed()));
                }
                Content::Text { text } => {
                    let _ = write!(self.body, "<g id=\"{}\" data-name=\"{name}\" aria-label=\"{}\"{attrs}>", l.id, esc(&text.text));
                    for (c, p) in nori_render::text::outlines_on(doc, &l.id, Some(doc.page_number(&page.id))) {
                        let _ = write!(self.body, "<path d=\"{}\" fill=\"{}\"{}/>", skia_d(&p), hex(&c), if c.a < 1.0 { format!(" fill-opacity=\"{:.3}\"", c.a) } else { String::new() });
                    }
                    self.body.push_str("</g>");
                }
                Content::Fill { color } => {
                    let _ = write!(self.body, "<rect id=\"{}\" data-name=\"{name}\" width=\"{}\" height=\"{}\" fill=\"{}\" fill-opacity=\"{:.3}\"{attrs}/>", l.id, page.width, page.height, hex(color), color.a);
                }
                Content::Raster { x, y, pixels } => {
                    self.image(&l.id, &name, *x, *y, pixels, &attrs)?;
                }
                Content::Adjustment { .. } => {
                    // Not expressible in SVG: [`write`] flattens what is under an adjustment.
                }
            }
        }
        Ok(())
    }

    fn image(&mut self, id: &str, name: &str, x: i32, y: i32, pixels: &Raster, attrs: &str) -> Result<(), String> {
        use base64::Engine;
        let png = nori_core::file::encode_png_rgba(pixels.width(), pixels.height(), &pixels.to_vec())?;
        let _ = write!(
            self.body,
            "<image id=\"{id}\" data-name=\"{name}\" x=\"{x}\" y=\"{y}\" width=\"{}\" height=\"{}\" href=\"data:image/png;base64,{}\"{attrs}/>",
            pixels.width(),
            pixels.height(),
            base64::engine::general_purpose::STANDARD.encode(png)
        );
        Ok(())
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The active page (or `page`) as SVG. What SVG can't say (adjustment layers, and the layers
/// under them) is drawn into one picture; everything above stays vectors and text.
pub fn write(doc: &Document, page: Option<&Page>) -> Result<String, String> {
    let page = page.unwrap_or_else(|| doc.page());
    let mut w = Writer { defs: String::new(), body: String::new(), next: 0 };
    let (flat, above) = crate::plan::split(doc, page);
    if let Some((pixels, x, y)) = flat {
        w.image("flattened", "Flattened", x, y, &pixels, "")?;
    }
    w.layers(doc, page, &above)?;
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">\n",
        w = page.width,
        h = page.height
    );
    if !w.defs.is_empty() {
        let _ = writeln!(out, "<defs>{}</defs>", w.defs);
    }
    out.push_str(&w.body);
    out.push_str("\n</svg>\n");
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    #[test]
    fn reads_paths_groups_and_gradients() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
            <defs><linearGradient id="a"><stop offset="0" stop-color="#000"/><stop offset="1" stop-color="#fff"/></linearGradient></defs>
            <g id="logo" opacity="0.5"><rect id="box" x="10" y="10" width="80" height="40" fill="url(#a)"/><circle cx="150" cy="50" r="30" fill="#ff0000" stroke="#000" stroke-width="4"/></g>
        </svg>"##;
        let d = read(svg, "t").unwrap();
        assert_eq!((d.width(), d.height()), (200, 100));
        let g = &d.page().layers[0];
        assert_eq!(g.name, "logo");
        assert!((g.opacity - 0.5).abs() < 1e-3);
        // Top first: the circle was drawn last.
        let kids = g.children();
        assert_eq!(kids.len(), 2);
        assert_eq!(kids[1].name, "box");
        let Content::Vector { shape } = &kids[0].content else { panic!("not a vector") };
        assert!(matches!(shape.fill, Paint::Solid { color } if color == Color::rgb(1.0, 0.0, 0.0)));
        assert!(shape.stroke.is_some());
        let Content::Vector { shape } = &kids[1].content else { panic!() };
        assert!(matches!(shape.fill, Paint::Linear { .. }));
        // And it draws.
        let px = nori_render::flatten(&d);
        let at = |x: u32, y: u32| &px[((y * 200 + x) * 4) as usize..((y * 200 + x) * 4 + 4) as usize];
        assert!(at(150, 50)[0] > 100 && at(150, 50)[3] > 100);
        assert_eq!(at(5, 5)[3], 0);
    }

    #[test]
    fn writes_what_it_reads() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><path id="p" d="M10 10 L50 10 L30 50 Z" fill="#00ff00"/></svg>"##;
        let d = read(svg, "t").unwrap();
        let out = write(&d, None).unwrap();
        assert!(out.contains("<path id="), "{out}");
        assert!(out.contains("fill=\"#00ff00\""));
        let again = read(out.as_bytes(), "t").unwrap();
        assert_eq!(again.page().layers.len(), 1);
    }
}

/// Converts already transformed path segments into editable IDML control points.
pub(crate) fn geometry_for_idml(segs: &[Seg]) -> Vec<SubPath> {
    let mut paths: Vec<SubPath> = vec![];
    for seg in segs { match *seg {
        Seg::Move(p) => paths.push(SubPath { nodes: vec![VNode::corner(p[0],p[1])], closed:false }),
        Seg::Line(p) => if let Some(sp)=paths.last_mut() {sp.nodes.push(VNode::corner(p[0],p[1]));},
        Seg::Cubic(a,b,p) => if let Some(sp)=paths.last_mut() {if let Some(last)=sp.nodes.last_mut() {last.handle_out=Some(a);}sp.nodes.push(VNode {x:p[0],y:p[1],handle_in:Some(b),handle_out:None});},
        Seg::Close => if let Some(sp)=paths.last_mut() {sp.closed=true;},
    }} paths
}
