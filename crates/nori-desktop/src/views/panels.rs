//! The right column: Layers (or Pages) at the top, the active layer's Properties under them,
//! History at the bottom. Every change is a command.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{AnyElement, App, Context, Entity, FontWeight, MouseButton, Render, RenderImage, SharedString, Subscription, Window, div, img, prelude::*, px};
use nori_core::{Adjustment, BlendMode, Content, Document, Layer};
use serde_json::{Value, json};

use crate::store::{Dialog, MenuEntry, MenuItem, RightTab, Store, StoreEvent, StoreExt};
use crate::theme::{ActiveTheme, MONO, size as sz};
use crate::ui::fold::Fold;
use crate::ui::input::{InputEvent, TextInput};
use crate::ui::scrub::{Scrub, ScrubChange};
use crate::ui::{Button, GlassExt, caps, group, icon};

pub struct RightColumn {
    store: Entity<Store>,
    /// Number fields of the properties, by `layer/setting`.
    scrubs: HashMap<String, (Entity<Scrub>, Subscription)>,
    words: Entity<TextInput>,
    words_for: Option<String>,
    name: Option<(String, Entity<TextInput>, Subscription)>,
    thumbs: HashMap<String, (u64, Arc<RenderImage>)>,
    thumb_job: Option<gpui::Task<()>>,
    garbage: Vec<Arc<RenderImage>>,
    show_history: bool,
    _subs: Vec<Subscription>,
}

/// A cheap signature of a layer's look (settings and pixel tiles).
fn signature(l: &Layer) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(l).unwrap_or_default().hash(&mut h);
    if let Some((_, _, p)) = l.raster() {
        let (gx, gy) = p.grid();
        for ty in 0..gy {
            for tx in 0..gx {
                p.tile_arc(tx, ty).map(|a| Arc::as_ptr(a) as usize).hash(&mut h);
            }
        }
    }
    h.finish()
}

