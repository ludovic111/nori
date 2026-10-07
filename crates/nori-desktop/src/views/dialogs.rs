//! Dialogs (tier-3 glass over the scrim, inside corner brackets): new document, export,
//! settings (appearance, agent, lsuite AI, about), plugins, a filter's parameters, shortcuts,
//! what's new.

use std::collections::HashMap;

use gpui::{AnyElement, App, Context, Entity, FontWeight, MouseButton, Render, SharedString, Subscription, Window, deferred, div, prelude::*, px};
use serde_json::{Value, json};

use crate::store::{Dialog, Store, StoreExt};
use crate::theme::{ActiveTheme, MONO, size as sz};
use crate::ui::input::{InputEvent, TextInput};
use crate::ui::scrub::{Scrub, ScrubChange};
use crate::ui::{Button, GlassExt, caps, icon, logo, motion};

pub struct Dialogs {
    store: Entity<Store>,
    section: String,
    // New document.
    new_name: Entity<TextInput>,
    new_w: Entity<Scrub>,
    new_h: Entity<Scrub>,
    new_dpi: Entity<Scrub>,
    new_pages: Entity<Scrub>,
    new_bg: &'static str,
    // Export.
    export_format: &'static str,
    export_scale: f32,
    export_quality: Entity<Scrub>,
    // Filter parameters.
    filter_values: HashMap<String, f64>,
    filter_scrubs: HashMap<String, (Entity<Scrub>, Subscription)>,
    filter_for: String,
    // Settings › Agent.
    key_input: Entity<TextInput>,
    key_for: String,
    // Plugins › Build with your agent.
    describe: Entity<TextInput>,
    _subs: Vec<Subscription>,
}

