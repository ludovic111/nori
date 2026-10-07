//! The Tools column (boxed groups by kind, the colours under them) and the options bar of the
//! tool in hand.

use gpui::{AnyElement, App, Context, Entity, MouseButton, Render, Subscription, Window, div, prelude::*, px};
use serde_json::json;

use crate::store::{MenuItem, ShapeKind, Store, StoreExt, Tool};
use crate::theme::{ActiveTheme, MONO, size as sz};
use crate::ui::input::{InputEvent, TextInput};
use crate::ui::scrub::{Scrub, ScrubChange};
use crate::ui::{Button, caps, group, icon};

pub struct ToolColumn {
    store: Entity<Store>,
    _sub: Subscription,
}

/// The column's groups: what goes together is boxed together.
const GROUPS: &[(&str, &[Tool])] = &[
    ("Select", &[Tool::Move, Tool::Select, Tool::Lasso, Tool::Wand, Tool::Crop]),
    ("Paint", &[Tool::Brush, Tool::Eraser, Tool::Fill, Tool::Gradient, Tool::Eyedropper]),
    ("Draw", &[Tool::Pen, Tool::Direct, Tool::Shape]),
    ("Type", &[Tool::Text, Tool::Frame]),
    ("View", &[Tool::Hand, Tool::Zoom]),
];

impl ToolColumn {
    pub fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = cx.store();
        let sub = cx.observe(&store, |_, _, cx| cx.notify());
        Self { store, _sub: sub }
    }
}

/// A colour swatch; clicking it opens the colour menu for `which` (foreground, background).
pub fn swatch(id: &'static str, c: nori_core::Color, size: f32, cx: &App) -> gpui::Stateful<gpui::Div> {
    let t = cx.theme();
    let [r, g, b, _] = c.to_u8();
    div().id(id).size(px(size)).border_1().border_color(t.line_strong).bg(gpui::Rgba { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: 1.0 }).cursor_pointer()
}

/// The colour menu: black, white, greys, a row of hues, the imported swatches, and the hex field
/// in the options bar.
pub fn color_menu(which: &'static str, at: gpui::Point<gpui::Pixels>, cx: &mut App) {
    let mut entries = vec![];
    let preset: [(&str, &str); 14] = [
        ("Black", "#000000"),
        ("White", "#ffffff"),
        ("Grey 25 %", "#bfbfbf"),
        ("Grey 50 %", "#808080"),
        ("Grey 75 %", "#404040"),
        ("Red", "#e5322d"),
        ("Orange", "#f28c28"),
        ("Yellow", "#f5d32c"),
        ("Green", "#2fa04f"),
        ("Teal", "#1f9e9e"),
        ("Blue", "#2f6fd6"),
        ("Violet", "#7b4ae0"),
        ("Pink", "#e54b9b"),
        ("Brown", "#7a4f2a"),
    ];
    let swatches = cx.store().read(cx).session.swatches.read().clone();
    for (name, hex) in preset.iter().map(|(n, h)| (n.to_string(), h.to_string())).chain(swatches.iter().map(|s| (s.name.clone(), s.color.hex()))) {
        let hex2 = hex.clone();
        entries.push(MenuItem::new(format!("{name}  {hex}"), move |_, cx| cx.store().update(cx, |s, cx| s.run("color.set", json!({ which: hex2 }), cx))).icon("square").entry());
    }
    cx.store().update(cx, |s, cx| s.open_menu(at, entries, cx));
}