impl RightColumn {
    pub fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = cx.store();
        let words = cx.new(|cx| TextInput::new(cx).multiline(3).placeholder("The words"));
        let subs = vec![
            cx.observe(&store, |this: &mut Self, _, cx| {
                this.refresh_thumbs(cx);
                cx.notify()
            }),
            cx.subscribe(&words, |this: &mut Self, input, e: &InputEvent, cx| {
                if let InputEvent::Changed(text) = e
                    && let Some(id) = this.words_for.clone()
                {
                    let _ = input;
                    let text = text.clone();
                    this.store.update(cx, |s, cx| s.run("text.update", json!({ "layerId": id, "text": text, "coalesce": format!("words-{id}") }), cx));
                }
            }),
        ];
        let mut this = Self { store, scrubs: HashMap::new(), words, words_for: None, name: None, thumbs: HashMap::new(), thumb_job: None, garbage: vec![], show_history: true, _subs: subs };
        this.refresh_thumbs(cx);
        this
    }

    /// Pixel layers show a small picture of themselves, made in the background.
    fn refresh_thumbs(&mut self, cx: &mut Context<Self>) {
        let Some(doc) = self.store.read(cx).doc.clone() else { return };
        let mut todo = vec![];
        for l in doc.page().all() {
            if l.raster().is_some() {
                let sig = signature(l);
                if self.thumbs.get(&l.id).is_none_or(|(s, _)| *s != sig) {
                    todo.push((l.id.clone(), sig));
                }
            }
        }
        if todo.is_empty() || self.thumb_job.is_some() {
            return;
        }
        let task = cx.background_spawn(async move {
            // Waits a moment: a stroke or a drag changes the layer many times.
            std::thread::sleep(std::time::Duration::from_millis(250));
            todo.into_iter()
                .filter_map(|(id, sig)| {
                    let (w, h, rgba) = nori_render::composite::layer_image(&doc, &id, 64)?;
                    Some((id, sig, crate::views::canvas::to_image(&rgba, w, h)?))
                })
                .collect::<Vec<_>>()
        });
        self.thumb_job = Some(cx.spawn(async move |this, cx| {
            let out = task.await;
            this.update(cx, |c, cx| {
                for (id, sig, img) in out {
                    if let Some((_, old)) = c.thumbs.insert(id, (sig, img)) {
                        c.garbage.push(old);
                    }
                }
                c.thumb_job = None;
                c.refresh_thumbs(cx);
                cx.notify();
            })
            .ok();
        }));
    }

    /// A number field for a layer setting, made once and kept.
    fn scrub(&mut self, key: String, label: &str, value: f64, step: f64, dec: usize, unit: &str, min: f64, max: f64, cx: &mut Context<Self>, on: impl Fn(f64, &str, &mut App) + 'static) -> Entity<Scrub> {
        if !self.scrubs.contains_key(&key) {
            let e = cx.new(|_| Scrub::new(label.to_string(), step, dec).unit(unit.to_string()).range(min, max));
            let k = key.clone();
            let sub = cx.subscribe(&e, move |_, _, ch: &ScrubChange, cx| on(ch.value, &k, cx));
            self.scrubs.insert(key.clone(), (e, sub));
        }
        let e = self.scrubs[&key].0.clone();
        e.update(cx, |s, _| s.set_value(value));
        e
    }

    // ---- layers ----------------------------------------------------------------------------

    fn layer_row(&mut self, d: &Document, l: &Layer, depth: usize, active: Option<&str>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let t = cx.theme().clone();
        let id = l.id.clone();
        let selected = active == Some(id.as_str());
        let kind_icon = match &l.content {
            Content::Text { text } if text.frame.is_some() => "frame",
            Content::Text { .. } => "type",
            Content::Vector { .. } => "pen-tool",
            Content::Adjustment { .. } => "sliders-horizontal",
            Content::Group { .. } => "folder",
            Content::Fill { .. } => "droplet",
            Content::Raster { .. } => "image",
        };
        let thumb = self.thumbs.get(&id).map(|(_, i)| i.clone());
        let (iid, eid, mid) = (id.clone(), id.clone(), id.clone());
        let visible = l.visible;
        let expanded = matches!(l.content, Content::Group { expanded: true, .. });
        let is_group = l.is_group();
        let renaming = self.name.as_ref().filter(|(n, ..)| *n == id).map(|(_, i, _)| i.clone());
        let fill = match &l.content {
            Content::Fill { color } => Some(*color),
            _ => None,
        };
        let row = div()
            .id(SharedString::from(format!("layer-{id}")))
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(36.))
            .pl(px(6. + depth as f32 * 14.))
            .pr(px(8.))
            .border_b_1()
            .border_color(t.line)
            .cursor_pointer()
            .when(selected, |d| d.bg(t.accent).text_color(t.text_on_accent))
            .when(!selected, |d| d.hover(|s| s.bg(t.hover)))
            .on_click(cx.listener(move |this, e: &gpui::ClickEvent, window, cx| {
                if e.click_count() >= 2 {
                    this.start_rename(&iid, window, cx);
                } else {
                    this.store.update(cx, |s, cx| s.run("layer.select", json!({ "layerId": iid }), cx));
                }
            }))
            .on_mouse_down(MouseButton::Right, cx.listener(move |this, e: &gpui::MouseDownEvent, _, cx| {
                this.store.update(cx, |s, cx| s.run("layer.select", json!({ "layerId": mid }), cx));
                layer_menu(&mid, e.position, cx);
                cx.stop_propagation();
            }))
            .child(
                div()
                    .id(SharedString::from(format!("eye-{id}")))
                    .flex_none()
                    .size(px(22.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .opacity(if visible { 1.0 } else { 0.45 })
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        cx.store().update(cx, |s, cx| s.run("layer.update", json!({ "layerId": eid, "visible": !visible }), cx));
                    })
                    .child(icon(if visible { "eye" } else { "eye-off" })),
            )
            .when(is_group, |d| {
                let gid = id.clone();
                d.child(
                    div()
                        .id(SharedString::from(format!("fold-{id}")))
                        .flex_none()
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            cx.store().update(cx, |s, cx| s.run("layer.update", json!({ "layerId": gid, "expanded": !expanded }), cx));
                        })
                        .child(icon(if expanded { "chevron-down" } else { "chevron-right" })),
                )
            })
            .child(
                div()
                    .flex_none()
                    .size(px(28.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_1()
                    .border_color(if selected { t.text_on_accent.opacity(0.4) } else { t.line_strong })
                    .bg(if thumb.is_some() { gpui::Hsla::from(gpui::rgb(0xcfcfcf)) } else { gpui::transparent_black() })
                    .overflow_hidden()
                    .map(|d| match (thumb, fill) {
                        (Some(img_), _) => d.child(img(img_).size_full().object_fit(gpui::ObjectFit::Contain)),
                        (None, Some(c)) => {
                            let [r, g, b, _] = c.to_u8();
                            d.bg(gpui::Rgba { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: 1.0 })
                        }
                        _ => d.child(icon(kind_icon)),
                    }),
            )
            .child(match renaming {
                Some(input) => div().flex_1().min_w_0().child(input).into_any_element(),
                None => div().flex_1().min_w_0().truncate().text_size(px(sz::SM)).font_weight(if selected { FontWeight::SEMIBOLD } else { FontWeight::NORMAL }).child(l.name.clone()).into_any_element(),
            })
            .when(l.clipped, |d| d.child(icon("corner-down-right")))
            .when(l.mask.is_some(), |d| d.child(div().flex_none().font_family(MONO).text_size(px(9.)).child("MASK")))
            .when(l.locked, |d| d.child(icon("lock")))
            .when(l.blend != BlendMode::Normal && l.blend != BlendMode::PassThrough, |d| d.child(div().flex_none().font_family(MONO).text_size(px(9.)).opacity(0.7).child(l.blend.label().to_uppercase())));
        let mut out = vec![row.into_any_element()];
        if let Content::Group { children, expanded: true } = &l.content {
            for c in children {
                out.extend(self.layer_row(d, c, depth + 1, active, cx));
            }
        }
        out
    }

    fn start_rename(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.store.read(cx).doc.clone() else { return };
        let Some(l) = doc.layer(id) else { return };
        let input = cx.new(|cx| {
            let mut i = TextInput::new(cx);
            i.set_text(l.name.clone(), cx);
            i.select_all_text(cx);
            i
        });
        crate::ui::input::focus(&input, window, cx);
        let lid = id.to_string();
        let sub = cx.subscribe(&input, move |this: &mut Self, input, e: &InputEvent, cx| match e {
            InputEvent::Submit | InputEvent::Blur => {
                let name = input.read(cx).text().trim().to_string();
                if !name.is_empty() {
                    let id = lid.clone();
                    this.store.update(cx, |s, cx| s.run("layer.update", json!({ "layerId": id, "name": name }), cx));
                }
                this.name = None;
                cx.notify();
            }
            InputEvent::Cancel => {
                this.name = None;
                cx.notify();
            }
            _ => {}
        });
        self.name = Some((id.to_string(), input, sub));
        cx.notify();
    }

    fn layers(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let s = self.store.read(cx);
        let Some(doc) = s.doc.clone() else { return div().into_any_element() };
        let active = doc.active_id();
        let al = active.as_ref().and_then(|a| doc.layer(a)).cloned();
        let mut rows = vec![];
        for l in &doc.page().layers {
            rows.extend(self.layer_row(&doc, l, 0, active.as_deref(), cx));
        }
        // The active layer's blend mode and opacity, over the list (as other editors place them).
        let settings = al.as_ref().map(|l| {
            let id = l.id.clone();
            let id2 = id.clone();
            let opacity = self.scrub(format!("{id}/opacity"), "Opacity", (l.opacity * 100.0).round() as f64, 1.0, 0, "%", 0.0, 100.0, cx, move |v, _, cx| {
                cx.store().update(cx, |s, cx| s.run("layer.update", json!({ "layerId": id2, "opacity": v / 100.0, "coalesce": "opacity" }), cx))
            });
            let blend = l.blend;
            let is_group = l.is_group();
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(10.))
                .py(px(8.))
                .border_b_1()
                .border_color(t.line)
                .child(
                    Button::new("blend", blend.label())
                        .small()
                        .icon_after("chevron-down")
                        .on_click(move |e, _, cx| {
                            let entries = BlendMode::ALL
                                .into_iter()
                                .filter(|m| is_group || *m != BlendMode::PassThrough)
                                .map(|m| {
                                    let id = id.clone();
                                    MenuItem::new(m.label(), move |_, cx| cx.store().update(cx, |s, cx| s.run("layer.update", json!({ "layerId": id, "blend": m.id() }), cx))).checked(m == blend).entry()
                                })
                                .collect();
                            cx.store().update(cx, |s, cx| s.open_menu(e.position(), entries, cx));
                        }),
                )
                .child(div().flex_1().child(opacity))
        });
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .children(settings)
            .child(div().id("layer-list").flex_1().min_h_0().overflow_y_scroll().children(rows))
            .child(
                div().flex().items_center().justify_end().gap(px(6.)).px(px(8.)).py(px(6.)).border_t_1().border_color(t.line).child(group(
                    [
                        Button::icon("add-layer", "plus", "New pixel layer").small().flush().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("layer.add", json!({}), cx))).into_any_element(),
                        Button::icon("add-adjust", "sliders-horizontal", "New adjustment layer").small().flush().on_click(|e, _, cx| adjustment_menu(e.position(), cx)).into_any_element(),
                        Button::icon("add-group", "folder", "New group").small().flush().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("layer.add", json!({ "kind": "group" }), cx))).into_any_element(),
                        Button::icon("add-mask", "circle-dot", "Add a mask (from the selection if there is one)")
                            .small()
                            .flush()
                            .disabled(al.is_none())
                            .on_click(|_, _, cx| {
                                let sel = cx.store().read(cx).doc().is_some_and(|d| d.selection.is_some());
                                cx.store().update(cx, |s, cx| s.run("layer.addMask", json!({ "from": if sel { "selection" } else { "reveal" } }), cx));
                            })
                            .into_any_element(),
                        Button::icon("delete-layer", "trash-2", "Delete layer").small().flush().disabled(al.is_none()).on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("layer.delete", json!({}), cx))).into_any_element(),
                    ],
                    cx,
                )),
            )
            .into_any_element()
    }

    // ---- pages -----------------------------------------------------------------------------

    fn pages(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let Some(doc) = self.store.read(cx).doc.clone() else { return div().into_any_element() };
        let row = |p: &nori_core::Page, label: String, active: bool, master: bool, cx: &mut Context<Self>| {
            let pid = p.id.clone();
            let mid = p.id.clone();
            div()
                .id(SharedString::from(format!("page-{}", p.id)))
                .flex()
                .items_center()
                .gap(px(10.))
                .h(px(44.))
                .px(px(10.))
                .border_b_1()
                .border_color(t.line)
                .cursor_pointer()
                .when(active, |d| d.bg(t.accent).text_color(t.text_on_accent))
                .when(!active, |d| d.hover(|s| s.bg(t.hover)))
                .on_click(move |_, _, cx| cx.store().update(cx, |s, cx| s.run("page.select", json!({ "page": pid }), cx)))
                .on_mouse_down(MouseButton::Right, move |e, _, cx| page_menu(&mid, master, e.position, cx))
                .child({
                    // The page's shape, at its proportions.
                    let (w, h) = (p.width as f32, p.height as f32);
                    let k = 26.0 / w.max(h);
                    div().flex_none().w(px(30.)).flex().justify_center().child(div().w(px(w * k)).h(px(h * k)).border_1().border_color(if active { t.text_on_accent } else { t.line_strong }).bg(if active { t.text_on_accent.opacity(0.15) } else { t.bg_raised }))
                })
                .child(div().flex_1().min_w_0().flex().flex_col().child(div().truncate().text_size(px(sz::SM)).font_weight(FontWeight::MEDIUM).child(label)).child(div().font_family(MONO).text_size(px(10.)).opacity(0.7).child(format!("{}×{}{}", p.width, p.height, p.master.as_ref().and_then(|m| doc.masters.iter().find(|x| &x.id == m)).map(|m| format!(" · {}", m.name)).unwrap_or_default()))))
                .into_any_element()
        };
        let mut rows = vec![];
        for (i, p) in doc.pages.iter().enumerate() {
            rows.push(row(p, format!("{}  {}", i + 1, p.name), p.id == doc.active_page, false, cx));
        }
        let mut masters = vec![];
        for p in &doc.masters {
            masters.push(row(p, p.name.clone(), p.id == doc.active_page, true, cx));
        }
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(div().id("page-list").flex_1().min_h_0().overflow_y_scroll().children(rows).when(!masters.is_empty(), |d| d.child(div().px(px(10.)).pt(px(10.)).pb(px(4.)).child(caps("Master pages", cx))).children(masters)))
            .child(
                div().flex().items_center().justify_end().gap(px(6.)).px(px(8.)).py(px(6.)).border_t_1().border_color(t.line).child(group(
                    [
                        Button::new("add-page", "Page").with_icon("plus").small().flush().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("page.add", json!({}), cx))).into_any_element(),
                        Button::new("add-master", "Master").with_icon("layout-template").small().flush().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("page.addMaster", json!({}), cx))).into_any_element(),
                    ],
                    cx,
                )),
            )
            .into_any_element()
    }

    // ---- properties ------------------------------------------------------------------------

    fn properties(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<(String, AnyElement)> {
        let t = cx.theme().clone();
        let s = self.store.read(cx);
        let doc = s.doc.clone()?;
        let id = doc.active_id()?;
        let l = doc.layer(&id)?.clone();
        let mut body: Vec<AnyElement> = vec![];
        let title;
        match &l.content {
            Content::Adjustment { adjustment } => {
                title = adjustment.label().to_string();
                body.extend(self.adjustment_fields(&id, adjustment, cx));
            }
            Content::Text { text } => {
                title = if text.frame.is_some() { "Text frame".into() } else { "Text".into() };
                if self.words_for.as_deref() != Some(&id) || !self.words.read(cx).is_focused(window) {
                    let w = text.text.clone();
                    self.words.update(cx, |i, cx| i.set_text(w, cx));
                    self.words_for = Some(id.clone());
                }
                body.push(div().child(self.words.clone()).into_any_element());
                let i1 = id.clone();
                let size = self.scrub(format!("{id}/size"), "Size", text.size as f64, 1.0, 0, "px", 1.0, 4000.0, cx, move |v, _, cx| cx.store().update(cx, |s, cx| s.run("text.update", json!({ "layerId": i1, "size": v, "coalesce": "text-size" }), cx)));
                let i2 = id.clone();
                let lh = self.scrub(format!("{id}/lh"), "Leading", (text.line_height * 100.0).round() as f64, 1.0, 0, "%", 50.0, 500.0, cx, move |v, _, cx| cx.store().update(cx, |s, cx| s.run("text.update", json!({ "layerId": i2, "lineHeight": v / 100.0, "coalesce": "text-lh" }), cx)));
                let i3 = id.clone();
                let font = text.font.clone();
                body.push(
                    div()
                        .flex()
                        .gap(px(6.))
                        .child(Button::new("text-font", font).small().icon_after("chevron-down").on_click(move |e, _, cx| font_menu(&i3, e.position(), cx)))
                        .child(div().flex_1().child(size))
                        .into_any_element(),
                );
                let i4 = id.clone();
                let i5 = id.clone();
                let weight = text.weight;
                body.push(
                    div()
                        .flex()
                        .gap(px(6.))
                        .items_center()
                        .child(crate::ui::segmented("text-weight", vec![(400u16, "Regular".into()), (600, "Semibold".into()), (800, "Bold".into())], if weight >= 700 { 800 } else if weight >= 550 { 600 } else { 400 }, move |w, _, cx| cx.store().update(cx, |s, cx| s.run("text.update", json!({ "layerId": i4, "weight": w }), cx)), cx).flex_1())
                        .child(crate::views::tools::swatch("text-color", text.color, 24., cx).on_click(move |e, _, cx| text_color_menu(&i5, e.position(), cx)))
                        .into_any_element(),
                );
                let i6 = id.clone();
                body.push(
                    div()
                        .flex()
                        .gap(px(6.))
                        .child(crate::ui::segmented("text-align", vec![(nori_core::TextAlign::Left, "Left".into()), (nori_core::TextAlign::Center, "Centre".into()), (nori_core::TextAlign::Right, "Right".into())], text.align, move |a, _, cx| cx.store().update(cx, |s, cx| s.run("text.update", json!({ "layerId": i6, "align": a }), cx)), cx).flex_1())
                        .child(div().w(px(110.)).child(lh))
                        .into_any_element(),
                );
                // Styles and threads.
                let styles: Vec<String> = doc.styles.paragraph.iter().map(|p| p.name.clone()).collect();
                let i7 = id.clone();
                let cur = text.style.clone().unwrap_or_else(|| "No style".into());
                body.push(
                    div()
                        .flex()
                        .gap(px(6.))
                        .child(Button::new("text-style", format!("Style: {cur}")).small().icon_after("chevron-down").on_click(move |e, _, cx| {
                            let mut entries: Vec<MenuEntry> = styles
                                .iter()
                                .map(|n| {
                                    let (n2, id) = (n.clone(), i7.clone());
                                    MenuItem::new(n.clone(), move |_, cx| cx.store().update(cx, |s, cx| s.run("text.applyStyle", json!({ "style": n2, "layerIds": [id] }), cx))).entry()
                                })
                                .collect();
                            let id = i7.clone();
                            entries.push(MenuEntry::Separator);
                            entries.push(MenuItem::new("New style from this text…", move |_, cx| new_style_from(&id, cx)).icon("plus").entry());
                            cx.store().update(cx, |s, cx| s.open_menu(e.position(), entries, cx));
                        }))
                        .when(text.frame.is_some(), |d| {
                            let i8 = id.clone();
                            d.child(Button::new("text-thread", if text.next.is_some() { "Threaded ↦" } else { "Thread to…" }).small().on_click(move |e, _, cx| thread_menu(&i8, e.position(), cx)))
                        })
                        .into_any_element(),
                );
                if let Some(img) = nori_render::text::render_thread(&doc, &id).into_iter().find(|(i, _)| *i == id).map(|(_, i)| i)
                    && img.overflow
                {
                    body.push(div().flex().gap(px(6.)).items_center().text_size(px(sz::SM)).text_color(t.danger).child(icon("circle-alert")).child("The words don't all fit: enlarge the frame or thread it.").into_any_element());
                }
            }
            Content::Vector { shape } => {
                title = "Shape".into();
                let i1 = id.clone();
                let sw = shape.stroke.as_ref().map(|s| s.width).unwrap_or(0.0);
                let width = self.scrub(format!("{id}/sw"), "Stroke", sw as f64, 0.5, 1, "px", 0.0, 500.0, cx, move |v, _, cx| cx.store().update(cx, |s, cx| s.run("vector.update", json!({ "layerId": i1, "strokeWidth": v, "coalesce": "stroke-width" }), cx)));
                let fill = match &shape.fill {
                    nori_core::Paint::Solid { color } => Some(*color),
                    _ => None,
                };
                let stroke = shape.stroke.as_ref().and_then(|s| match &s.paint {
                    nori_core::Paint::Solid { color } => Some(*color),
                    _ => None,
                });
                let (i2, i3) = (id.clone(), id.clone());
                let paint_cell = |label: &'static str, c: Option<nori_core::Color>, gradient: bool, on: Box<dyn Fn(gpui::Point<gpui::Pixels>, &mut App)>, cx: &App| {
                    let t = cx.theme();
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .text_size(px(sz::SM))
                        .text_color(t.text_2)
                        .child(label)
                        .child(match c {
                            Some(c) => crate::views::tools::swatch(if label == "Fill" { "fill-swatch" } else { "stroke-swatch" }, c, 22., cx).on_click(move |e, _, cx| on(e.position(), cx)).into_any_element(),
                            None => Button::new(if label == "Fill" { "fill-none" } else { "stroke-none" }, if gradient { "Gradient" } else { "None" }).small().on_click(move |e, _, cx| on(e.position(), cx)).into_any_element(),
                        })
                };
                body.push(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(paint_cell("Fill", fill, !matches!(shape.fill, nori_core::Paint::None | nori_core::Paint::Solid { .. }), Box::new(move |at, cx| paint_menu(&i2, "fill", at, cx)), cx))
                        .child(paint_cell("Stroke", stroke, false, Box::new(move |at, cx| paint_menu(&i3, "stroke", at, cx)), cx))
                        .child(div().flex_1().child(width))
                        .into_any_element(),
                );
                if let nori_core::Geometry::Rect { radius, .. } = &shape.geometry {
                    let i4 = id.clone();
                    let r = self.scrub(format!("{id}/radius"), "Corners", *radius as f64, 1.0, 0, "px", 0.0, 10000.0, cx, move |v, _, cx| cx.store().update(cx, |s, cx| s.run("vector.update", json!({ "layerId": i4, "radius": v, "coalesce": "radius" }), cx)));
                    body.push(r.into_any_element());
                }
                let (i5, i6) = (id.clone(), id.clone());
                body.push(
                    div()
                        .flex()
                        .gap(px(6.))
                        .child(Button::new("to-path", "Edit points").small().with_icon("square-mouse-pointer").on_click(move |_, _, cx| {
                            cx.store().update(cx, |s, cx| {
                                s.run("vector.toPath", json!({ "layerId": i5 }), cx);
                                s.set_tool(crate::store::Tool::Direct, cx);
                            })
                        }))
                        .child(Button::new("combine", "Combine with below").small().with_icon("combine").on_click(move |e, _, cx| combine_menu(&i6, e.position(), cx)))
                        .into_any_element(),
                );
            }
            Content::Raster { pixels, x, y } => {
                title = "Pixels".into();
                body.push(div().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_2).child(format!("{}×{} at {x}, {y}", pixels.width(), pixels.height())).into_any_element());
                body.push(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(6.))
                        .child(Button::new("filters", "Filter").small().with_icon("wand").icon_after("chevron-down").on_click(|e, _, cx| filter_menu(e.position(), cx)))
                        .child(Button::new("adjust", "Adjust").small().with_icon("sliders-horizontal").icon_after("chevron-down").on_click(|e, _, cx| destructive_adjust_menu(e.position(), cx)))
                        .into_any_element(),
                );
            }
            Content::Fill { color } => {
                title = "Colour fill".into();
                let i1 = id.clone();
                body.push(div().flex().gap(px(8.)).items_center().child(crate::views::tools::swatch("fill-color", *color, 26., cx).on_click(move |e, _, cx| fill_color_menu(&i1, e.position(), cx))).child(div().font_family(MONO).text_size(px(sz::SM)).child(color.hex())).into_any_element());
            }
            Content::Group { .. } => {
                title = "Group".into();
                let i1 = id.clone();
                let pass = l.blend == BlendMode::PassThrough;
                body.push(crate::ui::switch("pass-through", "Pass through (adjustments inside reach below)", pass, move |v, _, cx| cx.store().update(cx, |s, cx| s.run("layer.update", json!({ "layerId": i1, "blend": if v { "pass-through" } else { "normal" } }), cx)), cx).text_size(px(sz::SM)).into_any_element());
            }
        }
        let _ = window;
        Some((title, div().flex().flex_col().gap(px(10.)).children(body).into_any_element()))
    }

    fn adjustment_fields(&mut self, id: &str, a: &Adjustment, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = vec![];
        let v = serde_json::to_value(a).unwrap_or(Value::Null);
        let ranges: &[(&str, &str, f64, f64, f64, usize)] = match a {
            Adjustment::Levels { .. } => &[("inputBlack", "Black in", 0.0, 253.0, 1.0, 0), ("inputWhite", "White in", 2.0, 255.0, 1.0, 0), ("gamma", "Midtones", 0.1, 9.99, 0.01, 2), ("outputBlack", "Black out", 0.0, 255.0, 1.0, 0), ("outputWhite", "White out", 0.0, 255.0, 1.0, 0)],
            Adjustment::HueSaturation { .. } => &[("hue", "Hue", -180.0, 180.0, 1.0, 0), ("saturation", "Saturation", -100.0, 100.0, 1.0, 0), ("lightness", "Lightness", -100.0, 100.0, 1.0, 0)],
            Adjustment::Exposure { .. } => &[("exposure", "Exposure", -5.0, 5.0, 0.01, 2), ("offset", "Offset", -0.5, 0.5, 0.001, 3), ("gamma", "Gamma", 0.1, 9.99, 0.01, 2)],
            Adjustment::BrightnessContrast { .. } => &[("brightness", "Brightness", -150.0, 150.0, 1.0, 0), ("contrast", "Contrast", -50.0, 100.0, 1.0, 0)],
            Adjustment::Vibrance { .. } => &[("vibrance", "Vibrance", -100.0, 100.0, 1.0, 0), ("saturation", "Saturation", -100.0, 100.0, 1.0, 0)],
            Adjustment::BlackWhite { .. } => &[("red", "Reds", -200.0, 300.0, 1.0, 0), ("green", "Greens", -200.0, 300.0, 1.0, 0), ("blue", "Blues", -200.0, 300.0, 1.0, 0)],
            Adjustment::Threshold { .. } => &[("level", "Level", 1.0, 255.0, 1.0, 0)],
            Adjustment::Posterize { .. } => &[("levels", "Levels", 2.0, 255.0, 1.0, 0)],
            Adjustment::Lut { .. } => &[("amount", "Amount", 0.0, 1.0, 0.01, 2)],
            Adjustment::Invert | Adjustment::Curves { .. } => &[],
        };
        for (key, label, min, max, step, dec) in ranges {
            let lid = id.to_string();
            let k = key.to_string();
            let e = self.scrub(format!("{id}/{key}"), label, v[*key].as_f64().unwrap_or(0.0), *step, *dec, "", *min, *max, cx, move |val, _, cx| {
                cx.store().update(cx, |s, cx| s.run("layer.setAdjustment", json!({ "layerId": lid, "settings": { k.clone(): val }, "coalesce": format!("adj-{k}") }), cx))
            });
            out.push(e.into_any_element());
        }
        if let Adjustment::HueSaturation { colorize, .. } = a {
            let lid = id.to_string();
            out.push(crate::ui::switch("colorize", "Colorize", *colorize, move |on, _, cx| cx.store().update(cx, |s, cx| s.run("layer.setAdjustment", json!({ "layerId": lid, "settings": { "colorize": on } }), cx)), cx).text_size(px(sz::SM)).into_any_element());
        }
        if let Adjustment::Curves { .. } = a {
            // The common curves, one click each.
            let presets: [(&str, Value); 5] = [
                ("Linear", json!([[0, 0], [255, 255]])),
                ("More contrast", json!([[0, 0], [64, 48], [192, 208], [255, 255]])),
                ("Less contrast", json!([[0, 16], [64, 72], [192, 184], [255, 240]])),
                ("Lighter", json!([[0, 0], [128, 160], [255, 255]])),
                ("Darker", json!([[0, 0], [128, 96], [255, 255]])),
            ];
            out.push(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .children(presets.into_iter().map(|(name, pts)| {
                        let lid = id.to_string();
                        Button::new(SharedString::from(format!("curve-{name}")), name).small().on_click(move |_, _, cx| cx.store().update(cx, |s, cx| s.run("layer.setAdjustment", json!({ "layerId": lid, "settings": { "rgb": pts } }), cx)))
                    }))
                    .into_any_element(),
            );
        }
        out
    }

    fn history(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let s = self.store.read(cx);
        let n = s.undo.len();
        let mut rows: Vec<AnyElement> = vec![];
        let row = |i: usize, label: String, by: String, current: bool, future: bool, cx: &App| {
            let t = cx.theme();
            div()
                .id(SharedString::from(format!("history-{i}")))
                .flex()
                .items_center()
                .gap(px(8.))
                .h(px(26.))
                .px(px(12.))
                .text_size(px(sz::SM))
                .cursor_pointer()
                .when(current, |d| d.bg(t.accent).text_color(t.text_on_accent))
                .when(!current, |d| d.hover(|s| s.bg(t.hover)))
                .when(future, |d| d.text_color(t.text_3))
                .on_click(move |_, _, cx| cx.store().update(cx, |s, cx| s.run("history.goTo", json!({ "steps": i }), cx)))
                .child(div().flex_1().truncate().child(label))
                .when(by != "window", |d| d.child(div().font_family(MONO).text_size(px(9.)).child(by.to_uppercase())))
                .into_any_element()
        };
        rows.push(row(0, "Opened".into(), "window".into(), n == 0, false, cx));
        for (i, st) in s.undo.iter().enumerate() {
            rows.push(row(i + 1, crate::app::step_name(&st.label), st.source.clone(), i + 1 == n, false, cx));
        }
        for (k, st) in s.redo.iter().enumerate() {
            rows.push(row(n + k + 1, crate::app::step_name(&st.label), st.source.clone(), false, true, cx));
        }
        let _ = t;
        div().id("history-list").max_h(px(180.)).overflow_y_scroll().children(rows).into_any_element()
    }
}