impl Dialogs {
    pub fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = cx.store();
        let num = |cx: &mut Context<Self>, label: &str, v: f64, step: f64, unit: &str, min: f64, max: f64| {
            cx.new(|_| {
                let mut s = Scrub::new(label.to_string(), step, 0).unit(unit.to_string()).range(min, max);
                s.set_value(v);
                s
            })
        };
        let new_w = num(cx, "Width", 1920.0, 1.0, "px", 1.0, 30000.0);
        let new_h = num(cx, "Height", 1080.0, 1.0, "px", 1.0, 30000.0);
        let new_dpi = num(cx, "Resolution", 72.0, 1.0, "dpi", 1.0, 2400.0);
        let new_pages = num(cx, "Pages", 1.0, 1.0, "", 1.0, 999.0);
        let export_quality = num(cx, "Quality", 90.0, 1.0, "", 1.0, 100.0);
        let new_name = cx.new(|cx| {
            let mut i = TextInput::new(cx).placeholder("Untitled");
            i.set_text("Untitled", cx);
            i
        });
        let key_input = cx.new(|cx| {
            let mut i = TextInput::new(cx).placeholder("Paste the API key");
            i.mono = true;
            i
        });
        let describe = cx.new(|cx| TextInput::new(cx).multiline(3).placeholder("Describe the plugin you want: \"a film grain that is stronger in the shadows\""));
        let mut subs = vec![cx.observe(&store, |_, _, cx| cx.notify())];
        subs.push(cx.subscribe(&key_input, |this: &mut Self, input, e: &InputEvent, cx| {
            if let InputEvent::Submit = e {
                let key = input.read(cx).text().trim().to_string();
                let p = this.key_for.clone();
                if !key.is_empty() && !p.is_empty() {
                    this.store.update(cx, |s, cx| s.run_then("agent.setKey", json!({ "provider": p, "key": key }), cx, |s, _, cx| s.flash("Saved in the keychain.", cx)));
                    input.update(cx, |i, cx| i.set_text("", cx));
                }
            }
        }));
        Self {
            store,
            section: "appearance".into(),
            new_name,
            new_w,
            new_h,
            new_dpi,
            new_pages,
            new_bg: "white",
            export_format: "png",
            export_scale: 1.0,
            export_quality,
            filter_values: HashMap::new(),
            filter_scrubs: HashMap::new(),
            filter_for: String::new(),
            key_input,
            key_for: "anthropic".into(),
            describe,
            _subs: subs,
        }
    }

    // ---- new document ----------------------------------------------------------------------

    fn new_document(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let bg = self.new_bg;
        let this = cx.entity();
        let (e1, e2) = (this.clone(), this.clone());
        body(
            "New document",
            None,
            div()
                .flex()
                .flex_col()
                .gap(px(14.))
                .child(field("Name", self.new_name.clone().into_any_element(), cx))
                .child(div().flex().gap(px(8.)).child(div().flex_1().child(self.new_w.clone())).child(div().flex_1().child(self.new_h.clone())))
                .child(div().flex().gap(px(8.)).child(div().flex_1().child(self.new_dpi.clone())).child(div().flex_1().child(self.new_pages.clone())))
                .child(field(
                    "Background",
                    crate::ui::segmented("new-bg", vec![("white", "White".into()), ("transparent", "Transparent".into()), ("black", "Black".into())], bg, move |v, _, cx| e1.update(cx, |d, cx| { d.new_bg = *v; cx.notify(); }), cx).into_any_element(),
                    cx,
                ))
                .child(div().text_size(px(sz::SM)).text_color(t.text_2).child("Several pages make a layout (master pages, text that flows from page to page); 300 dpi is for print."))
                .into_any_element(),
            vec![
                Button::new("new-cancel", "Cancel").on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.close_dialog(cx))).into_any_element(),
                Button::new("new-create", "Create").primary().on_click(move |_, _, cx| {
                    let d = e2.read(cx);
                    let name = d.new_name.read(cx).text().trim().to_string();
                    let params = json!({
                        "name": if name.is_empty() { "Untitled".to_string() } else { name },
                        "width": d.new_w.read(cx).value(),
                        "height": d.new_h.read(cx).value(),
                        "dpi": d.new_dpi.read(cx).value(),
                        "pages": d.new_pages.read(cx).value(),
                        "background": match d.new_bg { "black" => "#000000", "transparent" => "transparent", _ => "#ffffff" },
                    });
                    cx.store().update(cx, |s, cx| {
                        s.close_dialog(cx);
                        s.run("doc.new", params, cx);
                    })
                })
                .into_any_element(),
            ],
            cx,
        )
    }

    // ---- export ----------------------------------------------------------------------------

    fn export(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let s = self.store.read(cx);
        let Some(doc) = s.doc.clone() else { return div().into_any_element() };
        let fmt = self.export_format;
        let scale = self.export_scale;
        let this = cx.entity();
        let (e1, e2, e3) = (this.clone(), this.clone(), this.clone());
        let picture = matches!(fmt, "png" | "jpeg" | "webp" | "tiff");
        let page = doc.page();
        let (w, h) = ((page.width as f32 * scale).round(), (page.height as f32 * scale).round());
        let what = match fmt {
            "pdf" => format!("{} page{} as PDF: shapes and text stay vectors, sharp at any size.", doc.pages.len(), if doc.pages.len() == 1 { "" } else { "s" }),
            "svg" => "This page as SVG: shapes and text as vectors, pictures embedded.".into(),
            "ora" => "This page with its layers, for Krita, GIMP and MyPaint.".into(),
            "nori" => "Everything, to open in nori again.".into(),
            "jpeg" => format!("{w}×{h} pixels, no transparency."),
            _ => format!("{w}×{h} pixels."),
        };
        let formats: Vec<(&'static str, SharedString)> = vec![("png", "PNG".into()), ("jpeg", "JPEG".into()), ("webp", "WebP".into()), ("tiff", "TIFF".into()), ("pdf", "PDF".into()), ("svg", "SVG".into()), ("ora", "ORA".into())];
        let kimchi = nori_control::discovery::find("kimchi").is_some_and(|k| k.running.is_some());
        body(
            "Export",
            Some(format!("{}×{} · {}", page.width, page.height, doc.name)),
            div()
                .flex()
                .flex_col()
                .gap(px(14.))
                .child(field("Format", crate::ui::segmented("export-format", formats, fmt, move |v, _, cx| e1.update(cx, |d, cx| { d.export_format = *v; cx.notify(); }), cx).into_any_element(), cx))
                .when(picture, |d| {
                    d.child(field(
                        "Size",
                        crate::ui::segmented("export-scale", vec![(0.5f32, "½×".into()), (1.0, "1×".into()), (2.0, "2×".into()), (3.0, "3×".into()), (4.0, "4×".into())], scale, move |v, _, cx| e2.update(cx, |d, cx| { d.export_scale = *v; cx.notify(); }), cx).into_any_element(),
                        cx,
                    ))
                })
                .when(fmt == "jpeg", |d| d.child(self.export_quality.clone()))
                .child(div().text_size(px(sz::SM)).text_color(t.text_2).child(what))
                .when(kimchi, |d| {
                    d.child(
                        div().flex().items_center().gap(px(10.)).p(px(10.)).border_1().border_color(t.line).child(logo("kimchi", px(22.))).child(div().flex_1().text_size(px(sz::SM)).child("kimchi is open: put this picture on its timeline.")).child(Button::new("to-kimchi", "Send to kimchi").small().on_click(|_, _, cx| {
                            cx.store().update(cx, |s, cx| {
                                s.close_dialog(cx);
                                s.run_then("handoff.toKimchi", json!({}), cx, |s, _, cx| s.toast(nori_control::ToastKind::Success, "Sent to kimchi: it's on the timeline.", cx));
                            })
                        })),
                    )
                })
                .into_any_element(),
            vec![
                Button::new("export-cancel", "Cancel").on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.close_dialog(cx))).into_any_element(),
                Button::new("export-go", "Export…").primary().with_icon("share-2").on_click(move |_, _, cx| export_to_file(&e3, cx)).into_any_element(),
            ],
            cx,
        )
    }

    // ---- filter ----------------------------------------------------------------------------

    fn filter(&mut self, id: &str, cx: &mut Context<Self>) -> AnyElement {
        let (name, params): (String, Vec<(String, String, f64, f64, f64, String, String)>) = if let Some(p) = id.strip_prefix("plugin:") {
            let s = self.store.read(cx);
            let f = s.session.plugins.find(p);
            match f {
                Some(l) => (l.info.name.clone(), l.info.params.iter().map(|p| (p.name.clone(), if p.label.is_empty() { p.name.clone() } else { p.label.clone() }, p.min, p.max, p.default, p.unit.clone(), p.kind.clone())).collect()),
                None => return div().into_any_element(),
            }
        } else {
            match nori_render::filters::spec(id) {
                Some(f) => (f.name.to_string(), f.params.iter().map(|p| (p.name.to_string(), p.label.to_string(), p.min, p.max, p.default, p.unit.to_string(), p.kind.to_string())).collect()),
                None => return div().into_any_element(),
            }
        };
        if self.filter_for != id {
            self.filter_for = id.to_string();
            self.filter_values = params.iter().map(|p| (p.0.clone(), p.4)).collect();
            self.filter_scrubs.clear();
        }
        let mut fields: Vec<AnyElement> = vec![];
        for (pname, label, min, max, default, unit, kind) in &params {
            if kind == "boolean" {
                let on = self.filter_values.get(pname).copied().unwrap_or(*default) >= 0.5;
                let (k, e) = (pname.clone(), cx.entity());
                fields.push(crate::ui::switch(SharedString::from(format!("f-{pname}")), label.clone(), on, move |v, _, cx| e.update(cx, |d, cx| { d.filter_values.insert(k.clone(), if v { 1.0 } else { 0.0 }); cx.notify(); }), cx).into_any_element());
                continue;
            }
            if !self.filter_scrubs.contains_key(pname) {
                let dec = if kind == "number" && max - min <= 20.0 { 2 } else { 0 };
                let step = if dec > 0 { 0.05 } else { 1.0 };
                let e = cx.new(|_| {
                    let mut s = Scrub::new(label.clone(), step, dec).unit(unit.clone()).range(*min, *max);
                    s.set_value(*default);
                    s
                });
                let k = pname.clone();
                let sub = cx.subscribe(&e, move |d: &mut Self, _, ch: &ScrubChange, cx| {
                    d.filter_values.insert(k.clone(), ch.value);
                    cx.notify();
                });
                self.filter_scrubs.insert(pname.clone(), (e, sub));
            }
            fields.push(self.filter_scrubs[pname].0.clone().into_any_element());
        }
        let has_sel = self.store.read(cx).doc.as_ref().is_some_and(|d| d.selection.is_some());
        let e = cx.entity();
        let fid = id.to_string();
        body(
            &name,
            Some(if has_sel { "Inside the selection".into() } else { "On the whole layer".into() }),
            div().flex().flex_col().gap(px(10.)).children(fields).into_any_element(),
            vec![
                Button::new("filter-cancel", "Cancel").on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.close_dialog(cx))).into_any_element(),
                Button::new("filter-apply", "Apply").primary().on_click(move |_, _, cx| {
                    let values: serde_json::Map<String, Value> = e.read(cx).filter_values.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
                    let id = fid.clone();
                    cx.store().update(cx, |s, cx| {
                        s.close_dialog(cx);
                        s.run("filter.apply", json!({ "filter": id, "params": values }), cx);
                    })
                })
                .into_any_element(),
            ],
            cx,
        )
    }

    // ---- settings --------------------------------------------------------------------------

    fn settings(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let sections = [("appearance", "Appearance", "sun-moon"), ("agent", "Agent", "bot"), ("account", "lsuite AI", "sparkles"), ("about", "About", "info")];
        let current = self.section.clone();
        let nav = div().w(px(180.)).flex_none().flex().flex_col().gap(px(2.)).p(px(10.)).border_r_1().border_color(t.line).children(sections.iter().map(|(id, label, ic)| {
            let sel = current == *id;
            let id_s = id.to_string();
            div()
                .id(SharedString::from(format!("section-{id}")))
                .flex()
                .items_center()
                .gap(px(8.))
                .h(px(32.))
                .px(px(10.))
                .cursor_pointer()
                .when(sel, |d| d.bg(t.accent).text_color(t.text_on_accent))
                .when(!sel, |d| d.hover(|s| s.bg(t.hover)))
                .on_click(cx.listener(move |d, _, _, cx| {
                    d.section = id_s.clone();
                    cx.notify();
                }))
                .child(icon(if *ic == "sun-moon" { "sun" } else { ic }))
                .child(*label)
        }));
        let content = match current.as_str() {
            "agent" => self.agent_settings(cx),
            "account" => account_section(cx),
            "about" => about_section(cx),
            _ => appearance_section(cx),
        };
        div()
            .flex()
            .h(px(520.))
            .child(nav)
            .child(div().flex_1().min_w_0().flex().flex_col().child(header("Settings", None, cx)).child(div().id("settings-body").flex_1().min_h_0().overflow_y_scroll().p(px(18.)).child(content)))
            .into_any_element()
    }

    fn agent_settings(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let s = self.store.read(cx);
        let settings = s.settings.clone();
        let signed = s.account["signedIn"] == json!(true);
        let providers: [(&str, &str, &str); 7] = [
            ("lsuite", "lsuite AI", "No setup. Sign in and your agent works."),
            ("claude-code", "Claude Code", "Your Claude Code on this computer."),
            ("codex", "Codex", "Your Codex CLI on this computer."),
            ("anthropic", "Anthropic API", "Claude with your own API key."),
            ("openai", "OpenAI API", "GPT with your own API key."),
            ("ollama", "Ollama", "A model on this computer, free and offline."),
            ("openai-compatible", "OpenAI-compatible", "Any server that speaks the OpenAI API."),
        ];
        let current = settings.agent.provider.clone();
        let list = div().flex().flex_col().gap(px(6.)).children(providers.iter().map(|(id, name, line)| {
            let sel = current == *id;
            let pid = id.to_string();
            let needs_key = matches!(*id, "anthropic" | "openai" | "openai-compatible");
            let has_key = needs_key && s.session.secret(id).is_some();
            let status = match *id {
                "lsuite" if signed => "Signed in",
                "lsuite" => "Sign in to use it",
                _ if needs_key && has_key => "Key saved",
                _ if needs_key => "Needs a key",
                _ => "",
            };
            div()
                .id(SharedString::from(format!("provider-{id}")))
                .flex()
                .items_center()
                .gap(px(10.))
                .p(px(10.))
                .border_1()
                .border_color(if sel { t.accent } else { t.line })
                .cursor_pointer()
                .hover(|s| s.bg(t.hover))
                .on_click(cx.listener(move |d, _, _, cx| {
                    let p = pid.clone();
                    if matches!(p.as_str(), "anthropic" | "openai" | "openai-compatible") {
                        d.key_for = p.clone();
                    }
                    d.store.update(cx, |s, cx| s.run("agent.setProvider", json!({ "provider": p }), cx));
                }))
                .child(logo(id, px(24.)))
                .child(div().flex_1().min_w_0().flex().flex_col().child(div().font_weight(FontWeight::SEMIBOLD).child(*name)).child(div().text_size(px(sz::SM)).text_color(t.text_2).child(*line)))
                .when(!status.is_empty(), |d| d.child(div().font_family(MONO).text_size(px(10.)).text_color(t.text_2).child(status.to_uppercase())))
                .when(sel, |d| d.child(icon("check")))
        }));
        let key_for = self.key_for.clone();
        let perms = settings.agent.permissions.clone();
        let perm = |id: &'static str, label: &'static str, key: &'static str, on: bool, cx: &App| crate::ui::switch(id, label, on, move |v, _, cx| cx.store().update(cx, |s, cx| s.run("app.setSetting", json!({ "key": key, "value": v }), cx)), cx).into_any_element();
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(caps("What runs the agent", cx))
            .child(list)
            .when(current == "lsuite" && !signed, |d| d.child(Button::new("agent-sign-in", "Sign in to lsuite AI").primary().with_icon("log-in").on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("account.signIn", json!({}), cx)))))
            .child(div().flex().flex_col().gap(px(6.)).child(caps(format!("API key · {key_for}"), cx)).child(self.key_input.clone()).child(div().text_size(px(sz::SM)).text_color(t.text_2).child(format!("Press Enter to save it in the {}. It never leaves this computer except to its provider.", crate::ui::keychain_name()))))
            .child(caps("Your own agent, outside nori", cx))
            .child(div().text_size(px(sz::SM)).text_color(t.text_2).child("Claude Code, Codex, Cursor or Claude Desktop can drive nori through its MCP server, with the same commands (and the same permissions) as the Agent panel:"))
            .child({
                let line = mcp_line(&s.session.data_dir);
                let copy = line.clone();
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().flex_1().min_w_0().px(px(10.)).py(px(8.)).border_1().border_color(t.line).font_family(MONO).text_size(px(sz::SM)).truncate().child(line))
                    .child(Button::new("copy-mcp", "Copy").small().with_icon("copy").on_click(move |_, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy.clone()));
                        cx.store().update(cx, |s, cx| s.flash("Copied: paste it in a terminal.", cx));
                    }))
            })
            .child(caps("What agents may do", cx))
            .child(perm("perm-enabled", "Agents and MCP clients can use nori", "agent.permissions.enabled", perms.enabled, cx))
            .child(perm("perm-files", "Open, save and export files", "agent.permissions.files", perms.files, cx))
            .child(perm("perm-settings", "Change settings", "agent.permissions.settings", perms.settings, cx))
            .child(perm("perm-plugins", "Write, build and install plugins", "agent.permissions.plugins", perms.plugins, cx))
            .child(perm("perm-app", "Quit nori", "agent.permissions.appControl", perms.app_control, cx))
            .child(div().text_size(px(sz::SM)).text_color(t.text_2).child("Editing the open document is always allowed and always undoable."))
            .into_any_element()
    }

    // ---- plugins ---------------------------------------------------------------------------

    fn plugins(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let s = self.store.read(cx);
        let p = s.plugins.clone();
        let toolchain_ok = nori_control::commands::plugin::cargo().is_some();
        let row = |name: String, detail: String, logo_id: &'static str, id: String, enabled: bool, removable: bool, cx: &App| {
            let t = cx.theme();
            let (i1, i2) = (id.clone(), id.clone());
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .py(px(8.))
                .border_b_1()
                .border_color(t.line)
                .child(logo(logo_id, px(20.)))
                .child(div().flex_1().min_w_0().flex().flex_col().child(div().font_weight(FontWeight::MEDIUM).child(name)).child(div().text_size(px(sz::SM)).text_color(t.text_2).child(detail)))
                .when(removable, |d| d.child(Button::new(SharedString::from(format!("remove-{id}")), "Remove").small().danger().on_click(move |_, _, cx| { let i = i1.clone(); cx.store().update(cx, |s, cx| s.run("plugin.remove", json!({ "id": i }), cx)) })))
                .child(crate::ui::switch(SharedString::from(format!("switch-{id}")), "", enabled, move |v, _, cx| { let i = i2.clone(); cx.store().update(cx, |s, cx| s.run(if v { "plugin.enable" } else { "plugin.disable" }, json!({ "id": i }), cx)) }, cx))
                .into_any_element()
        };
        let mut stock = vec![];
        for x in p["stock"].as_array().into_iter().flatten() {
            stock.push(row(x["name"].as_str().unwrap_or("").into(), x["description"].as_str().unwrap_or("").into(), "lsuite", x["id"].as_str().unwrap_or("").into(), x["enabled"] == json!(true), false, cx));
        }
        let mut installed = vec![];
        for x in p["installed"].as_array().into_iter().flatten() {
            let detail = format!("{} · {} · {}", x["vendor"].as_str().unwrap_or(""), x["version"].as_str().unwrap_or(""), x["description"].as_str().unwrap_or(""));
            installed.push(row(x["name"].as_str().unwrap_or("").into(), detail, "lsuite", x["id"].as_str().unwrap_or("").into(), x["enabled"] == json!(true), true, cx));
        }
        let problems: Vec<AnyElement> = p["problems"].as_array().into_iter().flatten().map(|x| div().flex().gap(px(6.)).text_size(px(sz::SM)).text_color(t.danger).child(icon("circle-alert")).child(format!("{}: {}", x["path"].as_str().unwrap_or(""), x["error"].as_str().unwrap_or(""))).into_any_element()).collect();
        let formats: Vec<AnyElement> = p["formats"].as_array().into_iter().flatten().map(|f| {
            let logo_id: &'static str = match f["logo"].as_str().unwrap_or("") {
                "resolve" => "resolve",
                "photoshop" => "photoshop",
                "gimp" => "gimp",
                _ => "lsuite",
            };
            div().flex().items_center().gap(px(10.)).py(px(8.)).border_b_1().border_color(t.line).child(logo(logo_id, px(20.))).child(div().flex_1().min_w_0().flex().flex_col().child(div().font_weight(FontWeight::MEDIUM).child(f["name"].as_str().unwrap_or("").to_string())).child(div().text_size(px(sz::SM)).text_color(t.text_2).child(format!("{} · {}", f["loads"].as_str().unwrap_or(""), f["where"].as_str().unwrap_or(""))))).into_any_element()
        }).collect();
        let describe = self.describe.clone();
        div()
            .flex()
            .flex_col()
            .max_h(px(620.))
            .child(header("Plugins", Some("Filters that ship with nori, the ones you add, and the ones your agent writes".into()), cx))
            .child(
                div()
                    .id("plugins-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(18.))
                    .flex()
                    .flex_col()
                    .gap(px(18.))
                    // Build with your agent.
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .p(px(14.))
                            .border_1()
                            .border_color(t.line_strong)
                            .child(div().flex().items_center().gap(px(8.)).child(icon("sparkles")).child(div().font_weight(FontWeight::SEMIBOLD).child("Build with your agent")))
                            .child(describe)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .child(logo("rust", px(18.)))
                                    .child(div().flex_1().text_size(px(sz::SM)).text_color(t.text_2).child(if toolchain_ok { "Rust is installed: plugins build on this computer." } else { "Plugins are written in Rust, which isn't installed yet." }))
                                    .when(!toolchain_ok, |d| d.child(Button::new("install-rust", "Install Rust").small().on_click(|_, _, cx| {
                                        cx.store().update(cx, |s, cx| s.flash("Installing Rust with rustup… it takes a few minutes.", cx));
                                        let task = gpui_tokio::Tokio::spawn(cx, async { tokio::task::spawn_blocking(nori_control::commands::plugin::install_rust).await.unwrap_or_else(|e| Err(e.to_string())) });
                                        let store = cx.store();
                                        cx.spawn(async move |cx| {
                                            let r = task.await.unwrap_or_else(|e| Err(e.to_string()));
                                            store.update(cx, |s, cx| match r {
                                                Ok(()) => s.toast(nori_control::ToastKind::Success, "Rust is installed.", cx),
                                                Err(e) => s.error(e, cx),
                                            });
                                        })
                                        .detach();
                                    })))
                                    .child(Button::new("build-plugin", "Ask the agent").primary().with_icon("send").on_click(cx.listener(|d, _, _, cx| {
                                        let text = d.describe.read(cx).text().trim().to_string();
                                        if text.is_empty() {
                                            return;
                                        }
                                        let prompt = format!("Write a nori plugin: {text}. Follow plugin_guide's recipe (plugin_toolchain, plugin_new, write src/lib.rs, plugin_build until it is green, plugin_publishLocal), then try it on the open picture with filter_apply and look at it.");
                                        d.describe.update(cx, |i, cx| i.set_text("", cx));
                                        d.store.update(cx, |s, cx| {
                                            s.close_dialog(cx);
                                            s.set_agent_open(true, cx);
                                            s.run("agent.send", json!({ "prompt": prompt }), cx);
                                        });
                                    }))),
                            ),
                    )
                    .child(div().flex().flex_col().child(div().flex().items_center().justify_between().child(caps("Installed", cx)).child(Button::new("rescan", "Rescan").small().with_icon("refresh-cw").on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("plugin.rescan", json!({}), cx))))).children(installed).when(p["installed"].as_array().is_none_or(|a| a.is_empty()), |d| d.child(div().py(px(8.)).text_size(px(sz::SM)).text_color(t.text_2).child(format!("None yet. Bundles go in {}.", nori_control::plugins::plugins_dir().display())))).children(problems))
                    .child(div().flex().flex_col().child(caps("Stock", cx)).children(stock))
                    .child(div().flex().flex_col().child(caps("Formats nori loads", cx)).children(formats)),
            )
            .into_any_element()
    }
}