impl Render for ToolColumn {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let s = self.store.read(cx);
        let current = s.tool;
        let colors = s.colors;
        div()
            .id("tools")
            .w(px(64.))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(10.))
            .py(px(10.))
            .glass(t.glass1)
            .border_0()
            .overflow_y_scroll()
            .child(div().font_family(MONO).text_size(px(10.)).text_color(t.text_2).child("TOOLS"))
            .children(GROUPS.iter().map(|(name, tools)| {
                let name = *name;
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .border_1()
                    .border_color(t.line_strong)
                    .bg(t.bg_sunken.opacity(0.35))
                    .children(tools.iter().enumerate().map(|(i, tool)| {
                        let tool = *tool;
                        // Rectangle and ellipse share a slot, as the M key switches them.
                        let shown = if tool == Tool::Select && current == Tool::EllipseSelect { Tool::EllipseSelect } else { tool };
                        let selected = current == shown;
                        let key = shown.key().to_uppercase().replace("SHIFT-", "⇧");
                        div()
                            .when(i > 0, |d| d.border_t_1().border_color(t.line))
                            .child(
                                Button::icon(gpui::SharedString::from(format!("tool-{}", shown.id())), shown.icon(), format!("{} ({key})", shown.label()))
                                    .selected(selected)
                                    .on_click(move |_, _, cx| cx.store().update(cx, |s, cx| s.set_tool(shown, cx))),
                            )
                    }))
                    .id(gpui::SharedString::from(format!("group-{name}")))
            }))
            .child(div().flex_1())
            // Foreground over background, swap and reset.
            .child(
                div()
                    .relative()
                    .w(px(44.))
                    .h(px(44.))
                    .child(
                        swatch("bg-color", colors.background, 26., cx)
                            .absolute()
                            .right_0()
                            .bottom_0()
                            .tooltip(|_, cx| crate::ui::tooltip("Background colour".into(), cx))
                            .on_click(|e, _, cx| color_menu("background", e.position(), cx)),
                    )
                    .child(
                        swatch("fg-color", colors.foreground, 26., cx)
                            .absolute()
                            .left_0()
                            .top_0()
                            .tooltip(|_, cx| crate::ui::tooltip("Foreground colour".into(), cx))
                            .on_click(|e, _, cx| color_menu("foreground", e.position(), cx)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(px(2.))
                    .child(Button::icon("swap-colors", "rotate-cw", "Swap colours (X)").small().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("color.set", json!({ "swap": true }), cx))))
                    .child(Button::icon("reset-colors", "contrast", "Black and white (D)").small().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("color.set", json!({ "reset": true }), cx)))),
            )
    }
}

/// The options bar: the settings of the tool in hand.
pub struct OptionsBar {
    store: Entity<Store>,
    size: Entity<Scrub>,
    hardness: Entity<Scrub>,
    opacity: Entity<Scrub>,
    flow: Entity<Scrub>,
    tolerance: Entity<Scrub>,
    stroke_width: Entity<Scrub>,
    font_size: Entity<Scrub>,
    hex: Entity<TextInput>,
    _subs: Vec<Subscription>,
}

