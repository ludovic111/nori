//! Home: start something new (a picture, a page, a booklet, a poster), open a file, pick up a
//! recent one, and what nori opens from the editors people come from.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{AnyElement, App, Context, Entity, FontWeight, MouseButton, Render, RenderImage, SharedString, Subscription, Window, div, img, prelude::*, px, svg};
use serde_json::json;

use crate::assets::icon_path;
use crate::store::{Dialog, Store, StoreExt};
use crate::theme::{ActiveTheme, MONO, size as sz};
use crate::ui::{Button, GlassExt, caps, icon, logo};

pub fn mark(size: f32, cx: &App) -> AnyElement {
    svg().path(icon_path("mark")).flex_none().size(px(size)).text_color(cx.theme().text).into_any_element()
}

/// The starts on the home screen: (label, detail, icon, `doc.new` parameters).
pub const STARTS: [(&str, &str, &str, &str); 8] = [
    ("Screen", "1920 × 1080", "app-window", r#"{"preset":"screen","name":"Screen"}"#),
    ("Square", "2048 × 2048", "square", r#"{"preset":"square","name":"Square"}"#),
    ("Story", "1080 × 1920", "rectangle-horizontal", r#"{"preset":"story","name":"Story"}"#),
    ("Post", "1080 × 1350", "image", r#"{"preset":"instagram","name":"Post"}"#),
    ("A4 page", "300 dpi", "file-text", r#"{"preset":"a4","name":"Page","margins":150}"#),
    ("Booklet", "A5 · 8 pages", "book-open", r#"{"preset":"a5","pages":8,"name":"Booklet","margins":140,"columns":1}"#),
    ("Poster", "A2 · 300 dpi", "layout-template", r#"{"preset":"poster-a2","name":"Poster","margins":300}"#),
    ("Card", "3.5 × 2 in", "credit-card", r#"{"preset":"card","name":"Card","margins":45}"#),
];

pub struct Home {
    store: Entity<Store>,
    previews: HashMap<String, Option<Arc<RenderImage>>>,
    _sub: Subscription,
}

impl Home {
    pub fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = cx.store();
        let sub = cx.observe(&store, |_, _, cx| cx.notify());
        Self { store, previews: HashMap::new(), _sub: sub }
    }

    /// A recent file's picture: a .nori's own preview, or the picture itself, made small in the
    /// background.
    fn preview(&mut self, path: &str, cx: &mut Context<Self>) -> Option<Arc<RenderImage>> {
        if let Some(p) = self.previews.get(path) {
            return p.clone();
        }
        self.previews.insert(path.to_string(), None);
        let p = path.to_string();
        let task = cx.background_spawn(async move {
            let path = std::path::Path::new(&p);
            let (w, h, rgba) = if path.extension().is_some_and(|e| e == "nori") {
                nori_core::file::read_preview(path)?
            } else {
                let bytes = std::fs::read(path).ok()?;
                if bytes.len() > 80 << 20 {
                    return None;
                }
                let img = image::load_from_memory(&bytes).ok()?.thumbnail(320, 320).to_rgba8();
                (img.width(), img.height(), img.into_raw())
            };
            let (w, h, rgba) = nori_render::transform::fit_rgba(&rgba, w, h, 320);
            Some((p, crate::views::canvas::to_image(&rgba, w, h)?))
        });
        cx.spawn(async move |this, cx| {
            if let Some((p, img)) = task.await {
                this.update(cx, |h, cx| {
                    h.previews.insert(p, Some(img));
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
        None
    }
}

impl Render for Home {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let recent: Vec<String> = self.store.read(cx).settings.recent.iter().filter(|p| std::path::Path::new(p).exists()).take(12).cloned().collect();
        let viewport = f32::from(window.viewport_size().width);
        let main_w = (viewport - 96.).clamp(320., 1120.);
        let controls = crate::ui::window_controls(window, cx);
        let header = div()
            .h(px(52.))
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .pl(px(if cfg!(target_os = "macos") { 84. } else { 16. }))
            .pr(px(10.))
            .window_control_area(gpui::WindowControlArea::Drag)
            .glass(t.glass1)
            .border_0()
            .border_b_1()
            .border_color(t.line)
            .on_mouse_down(MouseButton::Left, |e, window, _| {
                if e.click_count == 2 {
                    window.titlebar_double_click();
                } else {
                    window.start_window_move();
                }
            })
            .child(div().flex().items_center().gap(px(10.)).child(mark(20., cx)).child(div().font_weight(FontWeight::SEMIBOLD).child("nori")).child(div().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).child(format!("{} BETA", env!("CARGO_PKG_VERSION")))))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(Button::icon("home-plugins", "puzzle", "Plugins").small().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.open_dialog(Dialog::Plugins { tab: None }, cx))))
                    .child(Button::icon("home-settings", "settings", "Settings").small().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.open_dialog(Dialog::Settings { section: None }, cx))))
                    .children(controls),
            );
        let starts = div().flex().flex_wrap().gap(px(12.)).children(STARTS.iter().enumerate().map(|(i, (label, detail, ic, params))| {
            let params: serde_json::Value = serde_json::from_str(params).unwrap_or_default();
            div()
                .id(("start", i))
                .w(px(124.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .p(px(12.))
                .glass(t.glass1)
                .cursor_pointer()
                .hover(|s| s.border_color(t.accent))
                .on_click(move |_, _, cx| {
                    let p = params.clone();
                    cx.store().update(cx, |s, cx| s.run("doc.new", p, cx))
                })
                .child(icon(ic).size(px(22.)))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(*label))
                .child(div().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_2).child(*detail))
        }));
        let cards: Vec<AnyElement> = recent
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let preview = self.preview(p, cx);
                let name = crate::app::short(p);
                let path = p.clone();
                div()
                    .id(("recent", i))
                    .w(px(200.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        let p = path.clone();
                        cx.store().update(cx, |s, cx| s.run("doc.open", json!({ "path": p }), cx))
                    })
                    .child(
                        div()
                            .h(px(140.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(t.bg_sunken)
                            .border_1()
                            .border_color(t.line_strong)
                            .overflow_hidden()
                            .map(|d| match preview {
                                Some(img_) => d.child(img(img_).size_full().object_fit(gpui::ObjectFit::Contain)),
                                None => d.child(icon("image").text_color(t.text_3)),
                            }),
                    )
                    .child(div().truncate().text_size(px(sz::SM)).child(name))
                    .into_any_element()
            })
            .collect();
        let apps = div().flex().flex_wrap().gap(px(8.)).children(nori_io::APPS.iter().map(|a| {
            let how: SharedString = a.how.into();
            div()
                .id(SharedString::from(format!("from-{}", a.id)))
                .flex()
                .items_center()
                .gap(px(8.))
                .h(px(34.))
                .px(px(10.))
                .border_1()
                .border_color(t.line)
                .bg(t.bg_raised.opacity(0.6))
                .cursor_pointer()
                .hover(|s| s.border_color(t.line_strong))
                .tooltip(move |_, cx| crate::ui::tooltip(how.clone(), cx))
                .on_click(|_, _, cx| crate::app::open_dialog(cx))
                .child(logo(a.id, px(18.)))
                .child(div().text_size(px(sz::SM)).child(a.name))
                .child(div().font_family(MONO).text_size(px(9.)).text_color(t.text_3).child(a.formats.join(" ").to_uppercase()))
        }));
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div().id("home-scroll").flex_1().min_h_0().overflow_y_scroll().child(
                    div()
                        .w(px(main_w))
                        .mx_auto()
                        .pt(px(48.))
                        .pb(px(64.))
                        .flex()
                        .flex_col()
                        .gap(px(36.))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(8.))
                                .child(div().flex().items_center().gap(px(8.)).font_family(MONO).text_size(px(sz::XS)).text_color(t.text_2).child(div().size(px(6.)).bg(t.text)).child("PICTURES · DRAWINGS · PAGES"))
                                .child(div().text_size(px(40.)).font_weight(FontWeight::SEMIBOLD).child("What are you making?"))
                                .child(div().text_size(px(sz::MD)).text_color(t.text_2).child("Retouch a photo, draw a logo, lay out a booklet — in one app, with one undo history your agent shares.")),
                        )
                        .child(div().flex().flex_col().gap(px(12.)).child(div().flex().items_center().justify_between().child(caps("New", cx)).child(div().flex().gap(px(8.)).child(Button::new("custom", "Custom size…").small().with_icon("plus").on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.open_dialog(Dialog::NewDocument, cx)))).child(Button::new("open", "Open…").small().primary().with_icon("folder-open").on_click(|_, _, cx| crate::app::open_dialog(cx))))).child(starts))
                        .when(!cards.is_empty(), |d| d.child(div().flex().flex_col().gap(px(12.)).child(caps("Recent", cx)).child(div().flex().flex_wrap().gap(px(16.)).children(cards))))
                        .child(div().flex().flex_col().gap(px(12.)).child(caps("Coming from another editor", cx)).child(div().text_size(px(sz::SM)).text_color(t.text_2).child("nori opens their Photoshop documents with layers, OpenRaster from Krita and GIMP, and SVG from vector and layout apps. Hover one to see how.")).child(apps)),
                ),
            )
    }
}