/// Picks where to export and does it.
fn export_to_file(d: &Entity<Dialogs>, cx: &mut App) {
    let s = cx.store().read(cx);
    let Some(doc) = s.doc.clone() else { return };
    let dlg = d.read(cx);
    let fmt = dlg.export_format;
    let scale = dlg.export_scale;
    let quality = dlg.export_quality.read(cx).value();
    let ext = match fmt {
        "jpeg" => "jpg",
        f => f,
    };
    let dir = s.saved_to.as_ref().or(s.opened_from.as_ref()).and_then(|p| p.parent().map(|d| d.to_path_buf())).or_else(dirs::picture_dir).unwrap_or_else(std::env::temp_dir);
    let name = format!("{}.{ext}", if doc.name.is_empty() { "Untitled" } else { &doc.name });
    let rx = cx.prompt_for_new_path(&dir, Some(&name));
    let store = cx.store();
    cx.spawn(async move |cx| {
        let Ok(Ok(Some(path))) = rx.await else { return };
        store.update(cx, |s, cx| {
            s.close_dialog(cx);
            s.run_then("export.file", json!({ "path": path, "format": fmt, "scale": scale, "quality": quality }), cx, |s, v, cx| {
                s.toast(nori_control::ToastKind::Success, format!("Exported {}", crate::app::short(v["path"].as_str().unwrap_or(""))), cx)
            });
        });
    })
    .detach();
}