impl OptionsBar {
    pub fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = cx.store();
        let mk = |cx: &mut Context<Self>, label: &str, step: f64, dec: usize, unit: &str, min: f64, max: f64| cx.new(|_| Scrub::new(label.to_string(), step, dec).unit(unit.to_string()).range(min, max));
        let size = mk(cx, "Size", 1.0, 0, "px", 1.0, 2000.0);
        let hardness = mk(cx, "Hardness", 1.0, 0, "%", 0.0, 100.0);
        let opacity = mk(cx, "Opacity", 1.0, 0, "%", 1.0, 100.0);
        let flow = mk(cx, "Flow", 1.0, 0, "%", 1.0, 100.0);
        let tolerance = mk(cx, "Tolerance", 1.0, 0, "", 0.0, 255.0);
        let stroke_width = mk(cx, "Stroke", 0.5, 1, "px", 0.0, 500.0);
        let font_size = mk(cx, "Size", 1.0, 0, "px", 1.0, 2000.0);
        let hex = cx.new(|cx| {
            let mut i = TextInput::new(cx).placeholder("#000000");
            i.mono = true;
            i
        });
        let mut subs = vec![cx.observe(&store, |this: &mut Self, store, cx| {
            this.sync(store.read(cx), cx);
            cx.notify()
        })];
        // Each field sets its tool setting (a drag folds into one change).
        let field = |cx: &mut Context<Self>, e: &Entity<Scrub>, key: &'static str, scale: f64| {
            cx.subscribe(e, move |this: &mut Self, _, ch: &ScrubChange, cx| {
                let key = this.key_for(key, cx);
                let v = ch.value / scale;
                this.store.update(cx, |s, cx| s.run("app.setSetting", json!({ "key": key, "value": v }), cx));
            })
        };
        subs.push(field(cx, &size, "size", 1.0));
        subs.push(field(cx, &hardness, "hardness", 100.0));
        subs.push(field(cx, &opacity, "opacity", 100.0));
        subs.push(field(cx, &flow, "flow", 100.0));
        subs.push(field(cx, &tolerance, "tools.tolerance", 1.0));
        subs.push(field(cx, &stroke_width, "tools.shapeStrokeWidth", 1.0));
        subs.push(field(cx, &font_size, "tools.fontSize", 1.0));
        subs.push(cx.subscribe(&hex, |this: &mut Self, input, e: &InputEvent, cx| {
            if let InputEvent::Submit | InputEvent::Blur = e {
                let text = input.read(cx).text().trim().to_string();
                if nori_core::Color::parse(&text).is_ok() {
                    this.store.update(cx, |s, cx| s.run("color.set", json!({ "foreground": text }), cx));
                }
            }
        }));
        let mut this = Self { store: store.clone(), size, hardness, opacity, flow, tolerance, stroke_width, font_size, hex, _subs: subs };
        this.sync(store.read(cx), cx);
        this
    }

    /// The setting a brush field changes: the brush's or the eraser's.
    fn key_for(&self, key: &'static str, cx: &App) -> String {
        if key.contains('.') {
            return key.to_string();
        }
        let which = if self.store.read(cx).tool == Tool::Eraser { "eraser" } else { "brush" };
        format!("tools.{which}.{key}")
    }

    fn sync(&mut self, s: &Store, cx: &mut Context<Self>) {
        let tools = &s.settings.tools;
        let b = if s.tool == Tool::Eraser { &tools.eraser } else { &tools.brush };
        self.size.update(cx, |x, _| x.set_value(b.size as f64));
        self.hardness.update(cx, |x, _| x.set_value((b.hardness * 100.0) as f64));
        self.opacity.update(cx, |x, _| x.set_value((b.opacity * 100.0) as f64));
        self.flow.update(cx, |x, _| x.set_value((b.flow * 100.0) as f64));
        self.tolerance.update(cx, |x, _| x.set_value(tools.tolerance));
        self.stroke_width.update(cx, |x, _| x.set_value(tools.shape_stroke_width));
        self.font_size.update(cx, |x, _| x.set_value(tools.font_size));
        let hex = s.colors.foreground.hex();
        let focused = false;
        if !focused {
            self.hex.update(cx, |i, cx| i.set_text(hex, cx));
        }
    }

    fn hint(text: &'static str, cx: &App) -> AnyElement {
        div().text_size(px(sz::SM)).text_color(cx.theme().text_2).truncate().child(text).into_any_element()
    }
}

fn setting_switch(id: &'static str, label: &'static str, key: &'static str, on: bool, cx: &App) -> AnyElement {
    crate::ui::switch(id, label, on, move |v, _, cx| cx.store().update(cx, |s, cx| s.run("app.setSetting", json!({ "key": key, "value": v }), cx)), cx).text_size(px(sz::SM)).into_any_element()
}