fn adjustment_menu(at: gpui::Point<gpui::Pixels>, cx: &mut App) {
    let mut entries: Vec<MenuEntry> = Adjustment::KINDS
        .iter()
        .map(|k| {
            let k = *k;
            MenuItem::new(nori_core::layer::label_of(k), move |_, cx| cx.store().update(cx, |s, cx| s.run("layer.addAdjustment", json!({ "kind": k }), cx))).entry()
        })
        .collect();
    entries.push(MenuEntry::Separator);
    entries.push(
        MenuItem::new("Color Lookup from a .cube file…", |_, cx| {
            let rx = cx.prompt_for_paths(gpui::PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Add".into()) });
            let store = cx.store();
            cx.spawn(async move |cx| {
                let Ok(Ok(Some(paths))) = rx.await else { return };
                if let Some(p) = paths.into_iter().next() {
                    store.update(cx, |s, cx| s.run("layer.addLut", json!({ "path": p }), cx));
                }
            })
            .detach();
        })
        .icon("palette")
        .entry(),
    );
    cx.store().update(cx, |s, cx| s.open_menu(at, entries, cx));
}

fn layer_menu(id: &str, at: gpui::Point<gpui::Pixels>, cx: &mut App) {
    let doc = cx.store().read(cx).doc.clone();
    let l = doc.as_ref().and_then(|d| d.layer(id)).cloned();
    let raster = l.as_ref().is_some_and(|l| l.raster().is_some());
    let locked = l.as_ref().is_some_and(|l| l.locked);
    let clipped = l.as_ref().is_some_and(|l| l.clipped);
    let has_mask = l.as_ref().is_some_and(|l| l.mask.is_some());
    let mk = |label: &str, cmd: &'static str, params: Value| {
        MenuItem::new(label.to_string(), move |_, cx| {
            let p = params.clone();
            cx.store().update(cx, |s, cx| s.run(cmd, p, cx))
        })
    };
    let i = id.to_string();
    let mut entries = vec![
        mk("Duplicate", "layer.duplicate", json!({ "layerId": i })).icon("copy").entry(),
        mk("Group", "layer.group", json!({ "layerIds": [i] })).icon("folder").entry(),
        mk("Merge down", "layer.merge", json!({ "layerId": i })).entry(),
        MenuEntry::Separator,
        mk("Move to top", "layer.reorder", json!({ "layerId": i, "to": "top" })).entry(),
        mk("Move up", "layer.reorder", json!({ "layerId": i, "to": "up" })).icon("arrow-up").entry(),
        mk("Move down", "layer.reorder", json!({ "layerId": i, "to": "down" })).icon("arrow-down").entry(),
        mk("Move to bottom", "layer.reorder", json!({ "layerId": i, "to": "bottom" })).entry(),
        MenuEntry::Separator,
        mk(if clipped { "Release clipping" } else { "Clip to the layer below" }, "layer.update", json!({ "layerId": i, "clipped": !clipped })).entry(),
        mk(if locked { "Unlock" } else { "Lock" }, "layer.update", json!({ "layerId": i, "locked": !locked })).icon(if locked { "lock-open" } else { "lock" }).entry(),
    ];
    if !raster {
        entries.push(mk("Rasterize", "layer.rasterize", json!({ "layerId": i })).entry());
    }
    if has_mask {
        entries.push(MenuEntry::Separator);
        entries.push(mk("Invert mask", "layer.mask", json!({ "layerId": i, "action": "invert" })).entry());
        entries.push(mk("Disable mask", "layer.mask", json!({ "layerId": i, "action": "disable" })).entry());
        entries.push(mk("Apply mask", "layer.mask", json!({ "layerId": i, "action": "apply" })).entry());
        entries.push(mk("Delete mask", "layer.mask", json!({ "layerId": i, "action": "delete" })).entry());
    }
    entries.push(MenuEntry::Separator);
    entries.push(mk("Delete", "layer.delete", json!({ "layerIds": [i] })).icon("trash-2").danger().entry());
    cx.store().update(cx, |s, cx| s.open_menu(at, entries, cx));
}