fn header(title: &str, detail: Option<String>, cx: &App) -> AnyElement {
    let t = cx.theme();
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(10.))
        .h(px(50.))
        .px(px(18.))
        .border_b_1()
        .border_color(t.line)
        .child(div().text_size(px(sz::LG)).font_weight(FontWeight::SEMIBOLD).child(title.to_string()))
        .children(detail.map(|d| div().flex_1().min_w_0().truncate().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).child(d)))
        .child(div().flex_1())
        .child(Button::icon("dialog-close", "x", "Close").small().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.close_dialog(cx))))
        .into_any_element()
}

fn body(title: &str, detail: Option<String>, content: AnyElement, buttons: Vec<AnyElement>, cx: &App) -> AnyElement {
    let t = cx.theme();
    div()
        .flex()
        .flex_col()
        .child(header(title, detail, cx))
        .child(div().p(px(18.)).child(content))
        .child(div().flex().justify_end().gap(px(8.)).px(px(18.)).py(px(12.)).border_t_1().border_color(t.line).children(buttons))
        .into_any_element()
}

fn field(label: &str, el: AnyElement, cx: &App) -> AnyElement {
    div().flex().flex_col().gap(px(6.)).child(caps(label.to_string(), cx)).child(el).into_any_element()
}

fn appearance_section(cx: &App) -> AnyElement {
    let s = cx.store().read(cx);
    let a = s.settings.appearance.clone();
    let set = |key: &'static str| move |v: &&'static str, _: &mut Window, cx: &mut App| cx.store().update(cx, |s, cx| s.run("app.setSetting", json!({ "key": key, "value": *v }), cx));
    let mode: &'static str = match a.mode.as_str() {
        "dark" => "dark",
        "light" => "light",
        _ => "system",
    };
    let board: &'static str = match a.pasteboard.as_str() {
        "light" => "light",
        "mid" => "mid",
        _ => "dark",
    };
    div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .child(field("Light or dark", crate::ui::segmented("mode", vec![("system", "Like the system".into()), ("light", "Light".into()), ("dark", "Dark".into())], mode, set("appearance.mode"), cx).into_any_element(), cx))
        .child(field("Around the page", crate::ui::segmented("pasteboard", vec![("dark", "Dark grey".into()), ("mid", "Mid grey".into()), ("light", "Light grey".into())], board, set("appearance.pasteboard"), cx).into_any_element(), cx))
        .child(crate::ui::switch("transparency", "See-through panels", a.transparency, |v, _, cx| cx.store().update(cx, |s, cx| s.run("app.setSetting", json!({ "key": "appearance.transparency", "value": v }), cx)), cx))
        .into_any_element()
}