impl Render for OptionsBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let s = self.store.read(cx);
        let tool = s.tool;
        let tools = s.settings.tools.clone();
        let combine = s.combine;
        let shape = s.shape;
        let has_selection = s.doc.as_ref().is_some_and(|d| d.selection.is_some());
        let mut row: Vec<AnyElement> = vec![div().flex_none().flex().items_center().gap(px(6.)).text_size(px(sz::SM)).text_color(t.text_2).child(icon(tool.icon())).child(tool.label()).into_any_element()];
        match tool {
            Tool::Brush | Tool::Eraser => {
                row.push(self.size.clone().into_any_element());
                row.push(self.hardness.clone().into_any_element());
                row.push(self.opacity.clone().into_any_element());
                row.push(self.flow.clone().into_any_element());
                let tip = if tool == Tool::Eraser { tools.eraser.tip.clone() } else { tools.brush.tip.clone() };
                row.push(
                    Button::new("brush-tip", tip.unwrap_or_else(|| "Round".into()))
                        .small()
                        .icon_after("chevron-down")
                        .on_click(move |e, _, cx| {
                            let which = if tool == Tool::Eraser { "eraser" } else { "brush" };
                            let tips: Vec<String> = cx.store().read(cx).session.brushes.read().iter().map(|t| t.name.clone()).collect();
                            let mut entries = vec![MenuItem::new("Round", move |_, cx| cx.store().update(cx, |s, cx| s.run("app.setSetting", json!({ "key": format!("tools.{which}.tip"), "value": null }), cx))).entry()];
                            for n in tips {
                                let n2 = n.clone();
                                entries.push(MenuItem::new(n, move |_, cx| cx.store().update(cx, |s, cx| s.run("app.setSetting", json!({ "key": format!("tools.{which}.tip"), "value": n2 }), cx))).entry());
                            }
                            entries.push(crate::store::MenuEntry::Separator);
                            entries.push(MenuItem::new("Import a GIMP brush (.gbr)…", |_, cx| import_brush(cx)).icon("download").entry());
                            cx.store().update(cx, |s, cx| s.open_menu(e.position(), entries, cx));
                        })
                        .into_any_element(),
                );
            }
            Tool::Select | Tool::EllipseSelect | Tool::Lasso | Tool::Wand => {
                let opts: Vec<(&'static str, gpui::SharedString)> = vec![("replace", "New".into()), ("add", "Add".into()), ("subtract", "Subtract".into()), ("intersect", "Intersect".into())];
                row.push(crate::ui::segmented("combine", opts, combine, |v, _, cx| cx.store().update(cx, |s, cx| { s.combine = *v; cx.notify(); }), cx).w(px(260.)).into_any_element());
                if tool == Tool::Wand {
                    row.push(self.tolerance.clone().into_any_element());
                    row.push(setting_switch("contiguous", "Contiguous", "tools.contiguous", tools.contiguous, cx));
                }
                row.push(group(
                    [
                        Button::new("sel-all", "All").small().flush().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("select.all", json!({}), cx))).into_any_element(),
                        Button::new("sel-none", "None").small().flush().disabled(!has_selection).on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("select.none", json!({}), cx))).into_any_element(),
                        Button::new("sel-invert", "Invert").small().flush().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("select.invert", json!({}), cx))).into_any_element(),
                        Button::new("sel-feather", "Feather 4 px").small().flush().disabled(!has_selection).on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("select.modify", json!({ "feather": 4 }), cx))).into_any_element(),
                    ],
                    cx,
                ).into_any_element());
            }
            Tool::Fill => {
                row.push(self.tolerance.clone().into_any_element());
                row.push(setting_switch("contiguous", "Contiguous", "tools.contiguous", tools.contiguous, cx));
                row.push(setting_switch("sample-all", "All layers", "tools.sampleAll", tools.sample_all, cx));
            }
            Tool::Gradient => {
                let opts: Vec<(String, gpui::SharedString)> = vec![("linear".into(), "Linear".into()), ("radial".into(), "Radial".into()), ("reflected".into(), "Reflected".into())];
                row.push(crate::ui::segmented("gradient-kind", opts, tools.gradient.clone(), |v, _, cx| cx.store().update(cx, |s, cx| s.run("app.setSetting", json!({ "key": "tools.gradient", "value": v }), cx)), cx).w(px(240.)).into_any_element());
                row.push(Self::hint("Drag from where the foreground colour starts to where the background colour ends.", cx));
            }
            Tool::Shape | Tool::Pen | Tool::Direct => {
                if tool == Tool::Shape {
                    row.push(group(
                        ShapeKind::ALL.map(|k| Button::icon(gpui::SharedString::from(format!("shape-{}", k.id())), k.icon(), k.label()).small().flush().selected(shape == k).on_click(move |_, _, cx| cx.store().update(cx, |s, cx| { s.shape = k; cx.notify(); })).into_any_element()),
                        cx,
                    ).into_any_element());
                }
                let fill = nori_core::Color::parse(&tools.shape_fill).ok();
                row.push(div().flex().items_center().gap(px(6.)).text_size(px(sz::SM)).text_color(t.text_2).child("Fill").child(
                    match fill {
                        Some(c) => swatch("shape-fill", c, 20., cx).on_click(|e, _, cx| shape_color_menu("tools.shapeFill", e.position(), cx)).into_any_element(),
                        None => Button::new("shape-fill", "None").small().on_click(|e, _, cx| shape_color_menu("tools.shapeFill", e.position(), cx)).into_any_element(),
                    },
                ).into_any_element());
                let stroke = nori_core::Color::parse(&tools.shape_stroke).ok();
                row.push(div().flex().items_center().gap(px(6.)).text_size(px(sz::SM)).text_color(t.text_2).child("Stroke").child(
                    match stroke {
                        Some(c) => swatch("shape-stroke", c, 20., cx).on_click(|e, _, cx| shape_color_menu("tools.shapeStroke", e.position(), cx)).into_any_element(),
                        None => Button::new("shape-stroke", "None").small().on_click(|e, _, cx| shape_color_menu("tools.shapeStroke", e.position(), cx)).into_any_element(),
                    },
                ).into_any_element());
                row.push(self.stroke_width.clone().into_any_element());
                if tool == Tool::Pen {
                    row.push(Self::hint("Click for corners, drag for curves; click the first point to close, Enter to finish.", cx));
                } else if tool == Tool::Direct {
                    row.push(Self::hint("Drag a point or a handle of the selected shape.", cx));
                }
            }
            Tool::Text | Tool::Frame => {
                row.push(
                    Button::new("font", tools.font.clone())
                        .small()
                        .icon_after("chevron-down")
                        .on_click(|e, _, cx| {
                            let fams: Vec<String> = nori_render::text::families().into_iter().take(60).collect();
                            let entries = fams
                                .into_iter()
                                .map(|f| {
                                    let f2 = f.clone();
                                    MenuItem::new(f, move |_, cx| cx.store().update(cx, |s, cx| s.run("app.setSetting", json!({ "key": "tools.font", "value": f2 }), cx))).entry()
                                })
                                .collect();
                            cx.store().update(cx, |s, cx| s.open_menu(e.position(), entries, cx));
                        })
                        .into_any_element(),
                );
                row.push(self.font_size.clone().into_any_element());
                row.push(Self::hint(if tool == Tool::Text { "Click to type; drag to make a text frame." } else { "Drag a text frame; thread frames in Properties so words flow." }, cx));
            }
            Tool::Crop => row.push(Self::hint("Drag the area to keep. Undo puts it back.", cx)),
            Tool::Eyedropper => row.push(Self::hint("Click to pick the foreground colour; Alt-click for the background.", cx)),
            Tool::Move => row.push(Self::hint("Click a layer to select it, drag to move it; Shift keeps it straight.", cx)),
            Tool::Hand => row.push(Self::hint("Drag to look around; scroll does too, with ⌘ or Ctrl to zoom.", cx)),
            Tool::Zoom => row.push(Self::hint("Click to zoom in, Alt-click to zoom out.", cx)),
        }
        // The foreground colour as a hex field, for every painting tool.
        if matches!(tool, Tool::Brush | Tool::Fill | Tool::Gradient | Tool::Text | Tool::Frame) {
            row.push(div().flex_none().flex().items_center().gap(px(6.)).child(caps("Colour", cx)).child(div().w(px(92.)).child(self.hex.clone())).into_any_element());
        }
        div().id("options").flex().items_center().gap(px(12.)).overflow_x_scroll().on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).children(row)
    }
}