fn page_menu(id: &str, master: bool, at: gpui::Point<gpui::Pixels>, cx: &mut App) {
    let i = id.to_string();
    let masters: Vec<(String, String)> = cx.store().read(cx).doc.as_ref().map(|d| d.masters.iter().map(|m| (m.id.clone(), m.name.clone())).collect()).unwrap_or_default();
    let mut entries = vec![];
    if !master {
        let a = i.clone();
        entries.push(MenuItem::new("Duplicate page", move |_, cx| {
            let a = a.clone();
            cx.store().update(cx, |s, cx| s.run_then("page.select", json!({ "page": a }), cx, |s, _, cx| s.run("page.add", json!({ "duplicate": true }), cx)))
        }).icon("copy").entry());
        for (mid, name) in masters {
            let p = i.clone();
            entries.push(MenuItem::new(format!("Apply {name}"), move |_, cx| cx.store().update(cx, |s, cx| s.run("page.update", json!({ "page": p, "master": mid }), cx))).icon("layout-template").entry());
        }
        let p = i.clone();
        entries.push(MenuItem::new("No master", move |_, cx| cx.store().update(cx, |s, cx| s.run("page.update", json!({ "page": p, "master": "" }), cx))).entry());
        let p = i.clone();
        entries.push(MenuItem::new("Make a master from this page", move |_, cx| cx.store().update(cx, |s, cx| s.run("page.addMaster", json!({ "from": p }), cx))).entry());
    }
    entries.push(MenuEntry::Separator);
    entries.push(MenuItem::new("Delete", move |_, cx| cx.store().update(cx, |s, cx| s.run("page.remove", json!({ "page": i }), cx))).icon("trash-2").danger().entry());
    cx.store().update(cx, |s, cx| s.open_menu(at, entries, cx));
}