fn account_section(cx: &App) -> AnyElement {
    let t = cx.theme().clone();
    let s = cx.store().read(cx);
    let acc = s.account.clone();
    let signed = acc["signedIn"] == json!(true);
    let server = nori_control::account::server();
    let manage = format!("{server}/account");
    let mut col = div().flex().flex_col().gap(px(14.)).child(div().flex().items_center().gap(px(12.)).child(logo("lsuite", px(36.))).child(div().flex().flex_col().child(div().text_size(px(sz::LG)).font_weight(FontWeight::SEMIBOLD).child("lsuite AI")).child(div().text_size(px(sz::SM)).text_color(t.text_2).child("No setup. Sign in and your agent works — in every lsuite app."))));
    if signed {
        let email = acc["account"]["email"].as_str().unwrap_or("").to_string();
        let plan = acc["plan"].as_str().or(acc["account"]["plan"].as_str()).unwrap_or("").to_string();
        let usage = &acc["usage"];
        let used = usage["used"].as_f64();
        let limit = usage["limit"].as_f64();
        let line = match (used, limit) {
            (Some(u), Some(l)) if l > 0.0 => format!("{} · {:.0} % used{}", if plan.is_empty() { "No plan".into() } else { capital(&plan) }, u / l * 100.0, usage["resetsAt"].as_str().and_then(|r| chrono::DateTime::parse_from_rfc3339(r).ok()).map(|d| format!(" · resets {}", d.format("%-d %b"))).unwrap_or_default()),
            _ => if plan.is_empty() { "No plan yet".into() } else { capital(&plan) },
        };
        col = col
            .child(div().flex().flex_col().gap(px(4.)).p(px(12.)).border_1().border_color(t.line_strong).child(div().font_weight(FontWeight::MEDIUM).child(email)).child(div().font_family(MONO).text_size(px(sz::SM)).text_color(t.text_2).child(line)).when_some(acc["error"].as_str().map(str::to_string), |d, e| d.child(div().text_size(px(sz::SM)).text_color(t.danger).child(e))))
            .child(div().flex().gap(px(8.)).child(Button::new("manage-plan", "Manage plan").with_icon("external-link").on_click(move |_, _, cx| cx.open_url(&manage))).child(Button::new("sign-out", "Sign out").with_icon("log-out").on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("account.signOut", json!({}), cx)))));
    } else {
        col = col
            .child(div().text_size(px(sz::SM)).text_color(t.text_2).child("Signing in opens your browser; pick a plan there (a demo for now: nothing is charged) and press Connect nori. Your own Claude Code, Codex, API keys or Ollama stay free and work without it."))
            .child(div().flex().gap(px(8.)).child(Button::new("sign-in", "Sign in").primary().with_icon("log-in").on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("account.signIn", json!({}), cx)))).child(Button::new("see-plans", "See plans").with_icon("external-link").on_click(move |_, _, cx| cx.open_url(&manage))));
    }
    col.child(div().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).child(format!("Server: {server}"))).into_any_element()
}

