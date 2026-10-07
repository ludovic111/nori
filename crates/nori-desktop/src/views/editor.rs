//! The editor: the title bar (history · views · agent · app, Export as the primary action); the
//! Tools column; the Canvas with the tool's options above it; Layers (or Pages), Properties and
//! History on the right; and the Agent panel when it is open.

use gpui::{App, Context, Entity, MouseButton, Render, Subscription, Window, div, prelude::*, px};
use serde_json::json;

use crate::actions::{self as act, tip};
use crate::store::{Dialog, MenuItem, RightTab, Store, StoreEvent, StoreExt};
use crate::theme::{ActiveTheme, MONO, size as sz};
use crate::ui::{Button, GlassExt, group, icon, tool};
use crate::views::{agent_panel::AgentPanel, canvas::CanvasView, panels::RightColumn, tools::{OptionsBar, ToolColumn}};

pub const TOPBAR_H: f32 = 52.;

pub struct Editor {
    store: Entity<Store>,
    pub canvas: Entity<CanvasView>,
    pub tools: Entity<ToolColumn>,
    pub options: Entity<OptionsBar>,
    pub right: Entity<RightColumn>,
    pub agent: Entity<AgentPanel>,
    _subs: Vec<Subscription>,
}

impl Editor {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = cx.store();
        let subs = vec![cx.observe(&store, |_, _, cx| cx.notify())];
        Self {
            canvas: cx.new(CanvasView::new),
            tools: cx.new(|cx| ToolColumn::new(window, cx)),
            options: cx.new(|cx| OptionsBar::new(window, cx)),
            right: cx.new(|cx| RightColumn::new(window, cx)),
            agent: cx.new(|cx| AgentPanel::new(window, cx)),
            store,
            _subs: subs,
        }
    }

    fn top_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = cx.theme().clone();
        let s = self.store.read(cx);
        let width = f32::from(window.viewport_size().width);
        let wide = width >= 1280.;
        let (can_undo, can_redo) = (!s.undo.is_empty(), !s.redo.is_empty());
        let agent_open = s.agent_open;
        let agent_enabled = s.settings.agent.enabled;
        let name: gpui::SharedString = s.doc.as_ref().map(|d| d.name.clone()).unwrap_or_default().into();
        let dirty = s.dirty;
        let zoom = s.zoom;
        let progress = s.progress.first().map(|(_, l)| l.clone());
        let fullscreen = window.is_fullscreen();
        let controls = crate::ui::window_controls(window, cx);
        let canvas = self.canvas.clone();
        let (c1, c2, c3) = (canvas.clone(), canvas.clone(), canvas.clone());
        div()
            .id("top-bar")
            .h(px(TOPBAR_H))
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .pl(px(if fullscreen || !cfg!(target_os = "macos") { 10. } else { 84. }))
            .when(controls.is_none(), |d| d.pr(px(10.)))
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
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .id("home")
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap(px(4.))
                            .px(px(6.))
                            .h(px(30.))
                            .cursor_pointer()
                            .text_color(t.text_2)
                            .hover(|s| s.bg(t.hover).text_color(t.text))
                            .tooltip(|_, cx| crate::ui::tooltip(tip("Close and go home", &act::CloseDocument), cx))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(|_, window, cx| crate::app::close_document(window,cx))
                            .child(icon("chevron-left"))
                            .child(crate::views::home::mark(18., cx)),
                    )
                    .child(div().flex_none().text_color(t.text_3).child("/"))
                    .child(div().min_w(px(40.)).max_w(px(360.)).truncate().font_weight(gpui::FontWeight::SEMIBOLD).child(name))
                    .when(dirty, |d| d.child(div().flex_none().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).child("EDITED")))
                    .when_some(progress, |d, p| d.child(div().flex_none().flex().items_center().gap(px(6.)).text_size(px(sz::SM)).text_color(t.text_2).child(icon("loader-circle")).child(p))),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    // History.
                    .child(group(
                        [
                            Button::icon("undo", "undo-2", tip("Undo", &act::Undo)).small().flush().disabled(!can_undo).on_click(|_, _, cx| cx.store().update(cx, |s, cx| crate::app::undo_redo(s, true, cx))).into_any_element(),
                            Button::icon("redo", "redo-2", tip("Redo", &act::Redo)).small().flush().disabled(!can_redo).on_click(|_, _, cx| cx.store().update(cx, |s, cx| crate::app::undo_redo(s, false, cx))).into_any_element(),
                        ],
                        cx,
                    ))
                    // Views.
                    .child(group(
                        [
                            Button::icon("zoom-out", "zoom-out", tip("Zoom out", &act::ZoomOut)).small().flush().on_click(move |_, _, cx| c1.update(cx, |c, cx| c.set_zoom(c.zoom / 1.25, cx))).into_any_element(),
                            div()
                                .id("zoom-level")
                                .w(px(54.))
                                .h_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .font_family(MONO)
                                .text_size(px(sz::SM))
                                .text_color(t.text_2)
                                .cursor_pointer()
                                .hover(|s| s.bg(t.hover))
                                .tooltip(|_, cx| crate::ui::tooltip(tip("Actual size", &act::ZoomActual), cx))
                                .on_click(move |_, _, cx| c2.update(cx, |c, cx| c.actual_size(cx)))
                                .child(format!("{:.0}%", zoom * 100.0))
                                .into_any_element(),
                            Button::icon("zoom-in", "zoom-in", tip("Zoom in", &act::ZoomIn)).small().flush().on_click(move |_, _, cx| canvas.update(cx, |c, cx| c.set_zoom(c.zoom * 1.25, cx))).into_any_element(),
                            Button::icon("fit", "maximize", tip("Fit on screen", &act::ZoomFit)).small().flush().on_click(move |_, _, cx| c3.update(cx, |c, cx| c.fit(cx))).into_any_element(),
                        ],
                        cx,
                    ))
                    // The agent.
                    .when(agent_enabled, |d| {
                        d.child(group([tool("agent", "bot", "Agent", wide, tip("Agent", &act::ToggleAgent)).selected(agent_open).on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.set_agent_open(!s.agent_open, cx))).into_any_element()], cx))
                    })
                    // The app.
                    .child(group(
                        [
                            Button::icon("plugins", "puzzle", "Plugins").small().flush().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.open_dialog(Dialog::Plugins { tab: None }, cx))).into_any_element(),
                            Button::icon("settings", "settings", tip("Settings", &act::OpenSettings)).small().flush().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.open_dialog(Dialog::Settings { section: None }, cx))).into_any_element(),
                            Button::icon("more", "ellipsis", "More: open, place, save, kimchi, shortcuts…")
                                .small()
                                .flush()
                                .on_click(|e, _, cx| {
                                    let at = e.position();
                                    more_menu(gpui::point(at.x - px(220.), at.y + px(18.)), cx)
                                })
                                .into_any_element(),
                        ],
                        cx,
                    ))
                    .child(div().ml(px(4.)).child(Button::new("export", "Export").primary().with_icon("share-2").tooltip(tip("Export", &act::Export)).on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.open_dialog(Dialog::Export, cx))))),
            )
            .children(controls)
            .into_any_element()
    }
}