fn font_menu(id: &str, at: gpui::Point<gpui::Pixels>, cx: &mut App) {
    let fams: Vec<String> = nori_render::text::families().into_iter().take(80).collect();
    let entries = fams
        .into_iter()
        .map(|f| {
            let (f2, id) = (f.clone(), id.to_string());
            MenuItem::new(f, move |_, cx| cx.store().update(cx, |s, cx| s.run("text.update", json!({ "layerId": id, "font": f2 }), cx))).entry()
        })
        .collect();
    cx.store().update(cx, |s, cx| s.open_menu(at, entries, cx));
}

fn color_entries(f: impl Fn(String) -> Box<dyn Fn(&mut Window, &mut App)> + 'static, cx: &App) -> Vec<MenuEntry> {
    let fg = cx.store().read(cx).colors.foreground.hex();
    let bg = cx.store().read(cx).colors.background.hex();
    let mut out = vec![];
    for (name, hex) in [("Foreground", fg), ("Background", bg), ("Black".into(), "#000000".into()), ("White".into(), "#ffffff".into())].map(|(n, h): (&str, String)| (n.to_string(), h)) {
        let act = f(hex.clone());
        out.push(MenuEntry::Item(MenuItem { label: format!("{name}  {hex}").into(), icon: Some("square"), logo: None, shortcut: None, danger: false, disabled: false, checked: false, action: std::rc::Rc::new(move |w, cx| act(w, cx)) }));
    }
    for sw in cx.store().read(cx).session.swatches.read().iter().take(24) {
        let act = f(sw.color.hex());
        out.push(MenuEntry::Item(MenuItem { label: format!("{}  {}", sw.name, sw.color.hex()).into(), icon: Some("square"), logo: None, shortcut: None, danger: false, disabled: false, checked: false, action: std::rc::Rc::new(move |w, cx| act(w, cx)) }));
    }
    out
}