/// The line that adds nori's MCP server to Claude Code (`nori-cli mcp-config` has the others).
fn mcp_line(data_dir: &std::path::Path) -> String {
    let mcp = nori_control::discovery::entry(data_dir, None).mcp.map(|p| p.display().to_string()).unwrap_or_else(|| crate::ui::mcp_fallback().into());
    format!("claude mcp add nori -- {} --live", crate::ui::shell_quote(&mcp))
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

fn about_section(cx: &App) -> AnyElement {
    let t = cx.theme();
    div()
        .flex()
        .flex_col()
        .gap(px(12.))
        .child(div().flex().items_center().gap(px(12.)).child(crate::views::home::mark(40., cx)).child(div().flex().flex_col().child(div().text_size(px(sz::XL)).font_weight(FontWeight::SEMIBOLD).child("nori")).child(div().font_family(MONO).text_size(px(sz::SM)).text_color(t.text_2).child(format!("{} beta · {} {}", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH)))))
        .child(div().text_size(px(sz::MD)).child("Pictures, drawings and pages in one app. Part of lsuite: free, open source (MIT), and every action is a command your agent can run."))
        .child(div().flex().gap(px(8.)).child(Button::new("about-site", "lsuite.xyz/nori").with_icon("external-link").on_click(|_, _, cx| cx.open_url("https://lsuite.xyz/nori"))).child(Button::new("about-support", "Support nori").with_icon("external-link").on_click(|_, _, cx| cx.open_url(crate::app::SUPPORT_URL))))
        .child(div().text_size(px(sz::SM)).text_color(t.text_2).child("Fonts: Chakra Petch, Manrope, IBM Plex Mono (SIL OFL). Icons: Lucide (ISC). Logos belong to their owners and only name the apps and services nori works with."))
        .into_any_element()
}

fn shortcuts_sheet(cx: &App) -> AnyElement {
    let t = cx.theme();
    let mut groups: Vec<(&str, Vec<&crate::actions::Shortcut>)> = vec![];
    for s in crate::actions::SHORTCUTS {
        match groups.iter_mut().find(|(g, _)| *g == s.group) {
            Some((_, v)) => v.push(s),
            None => groups.push((s.group, vec![s])),
        }
    }
    div()
        .flex()
        .flex_col()
        .max_h(px(620.))
        .child(header("Keyboard shortcuts", None, cx))
        .child(div().id("shortcuts").flex_1().min_h_0().overflow_y_scroll().p(px(18.)).flex().flex_wrap().gap(px(24.)).children(groups.into_iter().map(|(g, list)| {
            div().w(px(260.)).flex().flex_col().gap(px(4.)).child(caps(g, cx)).children(list.into_iter().map(|s| div().flex().items_center().justify_between().gap(px(8.)).text_size(px(sz::SM)).child(div().text_color(t.text_2).child(s.label)).children(s.hint().map(|h| crate::ui::kbd(h, cx)))))
        })))
        .into_any_element()
}

fn whats_new_sheet(cx: &App) -> AnyElement {
    let notes = nori_control::release_notes::all();
    let md: String = notes.iter().take(3).map(|r| format!("## {} — {}\n\n{}\n\n", r.version, r.date.clone().unwrap_or_default(), r.notes)).collect();
    div().flex().flex_col().max_h(px(620.)).child(header("What's new", None, cx)).child(div().id("whats-new").flex_1().min_h_0().overflow_y_scroll().p(px(18.)).child(crate::ui::markdown::render(&md, cx))).into_any_element()
}

/// Scrim, centred tier-3 panel inside corner brackets; clicking the scrim closes it.
pub fn modal(name: &'static str, width: f32, content: impl IntoElement, window: &Window, cx: &App) -> AnyElement {
    let t = cx.theme().clone();
    let vp = window.viewport_size();
    let w = width.min(f32::from(vp.width) - 40.);
    let panel = div()
        .id("modal")
        .occlude()
        .relative()
        .w(px(w))
        .max_h(px(f32::from(vp.height) - 60.))
        .flex()
        .flex_col()
        .glass(t.glass3)
        .shadow(t.glass_shadow())
        .overflow_hidden()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(content);
    let framed = div().relative().child(panel).child(crate::ui::grain::brackets(14., -9., t.text_3));
    let scrim = div()
        .id("modal-scrim")
        .occlude()
        .absolute()
        .inset_0()
        .bg(t.scrim)
        .flex()
        .justify_center()
        .items_center()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.store().update(cx, |s, cx| s.close_dialog(cx)))
        .child(motion::enter(framed, (name, 1usize), motion::BASE, (0., 10.)));
    deferred(motion::fade(scrim, (name, 0usize), motion::FAST)).with_priority(1).into_any_element()
}