/// The title bar's "…" menu: file and app actions that don't need a button of their own.
fn more_menu(at: gpui::Point<gpui::Pixels>, cx: &mut App) {
    let entries = vec![
        MenuItem::new("New…", |_, cx| cx.store().update(cx, |s, cx| s.open_dialog(Dialog::NewDocument, cx))).icon("file-plus").shortcut(act::hint(&act::NewDocument).unwrap_or_default()).entry(),
        MenuItem::new("Open…", |_, cx| crate::app::open_dialog(cx)).icon("folder-open").shortcut(act::hint(&act::OpenDocument).unwrap_or_default()).entry(),
        MenuItem::new("Place a file…", |_, cx| crate::app::place_dialog(cx)).icon("image").shortcut(act::hint(&act::PlaceFile).unwrap_or_default()).entry(),
        crate::store::MenuEntry::Separator,
        MenuItem::new("Save", |_, cx| crate::app::save(false, cx)).icon("download").shortcut(act::hint(&act::Save).unwrap_or_default()).entry(),
        MenuItem::new("Save as…", |_, cx| crate::app::save(true, cx)).shortcut(act::hint(&act::SaveAs).unwrap_or_default()).entry(),
        MenuItem::new("Send to kimchi", |_, cx| {
            cx.store().update(cx, |s, cx| s.run_then("handoff.toKimchi", json!({}), cx, |s, _, cx| s.toast(nori_control::ToastKind::Success, "Sent to kimchi: it's on the timeline.", cx)))
        })
        .logo("kimchi")
        .entry(),
        crate::store::MenuEntry::Separator,
        MenuItem::new("Keyboard shortcuts", |_, cx| cx.store().update(cx, |s, cx| s.open_dialog(Dialog::Shortcuts, cx))).icon("keyboard").shortcut(act::hint(&act::ShowShortcuts).unwrap_or_default()).entry(),
        MenuItem::new("What's new", |_, cx| cx.store().update(cx, |s, cx| s.open_dialog(Dialog::WhatsNew, cx))).icon("sparkles").entry(),
        MenuItem::new("Set up nori…", |_, cx| cx.store().update(cx, |s, cx| s.open_setup(None, cx))).icon("wand").entry(),
        MenuItem::new("Help", |_, cx| cx.open_url(crate::app::HELP_URL)).icon("circle-help").entry(),
    ];
    cx.store().update(cx, |s, cx| s.open_menu(at, entries, cx));
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let s = self.store.read(cx);
        let agent_open = s.agent_open;
        let width = f32::from(window.viewport_size().width);
        let right_w = if width < 1180. { 268. } else { 304. };
        let agent_w = if width < 1300. { 320. } else { 380. };
        let (spec, title) = match &s.doc {
            Some(d) => {
                let p = d.page();
                let unit = if d.dpi > 72.0 { format!(" · {:.0} dpi", d.dpi) } else { String::new() };
                let page = if d.pages.len() > 1 || !d.masters.is_empty() { format!(" · {} of {}", if d.master_index(&d.active_page).is_some() { "master".to_string() } else { (d.active_index() + 1).to_string() }, d.pages.len()) } else { String::new() };
                (format!("{}×{}{unit}{page}", p.width, p.height), if d.master_index(&d.active_page).is_some() { format!("Master · {}", p.name) } else if d.pages.len() > 1 { p.name.clone() } else { "Canvas".to_string() })
            }
            None => (String::new(), "Canvas".into()),
        };
        let _ = StoreEvent::FitView;
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.top_bar(window, cx))
            .child({
                let tabs = self.store.read(cx).session.tabs();
                div().id("document-tabs").flex().flex_none().h(px(34.)).overflow_x_scroll().gap(px(2.)).bg(t.bg).border_b_1().border_color(t.line)
                    .children(tabs.into_iter().map(|tab| {
                        let id = tab["id"].as_u64().unwrap_or(0);
                        let active = tab["active"].as_bool().unwrap_or(false);
                        let name = format!("{}{}", tab["name"].as_str().unwrap_or("Untitled"), if tab["dirty"] == true { " •" } else { "" });
                        div().id(("doc-tab", id)).flex().items_center().px(px(12.)).min_w(px(80.)).max_w(px(240.)).text_size(px(sz::SM)).text_color(if active { t.text } else { t.text_2 })
                            .when(active, |d| d.bg(t.hover)).cursor_pointer().hover(|d| d.bg(t.hover)).child(name)
                            .on_click(move |_, _, cx| cx.store().update(cx, |s, cx| s.run("doc.select", json!({ "id": id }), cx)))
                    }))
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.tools.clone())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .border_l_1()
                            .border_r_1()
                            .border_color(t.line)
                            .child(
                                // The canvas area's title, and the tool's options.
                                div()
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .gap(px(12.))
                                    .h(px(44.))
                                    .px(px(12.))
                                    .glass(t.glass1)
                                    .border_0()
                                    .border_b_1()
                                    .border_color(t.line)
                                    .child(crate::ui::panel_title(title))
                                    .child(div().flex_none().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).child(spec))
                                    .child(div().w(px(1.)).h(px(20.)).bg(t.line))
                                    .child(div().flex_1().min_w_0().child(self.options.clone())),
                            )
                            .child(div().flex_1().min_h_0().child(self.canvas.clone())),
                    )
                    .child(div().flex_none().w(px(right_w)).h_full().child(self.right.clone()))
                    .when(agent_open, |d| d.child(div().flex_none().w(px(agent_w)).h_full().border_l_1().border_color(t.line).child(self.agent.clone()))),
            )
    }
}

#[allow(dead_code)]
fn _tab(_: RightTab) {}