fn text_color_menu(id: &str, at: gpui::Point<gpui::Pixels>, cx: &mut App) {
    let id = id.to_string();
    let entries = color_entries(move |hex| { let id = id.clone(); Box::new(move |_, cx| { let (id, hex) = (id.clone(), hex.clone()); cx.store().update(cx, |s, cx| s.run("text.update", json!({ "layerId": id, "color": hex }), cx)) }) }, cx);
    cx.store().update(cx, |s, cx| s.open_menu(at, entries, cx));
}

fn fill_color_menu(id: &str, at: gpui::Point<gpui::Pixels>, cx: &mut App) {
    let id = id.to_string();
    let entries = color_entries(move |hex| {
        let id = id.clone();
        Box::new(move |_, cx| {
            let (id, hex) = (id.clone(), hex.clone());
            cx.store().update(cx, |s, cx| s.run("layer.update", json!({ "layerId": id, "color": hex }), cx))
        })
    }, cx);
    cx.store().update(cx, |s, cx| s.open_menu(at, entries, cx));
}

fn paint_menu(id: &str, which: &'static str, at: gpui::Point<gpui::Pixels>, cx: &mut App) {
    let id2 = id.to_string();
    let mut entries = color_entries(move |hex| { let id = id2.clone(); Box::new(move |_, cx| { let (id, hex) = (id.clone(), hex.clone()); cx.store().update(cx, |s, cx| s.run("vector.update", json!({ "layerId": id, which: hex }), cx)) }) }, cx);
    let (c1, c2) = { let s = cx.store().read(cx); (s.colors.foreground.hex(), s.colors.background.hex()) };
    let (i1, i2) = (id.to_string(), id.to_string());
    let bounds = cx.store().read(cx).doc.as_ref().and_then(|d| d.layer(id)).and_then(|l| match &l.content { Content::Vector { shape } => Some(shape.bounds()), _ => None }).unwrap_or_default();
    entries.push(MenuEntry::Separator);
    entries.push(MenuItem::new("Gradient: foreground to background", move |_, cx| {
        let p = json!({ "type": "linear", "x1": bounds.x, "y1": bounds.y, "x2": bounds.right(), "y2": bounds.bottom(), "stops": [c1.clone(), c2.clone()] });
        let id = i1.clone();
        cx.store().update(cx, |s, cx| s.run("vector.update", json!({ "layerId": id, which: p }), cx))
    }).icon("blend").entry());
    entries.push(MenuItem::new("None", move |_, cx| { let id = i2.clone(); cx.store().update(cx, |s, cx| s.run("vector.update", json!({ "layerId": id, which: "none" }), cx)) }).icon("x").entry());
    cx.store().update(cx, |s, cx| s.open_menu(at, entries, cx));
}