impl Render for Dialogs {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dialog = self.store.read(cx).dialog.clone();
        if let Some(Dialog::Settings { section: Some(sec) }) = &dialog
            && self.section != *sec
            && ["appearance", "agent", "account", "about"].contains(&sec.as_str())
        {
            self.section = sec.clone();
            self.store.update(cx, |s, _| s.dialog = Some(Dialog::Settings { section: None }));
        }
        let el = match dialog {
            Some(Dialog::NewDocument) => Some(modal("new", 460., self.new_document(cx), window, cx)),
            Some(Dialog::Export) => Some(modal("export", 560., self.export(cx), window, cx)),
            Some(Dialog::Settings { .. }) => Some(modal("settings", 780., self.settings(cx), window, cx)),
            Some(Dialog::Plugins { .. }) => Some(modal("plugins", 680., self.plugins(cx), window, cx)),
            Some(Dialog::Filter { id }) => Some(modal("filter", 420., self.filter(&id, cx), window, cx)),
            Some(Dialog::Shortcuts) => Some(modal("shortcuts", 900., shortcuts_sheet(cx), window, cx)),
            Some(Dialog::WhatsNew) => Some(modal("whats-new", 620., whats_new_sheet(cx), window, cx)),
            None => None,
        };
        div().when(el.is_some(), |d| d.absolute().inset_0()).children(el)
    }
}