fn shape_color_menu(key: &'static str, at: gpui::Point<gpui::Pixels>, cx: &mut App) {
    let fg = cx.store().read(cx).colors.foreground.hex();
    let entries = vec![
        MenuItem::new(format!("Foreground colour  {fg}"), move |_, cx| cx.store().update(cx, |s, cx| s.run("app.setSetting", json!({ "key": key, "value": fg.clone() }), cx))).icon("square").entry(),
        MenuItem::new("Black", move |_, cx| cx.store().update(cx, |s, cx| s.run("app.setSetting", json!({ "key": key, "value": "#000000" }), cx))).icon("square").entry(),
        MenuItem::new("White", move |_, cx| cx.store().update(cx, |s, cx| s.run("app.setSetting", json!({ "key": key, "value": "#ffffff" }), cx))).icon("square").entry(),
        MenuItem::new("None", move |_, cx| cx.store().update(cx, |s, cx| s.run("app.setSetting", json!({ "key": key, "value": "" }), cx))).icon("x").entry(),
    ];
    cx.store().update(cx, |s, cx| s.open_menu(at, entries, cx));
}

fn import_brush(cx: &mut App) {
    let rx = cx.prompt_for_paths(gpui::PathPromptOptions { files: true, directories: false, multiple: true, prompt: Some("Import".into()) });
    let store = cx.store();
    cx.spawn(async move |cx| {
        let Ok(Ok(Some(paths))) = rx.await else { return };
        store.update(cx, |s, cx| {
            for p in paths {
                s.run("brushes.import", json!({ "path": p }), cx);
            }
        });
    })
    .detach();
}