fn combine_menu(id: &str, at: gpui::Point<gpui::Pixels>, cx: &mut App) {
    let doc = cx.store().read(cx).doc.clone();
    // The vector layer right under this one.
    let below = doc.as_ref().and_then(|d| {
        let loc = d.path_of(id)?;
        let mut p = loc.path.clone();
        *p.last_mut()? += 1;
        let l = d.at(&nori_core::Loc { page: loc.page, path: p })?;
        matches!(l.content, Content::Vector { .. }).then(|| l.id.clone())
    });
    let Some(below) = below else {
        cx.store().update(cx, |s, cx| s.flash("Put another shape right under this one to combine them.", cx));
        return;
    };
    let entries = ["union", "subtract", "intersect", "exclude"]
        .into_iter()
        .map(|op| {
            let (a, b) = (id.to_string(), below.clone());
            let label = match op {
                "union" => "Unite",
                "subtract" => "Subtract the top from the bottom",
                "intersect" => "Keep where they overlap",
                _ => "Exclude where they overlap",
            };
            MenuItem::new(label, move |_, cx| { let (a, b) = (a.clone(), b.clone()); cx.store().update(cx, |s, cx| s.run("vector.combine", json!({ "layerIds": [a, b], "op": op }), cx)) }).icon(match op { "union" => "squares-unite", "subtract" => "squares-subtract", "intersect" => "squares-intersect", _ => "squares-exclude" }).entry()
        })
        .collect();
    cx.store().update(cx, |s, cx| s.open_menu(at, entries, cx));
}

fn thread_menu(id: &str, at: gpui::Point<gpui::Pixels>, cx: &mut App) {
    let Some(doc) = cx.store().read(cx).doc.clone() else { return };
    let mut entries = vec![];
    for (pi, p) in doc.pages.iter().enumerate() {
        for l in p.all() {
            if l.id != id && matches!(&l.content, Content::Text { text } if text.frame.is_some()) {
                let (a, b) = (id.to_string(), l.id.clone());
                entries.push(MenuItem::new(format!("Page {} · {}", pi + 1, l.name), move |_, cx| { let (a, b) = (a.clone(), b.clone()); cx.store().update(cx, |s, cx| s.run("text.thread", json!({ "from": a, "to": b }), cx)) }).icon("frame").entry());
            }
        }
    }
    if entries.is_empty() {
        entries.push(MenuItem::new("Draw another text frame first (Text Frame tool)", |_, _| {}).disabled(true).entry());
    }
    let a = id.to_string();
    entries.push(MenuEntry::Separator);
    entries.push(MenuItem::new("Stop the words here", move |_, cx| { let a = a.clone(); cx.store().update(cx, |s, cx| s.run("text.unthread", json!({ "layerId": a }), cx)) }).entry());
    cx.store().update(cx, |s, cx| s.open_menu(at, entries, cx));
}

fn new_style_from(id: &str, cx: &mut App) {
    let Some(doc) = cx.store().read(cx).doc.clone() else { return };
    let Some(Layer { content: Content::Text { text }, .. }) = doc.layer(id).cloned() else { return };
    let n = doc.styles.paragraph.len() + 1;
    let name = format!("Style {n}");
    let id = id.to_string();
    let params = json!({ "kind": "paragraph", "name": name, "font": text.font, "size": text.size, "weight": text.weight, "italic": text.italic, "color": text.color, "align": text.align, "lineHeight": text.line_height, "letterSpacing": text.letter_spacing, "paragraphSpacing": text.paragraph_spacing });
    cx.store().update(cx, |s, cx| s.run_then("text.defineStyle", params, cx, move |s, _, cx| s.run("text.applyStyle", json!({ "style": name, "layerIds": [id] }), cx)));
}

pub fn filter_menu(at: gpui::Point<gpui::Pixels>, cx: &mut App) {
    let plugins = cx.store().read(cx).session.plugins.filters();
    let mut entries: Vec<MenuEntry> = nori_render::filters::STOCK
        .iter()
        .map(|f| {
            let id = f.id.to_string();
            MenuItem::new(format!("{}…", f.name), move |_, cx| { let id = id.clone(); cx.store().update(cx, |s, cx| s.open_dialog(Dialog::Filter { id }, cx)) }).entry()
        })
        .collect();
    if !plugins.is_empty() {
        entries.push(MenuEntry::Separator);
        for p in plugins {
            let id = p["id"].as_str().unwrap_or("").to_string();
            entries.push(MenuItem::new(format!("{}…", p["name"].as_str().unwrap_or("")), move |_, cx| { let id = id.clone(); cx.store().update(cx, |s, cx| s.open_dialog(Dialog::Filter { id }, cx)) }).icon("puzzle").entry());
        }
    }
    entries.push(MenuEntry::Separator);
    entries.push(MenuItem::new("Plugins…", |_, cx| cx.store().update(cx, |s, cx| s.open_dialog(Dialog::Plugins { tab: None }, cx))).icon("puzzle").entry());
    cx.store().update(cx, |s, cx| s.open_menu(at, entries, cx));
}

fn destructive_adjust_menu(at: gpui::Point<gpui::Pixels>, cx: &mut App) {
    let presets: [(&str, &str, Value); 6] = [
        ("Invert", "invert", json!({})),
        ("Black & White", "blackWhite", json!({})),
        ("Auto contrast (levels 10–245)", "levels", json!({ "inputBlack": 10, "inputWhite": 245 })),
        ("Brighter", "exposure", json!({ "exposure": 0.5 })),
        ("Darker", "exposure", json!({ "exposure": -0.5 })),
        ("Desaturate", "hueSaturation", json!({ "saturation": -100 })),
    ];
    let entries = presets
        .into_iter()
        .map(|(label, kind, settings)| MenuItem::new(label, move |_, cx| { let st = settings.clone(); cx.store().update(cx, |s, cx| s.run("filter.adjust", json!({ "kind": kind, "settings": st }), cx)) }).entry())
        .collect();
    cx.store().update(cx, |s, cx| s.open_menu(at, entries, cx));
}

impl Render for RightColumn {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        for old in self.garbage.drain(..) {
            let _ = window.drop_image(old);
        }
        let t = cx.theme().clone();
        let tab = self.store.read(cx).right;
        let pages_n = self.store.read(cx).doc.as_ref().map(|d| d.pages.len()).unwrap_or(1);
        let top = match tab {
            RightTab::Layers => self.layers(cx),
            RightTab::Pages => self.pages(cx),
        };
        let props = self.properties(window, cx);
        let history = self.show_history.then(|| self.history(cx));
        let show = self.show_history;
        div()
            .size_full()
            .flex()
            .flex_col()
            .glass(t.glass1)
            .border_0()
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(44.))
                    .px(px(10.))
                    .border_b_1()
                    .border_color(t.line)
                    .child(crate::ui::segmented("right-tab", vec![(RightTab::Layers, "Layers".into()), (RightTab::Pages, if pages_n > 1 { format!("Pages · {pages_n}").into() } else { "Pages".into() })], tab, |v, _, cx| cx.store().update(cx, |s, cx| { s.right = *v; s.sync_ui(cx); cx.notify(); }), cx).flex_1()),
            )
            .child(div().flex_1().min_h(px(160.)).flex().flex_col().child(top))
            .when_some(props, |d, (title, body)| d.child(div().flex_none().max_h(px(360.)).border_t_1().border_color(t.line).child(div().id("props").max_h(px(360.)).overflow_y_scroll().child(Fold::new("props-fold", title, true).child(div().px(px(12.)).pb(px(12.)).child(body))))))
            .child(
                div().flex_none().border_t_1().border_color(t.line).child(
                    Fold::new("history-fold", "History", show)
                        .on_toggle({
                            let e = cx.entity();
                            move |_, cx| e.update(cx, |c, cx| { c.show_history = !c.show_history; cx.notify(); })
                        })
                        .children(history),
                ),
            )
            .on_mouse_down(MouseButton::Left, |_, w, _| {
                let _ = w;
            })
    }
}

#[allow(dead_code)]
fn _event(_: StoreEvent) {}
