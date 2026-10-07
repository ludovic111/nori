//! The root view: the lsuite backdrop, home or editor, and everything that floats above them
//! (dialogs, the context menu, toasts). It also carries out the commands only the window can
//! (`ui.*`, `app.quit`, `app.notify`) for every client of the registry.

use std::sync::Arc;

use gpui::{App, AppContext as _, Context, Entity, ExternalPaths, FocusHandle, Focusable, MouseButton, PathPromptOptions, Render, Subscription, Window, div, prelude::*, px};
use nori_control::{CmdResult, Session, ToastKind, UiCall};
use serde_json::{Value, json};

use crate::actions::*;
use crate::store::{Dialog, GlobalStore, RightTab, Store, StoreEvent, StoreExt, Tool};
use crate::theme::{ActiveTheme, Theme, os_reduces_transparency, size as sz};
use crate::ui::{GlassExt, icon};
use crate::views;

pub const SUPPORT_URL: &str = "https://lsuite.xyz/nori/support";
pub const HELP_URL: &str = "https://github.com/ludovic111/nori/blob/main/docs/README.md";

pub fn init(session: Arc<Session>, cx: &mut App) {
    let settings = session.settings();
    let mode = Theme::mode_for(&settings.appearance.mode, cx.window_appearance());
    cx.set_global(Theme::new(mode, settings.appearance.transparency && !os_reduces_transparency()));
    cx.set_reduce_motion(crate::theme::os_reduces_motion());
    let agent = nori_agent::Host::install(&session);
    let store = cx.new(|cx| Store::new(session, agent, cx));
    cx.set_global(GlobalStore(store));
    crate::actions::bind(cx);
    cx.set_menus(crate::actions::menus());
    cx.on_action(|_: &Quit, cx| {
        if let Some(w)=cx.windows().first().copied() { let _=w.update(cx,|_,window,cx|quit_dialog(window,cx)); }
    });
    cx.on_window_closed(|cx, _| {
        if cx.windows().is_empty() {
            cx.quit();
        }
    })
    .detach();
}

/// At start: the first-run setup, or a file to open.
pub fn welcome(session: &Arc<Session>, open: Option<std::path::PathBuf>, cx: &mut App) {
    let store = cx.store();
    let first = session.settings().onboarding.completed.is_empty() && std::env::var("NORI_NO_SETUP").is_err();
    store.update(cx, |s, cx| {
        if let Some(p) = open {
            s.run("doc.open", json!({ "path": p }), cx);
        } else if first {
            s.open_setup(None, cx);
        }
    });
}

pub fn apply_theme_setting(cx: &mut App) {
    let settings = cx.store().read(cx).settings.clone();
    let mode = Theme::mode_for(&settings.appearance.mode, cx.window_appearance());
    let transparent = settings.appearance.transparency && !os_reduces_transparency();
    let t = cx.global::<Theme>();
    if t.mode != mode || t.transparent != transparent {
        cx.set_global(Theme::new(mode, transparent));
        cx.refresh_windows();
    }
}

/// Picks a file and opens it as the document.
pub fn open_dialog(cx: &mut App) {
    let rx = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Open".into()) });
    let store = cx.store();
    cx.spawn(async move |cx| {
        let Ok(Ok(Some(paths))) = rx.await else { return };
        if let Some(p) = paths.into_iter().next() {
            store.update(cx, |s, cx| s.run("doc.open", json!({ "path": p }), cx));
        }
    })
    .detach();
}

/// Picks a file and places it on the page as a layer.
pub fn place_dialog(cx: &mut App) {
    let rx = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: true, prompt: Some("Place".into()) });
    let store = cx.store();
    cx.spawn(async move |cx| {
        let Ok(Ok(Some(paths))) = rx.await else { return };
        store.update(cx, |s, cx| {
            for p in paths {
                s.run("layer.place", json!({ "path": p }), cx);
            }
        });
    })
    .detach();
}

/// Saves (asking where the first time).
pub fn save(as_new: bool, cx: &mut App) {
    let store = cx.store();
    let s = store.read(cx);
    let Some(doc) = s.doc.clone() else { return };
    if !as_new && s.saved_to.is_some() {
        store.update(cx, |s, cx| s.run_then("doc.save", json!({}), cx, |s, v, cx| s.flash(format!("Saved {}", short(v["saved"].as_str().unwrap_or(""))), cx)));
        return;
    }
    let dir = s.saved_to.as_ref().or(s.opened_from.as_ref()).and_then(|p| p.parent().map(|d| d.to_path_buf())).or_else(dirs::document_dir).unwrap_or_else(std::env::temp_dir);
    let name = format!("{}.nori", if doc.name.is_empty() { "Untitled" } else { &doc.name });
    let rx = cx.prompt_for_new_path(&dir, Some(&name));
    cx.spawn(async move |cx| {
        let Ok(Ok(Some(path))) = rx.await else { return };
        store.update(cx, |s, cx| s.run_then("doc.save", json!({ "path": path }), cx, |s, v, cx| s.flash(format!("Saved {}", short(v["saved"].as_str().unwrap_or(""))), cx)));
    })
    .detach();
}

/// The file name of a path.
pub fn short(p: &str) -> String {
    std::path::Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.to_string())
}

/// Undoes or redoes, then says what ("Undid brush stroke").
pub fn undo_redo(s: &mut Store, undo: bool, cx: &mut Context<Store>) {
    let name = if undo { "history.undo" } else { "history.redo" };
    s.run_then(name, json!({}), cx, move |s, v, cx| {
        let what = step_name(v["step"].as_str().unwrap_or(""));
        let whose = match v["source"].as_str() {
            Some("agent") => "the agent's ",
            Some("mcp" | "cli") => "a script's ",
            _ => "",
        };
        s.flash(format!("{} {whose}{what}", if undo { "Undid" } else { "Redid" }), cx);
    });
}

/// The command that made an undo step, in words.
pub fn step_name(command: &str) -> String {
    match command {
        "raster.stroke" => "brush stroke",
        "raster.fill" => "fill",
        "raster.gradient" => "gradient",
        "raster.clear" => "clear",
        "layer.add" => "new layer",
        "layer.addAdjustment" | "layer.addLut" => "new adjustment",
        "layer.setAdjustment" => "adjustment change",
        "layer.update" => "layer change",
        "layer.move" => "move",
        "layer.transform" => "transform",
        "layer.delete" => "layer deletion",
        "layer.duplicate" => "duplicate",
        "layer.merge" => "merge",
        "layer.group" => "group",
        "layer.ungroup" => "ungroup",
        "layer.reorder" => "layer order",
        "layer.place" => "placed file",
        "layer.select" => "layer selection",
        "text.add" => "new text",
        "text.update" => "text change",
        "vector.addShape" => "new shape",
        "vector.addPath" => "new path",
        "vector.update" | "vector.editNode" | "vector.setGeometry" => "shape change",
        "vector.combine" => "path operation",
        "filter.apply" | "filter.adjust" => "filter",
        "doc.crop" => "crop",
        "doc.resize" => "image size",
        "doc.resizeCanvas" => "canvas size",
        "doc.rotate" => "rotation",
        "doc.batch" | "batch" => "batch of edits",
        c if c.starts_with("select.") => "selection",
        c if c.starts_with("page.") => "page change",
        "" => "last step",
        c => return c.rsplit('.').next().unwrap_or(c).to_string(),
    }
    .to_string()
}

pub struct Workspace {
    store: Entity<Store>,
    focus: FocusHandle,
    home: Entity<views::home::Home>,
    editor: Entity<views::editor::Editor>,
    dialogs: Entity<views::dialogs::Dialogs>,
    onboarding: Entity<views::onboarding::Onboarding>,
    _subs: Vec<Subscription>,
}

impl Workspace {
    #[cfg(test)]
    pub fn editor(&self) -> Entity<views::editor::Editor> {
        self.editor.clone()
    }

    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = cx.store();
        window.on_window_should_close(cx,|window,cx|{
            if cx.store().read(cx).session.tabs().iter().any(|t|t["dirty"]==true) {quit_dialog(window,cx);false}else{true}
        });
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let home = cx.new(|cx| views::home::Home::new(window, cx));
        let editor = cx.new(|cx| views::editor::Editor::new(window, cx));
        let dialogs = cx.new(|cx| views::dialogs::Dialogs::new(window, cx));
        let onboarding = cx.new(|cx| views::onboarding::Onboarding::new(window, cx));
        let mut subs = vec![cx.observe(&store, |_, _, cx| cx.notify())];
        subs.push(cx.observe_window_appearance(window, |_, _, cx| apply_theme_setting(cx)));
        subs.push(cx.subscribe_in(&store, window, |ws: &mut Self, _, e: &StoreEvent, window, cx| {
            if let StoreEvent::SetupClosed = e {
                window.focus(&ws.focus, cx)
            }
        }));
        // Commands only the window can do, from every client of the registry.
        let mut calls = store.read(cx).session.attach_ui();
        cx.spawn_in(window, async move |this, cx| {
            use futures::StreamExt;
            while let Some(call) = calls.next().await {
                let UiCall { command, params, reply } = call;
                let result = this.update_in(cx, |ws, window, cx| ws.ui_command(&command, params, window, cx)).unwrap_or_else(|_| Err("the window has closed".into()));
                let _ = reply.send(result);
            }
        })
        .detach();
        store.update(cx, |s, cx| s.sync_ui(cx));
        Self { store, focus, home, editor, dialogs, onboarding, _subs: subs }
    }

    fn ui_command(&mut self, command: &str, params: Value, window: &mut Window, cx: &mut Context<Self>) -> CmdResult {
        let store = self.store.clone();
        match command {
            "ui.setTool" => {
                let tool = params["tool"].as_str().and_then(Tool::from_id).ok_or("unknown tool")?;
                store.update(cx, |s, cx| s.set_tool(tool, cx));
                Ok(json!({ "tool": tool.id() }))
            }
            "ui.zoom" => {
                if store.read(cx).doc.is_none() {
                    return Err(nori_control::session::NO_DOCUMENT.into());
                }
                let canvas = self.editor.read(cx).canvas.clone();
                if params["fit"].as_bool() == Some(true) {
                    canvas.update(cx, |c, cx| c.fit(cx));
                } else if let Some(z) = params["zoom"].as_f64() {
                    canvas.update(cx, |c, cx| c.set_zoom(z as f32, cx));
                }
                Ok(json!({ "zoom": canvas.read(cx).zoom }))
            }
            "ui.showPanel" => {
                let panel = params["panel"].as_str().unwrap_or("");
                let open = params["open"].as_bool() != Some(false);
                store.update(cx, |s, cx| match panel {
                    "agent" => s.set_agent_open(open, cx),
                    "settings" | "account" if open => s.open_dialog(Dialog::Settings { section: params["section"].as_str().map(str::to_string).or((panel == "account").then(|| "account".to_string())) }, cx),
                    "export" if open => s.open_dialog(Dialog::Export, cx),
                    "plugins" if open => s.open_dialog(Dialog::Plugins { tab: None }, cx),
                    "shortcuts" if open => s.open_dialog(Dialog::Shortcuts, cx),
                    "whatsNew" if open => s.open_dialog(Dialog::WhatsNew, cx),
                    "onboarding" if open => s.open_setup(params["section"].as_str().map(str::to_string), cx),
                    "onboarding" => s.close_setup(cx),
                    "pages" => {
                        s.right = RightTab::Pages;
                        cx.notify();
                    }
                    "layers" => {
                        s.right = RightTab::Layers;
                        cx.notify();
                    }
                    "home" => s.run("doc.close", json!({}), cx),
                    _ if !open => s.close_dialog(cx),
                    _ => {}
                });
                store.update(cx, |s, cx| s.sync_ui(cx));
                Ok(json!({ "panel": panel, "open": open }))
            }
            "ui.action" => {
                let name = params["action"].as_str().unwrap_or("");
                let action = cx.build_action(&format!("nori::{name}"), None).map_err(|e| format!("The window has no action `{name}`: {e}"))?;
                window.dispatch_action(action, cx);
                Ok(json!({ "action": name }))
            }
            "ui.screenshot" => Err("ui.screenshot works on macOS only for now.".into()),
            "app.restart" => {
                let session = &store.read(cx).session;
                session.save_all()?;
                nori_control::update::restart().map_err(|e| e.to_string())?;
                cx.quit();
                Ok(json!({ "restarting": true }))
            }
            "app.quit" => {
                store.read(cx).session.save_all()?;
                cx.quit();
                Ok(json!({ "quitting": true }))
            }
            "app.notify" => {
                let kind = match params["kind"].as_str() {
                    Some("error") => ToastKind::Error,
                    Some("success") => ToastKind::Success,
                    _ => ToastKind::Info,
                };
                let text = params["text"].as_str().unwrap_or("").to_string();
                store.update(cx, |s, cx| s.toast(kind, text, cx));
                Ok(json!({ "shown": true }))
            }
            other => Err(format!("the window doesn't handle `{other}`")),
        }
    }

    fn run(&self, name: &str, params: Value, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| s.run(name, params, cx));
    }

    fn has_doc(&self, cx: &App) -> bool {
        self.store.read(cx).doc.is_some()
    }

    fn active(&self, cx: &App) -> Option<String> {
        self.store.read(cx).active()
    }

    fn tool(&mut self, tool: Tool, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| {
            // Pressing the key again switches between tools that share it.
            let t = match (s.tool, tool) {
                (Tool::Select, Tool::Select) => Tool::EllipseSelect,
                (Tool::EllipseSelect, Tool::Select) => Tool::Select,
                _ => tool,
            };
            s.set_tool(t, cx)
        });
    }

    fn zoom(&self, by: f32, to: Option<f32>, cx: &mut Context<Self>) {
        self.store.update(cx, |_, cx| cx.emit(StoreEvent::Zoom { by, to }));
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let store = self.store.read(cx);
        let has_doc = store.doc.is_some();
        let dropping = store.dropping && cx.has_active_drag();
        let menu = store.menu.clone();
        let toasts = store.toasts.clone();
        let setup = store.setup.is_some();
        let modal = store.dialog.is_some() || setup;
        match &store.doc {
            Some(d) => window.set_window_title(&format!("{}{} — nori", d.name, if store.dirty { " •" } else { "" })),
            None => window.set_window_title("nori"),
        }
        let bg = if t.transparent { t.bg.opacity(if t.is_dark() { 0.95 } else { 0.93 }) } else { t.bg };
        div()
            .key_context(if modal { "Workspace Modal" } else { "Workspace" })
            .track_focus(&self.focus)
            .on_action(cx.listener(|ws, _: &Undo, _, cx| {
                if ws.has_doc(cx) {
                    ws.store.update(cx, |s, cx| undo_redo(s, true, cx));
                }
            }))
            .on_action(cx.listener(|ws, _: &Redo, _, cx| {
                if ws.has_doc(cx) {
                    ws.store.update(cx, |s, cx| undo_redo(s, false, cx));
                }
            }))
            .on_action(cx.listener(|ws, _: &NewDocument, _, cx| ws.store.update(cx, |s, cx| s.open_dialog(Dialog::NewDocument, cx))))
            .on_action(cx.listener(|_, _: &OpenDocument, _, cx| open_dialog(cx)))
            .on_action(cx.listener(|ws, _: &PlaceFile, _, cx| {
                if ws.has_doc(cx) {
                    place_dialog(cx)
                }
            }))
            .on_action(cx.listener(|_, _: &Save, _, cx| save(false, cx)))
            .on_action(cx.listener(|_, _: &SaveAs, _, cx| save(true, cx)))
            .on_action(cx.listener(|ws, _: &Export, _, cx| {
                if ws.has_doc(cx) {
                    ws.store.update(cx, |s, cx| s.open_dialog(Dialog::Export, cx))
                }
            }))
            .on_action(cx.listener(|_, _: &CloseDocument, window, cx| close_document(window,cx)))
            .on_action(cx.listener(|ws, _: &OpenSettings, _, cx| ws.store.update(cx, |s, cx| s.open_dialog(Dialog::Settings { section: None }, cx))))
            .on_action(cx.listener(|ws, _: &About, _, cx| ws.store.update(cx, |s, cx| s.open_dialog(Dialog::Settings { section: Some("about".into()) }, cx))))
            .on_action(cx.listener(|ws, _: &OpenPlugins, _, cx| ws.store.update(cx, |s, cx| s.open_dialog(Dialog::Plugins { tab: None }, cx))))
            .on_action(cx.listener(|ws, _: &ShowShortcuts, _, cx| ws.store.update(cx, |s, cx| if s.dialog == Some(Dialog::Shortcuts) { s.close_dialog(cx) } else { s.open_dialog(Dialog::Shortcuts, cx) })))
            .on_action(cx.listener(|ws, _: &WhatsNew, _, cx| ws.store.update(cx, |s, cx| s.open_dialog(Dialog::WhatsNew, cx))))
            .on_action(cx.listener(|ws, _: &SetUpNori, _, cx| ws.store.update(cx, |s, cx| s.open_setup(None, cx))))
            .on_action(cx.listener(|ws, _: &ToggleAgent, _, cx| ws.store.update(cx, |s, cx| s.set_agent_open(!s.agent_open, cx))))
            .on_action(cx.listener(|ws, _: &ToggleTheme, _, cx| {
                let next = if cx.theme().is_dark() { "light" } else { "dark" };
                ws.run("app.setSetting", json!({ "key": "appearance.mode", "value": next }), cx);
            }))
            .on_action(cx.listener(|ws, _: &ZoomIn, _, cx| ws.zoom(1.25, None, cx)))
            .on_action(cx.listener(|ws, _: &ZoomOut, _, cx| ws.zoom(0.8, None, cx)))
            .on_action(cx.listener(|ws, _: &ZoomFit, _, cx| ws.store.update(cx, |_, cx| cx.emit(StoreEvent::FitView))))
            .on_action(cx.listener(|ws, _: &ZoomActual, _, cx| ws.zoom(1.0, Some(1.0), cx)))
            .on_action(cx.listener(|ws, _: &SelectAll, _, cx| ws.run("select.all", json!({}), cx)))
            .on_action(cx.listener(|ws, _: &DeselectAll, _, cx| ws.run("select.none", json!({}), cx)))
            .on_action(cx.listener(|ws, _: &InvertSelection, _, cx| ws.run("select.invert", json!({}), cx)))
            .on_action(cx.listener(|ws, _: &Deselect, _, cx| {
                let s = ws.store.read(cx);
                if s.menu.is_some() {
                    ws.store.update(cx, |s, cx| s.close_menu(cx));
                } else if s.dialog.is_some() {
                    ws.store.update(cx, |s, cx| s.close_dialog(cx));
                } else {
                    let canvas = ws.editor.read(cx).canvas.clone();
                    if !canvas.update(cx, |c, cx| c.cancel(cx)) && ws.has_doc(cx) && ws.store.read(cx).doc().is_some_and(|d| d.selection.is_some()) {
                        ws.run("select.none", json!({}), cx);
                    }
                }
            }))
            .on_action(cx.listener(|ws, _: &Confirm, _, cx| {
                let canvas = ws.editor.read(cx).canvas.clone();
                canvas.update(cx, |c, cx| c.confirm(cx));
            }))
            .on_action(cx.listener(|ws, _: &NewLayer, _, cx| ws.run("layer.add", json!({}), cx)))
            .on_action(cx.listener(|ws, _: &DuplicateLayer, _, cx| ws.run("layer.duplicate", json!({}), cx)))
            .on_action(cx.listener(|ws, _: &DeleteLayer, _, cx| ws.run("layer.delete", json!({}), cx)))
            .on_action(cx.listener(|ws, _: &ClearOrDelete, _, cx| {
                let Some(d) = ws.store.read(cx).doc.clone() else { return };
                let raster = d.active_id().and_then(|id| d.layer(&id).map(|l| l.raster().is_some())).unwrap_or(false);
                if d.selection.is_some() && raster {
                    ws.run("raster.clear", json!({}), cx);
                } else if d.active_id().is_some() {
                    ws.run("layer.delete", json!({}), cx);
                }
            }))
            .on_action(cx.listener(|ws, _: &MergeDown, _, cx| ws.run("layer.merge", json!({}), cx)))
            .on_action(cx.listener(|ws, _: &GroupLayers, _, cx| {
                if let Some(a) = ws.active(cx) {
                    ws.run("layer.group", json!({ "layerIds": [a] }), cx);
                }
            }))
            .on_action(cx.listener(|ws, _: &Ungroup, _, cx| ws.run("layer.ungroup", json!({}), cx)))
            .on_action(cx.listener(|ws, _: &FlattenImage, _, cx| ws.run("layer.flatten", json!({}), cx)))
            .on_action(cx.listener(|ws, _: &SwapColors, _, cx| ws.run("color.set", json!({ "swap": true }), cx)))
            .on_action(cx.listener(|ws, _: &ResetColors, _, cx| ws.run("color.set", json!({ "reset": true }), cx)))
            .on_action(cx.listener(|ws, _: &BrushSmaller, _, cx| {
                let (key, v) = brush_key(ws.store.read(cx), 1.0 / 1.25);
                ws.run("app.setSetting", json!({ "key": key, "value": v }), cx);
            }))
            .on_action(cx.listener(|ws, _: &BrushBigger, _, cx| {
                let (key, v) = brush_key(ws.store.read(cx), 1.25);
                ws.run("app.setSetting", json!({ "key": key, "value": v }), cx);
            }))
            .on_action(cx.listener(|ws, _: &NextPage, _, cx| page_step(ws, 1, cx)))
            .on_action(cx.listener(|ws, _: &PreviousPage, _, cx| page_step(ws, -1, cx)))
            .on_action(cx.listener(|ws, _: &AddPage, _, cx| ws.run("page.add", json!({}), cx)))
            .on_action(cx.listener(|ws, _: &ToolMove, _, cx| ws.tool(Tool::Move, cx)))
            .on_action(cx.listener(|ws, _: &ToolSelect, _, cx| ws.tool(Tool::Select, cx)))
            .on_action(cx.listener(|ws, _: &ToolLasso, _, cx| ws.tool(Tool::Lasso, cx)))
            .on_action(cx.listener(|ws, _: &ToolWand, _, cx| ws.tool(Tool::Wand, cx)))
            .on_action(cx.listener(|ws, _: &ToolCrop, _, cx| ws.tool(Tool::Crop, cx)))
            .on_action(cx.listener(|ws, _: &ToolEyedropper, _, cx| ws.tool(Tool::Eyedropper, cx)))
            .on_action(cx.listener(|ws, _: &ToolBrush, _, cx| ws.tool(Tool::Brush, cx)))
            .on_action(cx.listener(|ws, _: &ToolEraser, _, cx| ws.tool(Tool::Eraser, cx)))
            .on_action(cx.listener(|ws, _: &ToolFill, _, cx| ws.tool(Tool::Fill, cx)))
            .on_action(cx.listener(|ws, _: &ToolGradient, _, cx| ws.tool(Tool::Gradient, cx)))
            .on_action(cx.listener(|ws, _: &ToolPen, _, cx| ws.tool(Tool::Pen, cx)))
            .on_action(cx.listener(|ws, _: &ToolDirect, _, cx| ws.tool(Tool::Direct, cx)))
            .on_action(cx.listener(|ws, _: &ToolShape, _, cx| ws.tool(Tool::Shape, cx)))
            .on_action(cx.listener(|ws, _: &ToolText, _, cx| ws.tool(Tool::Text, cx)))
            .on_action(cx.listener(|ws, _: &ToolFrame, _, cx| ws.tool(Tool::Frame, cx)))
            .on_action(cx.listener(|ws, _: &ToolHand, _, cx| ws.tool(Tool::Hand, cx)))
            .on_action(cx.listener(|ws, _: &ToolZoom, _, cx| ws.tool(Tool::Zoom, cx)))
            .on_action(cx.listener(|_, _: &OpenHelp, _, cx| cx.open_url(HELP_URL)))
            .on_action(cx.listener(|_, _: &OpenSupport, _, cx| cx.open_url(SUPPORT_URL)))
            .on_drop(cx.listener(|ws, paths: &ExternalPaths, _, cx| {
                let list: Vec<String> = paths.paths().iter().map(|p| p.to_string_lossy().into_owned()).collect();
                let has = ws.has_doc(cx);
                ws.store.update(cx, |s, cx| {
                    s.dropping = false;
                    for (i, p) in list.iter().enumerate() {
                        if has || i > 0 {
                            s.run("layer.place", json!({ "path": p }), cx);
                        } else {
                            s.run("doc.open", json!({ "path": p }), cx);
                        }
                    }
                    cx.notify();
                });
            }))
            .on_drag_move::<ExternalPaths>(cx.listener(|ws, _, _, cx| {
                ws.store.update(cx, |s, cx| {
                    if !s.dropping {
                        s.dropping = true;
                        cx.notify();
                    }
                })
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(|ws, _, window, cx| {
                ws.store.update(cx, |s, cx| s.close_menu(cx));
                if !window.default_prevented() {
                    window.focus(&ws.focus, cx);
                }
            }))
            .relative()
            .size_full()
            .font_family(crate::theme::SANS)
            .text_size(px(sz::BASE))
            .text_color(t.text)
            .child(crate::ui::grain::backdrop(bg, window, cx))
            .child(if setup {
                self.onboarding.clone().into_any_element()
            } else if has_doc {
                self.editor.clone().into_any_element()
            } else {
                self.home.clone().into_any_element()
            })
            .child(self.dialogs.clone())
            .when_some(menu, |d, m| d.child(views::overlays::context_menu(m, window, cx)))
            .child(views::overlays::toasts(toasts, cx))
            .when(dropping, |d| {
                d.child(
                    div().absolute().inset(px(6.)).border_2().border_dashed().border_color(t.accent).flex().justify_center().child(
                        div()
                            .mt(px(64.))
                            .h(px(36.))
                            .glass(t.glass2)
                            .shadow(t.glass_shadow())
                            .px(px(16.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(icon("image").text_color(t.accent_text))
                            .child(if has_doc { "Drop to place it on the page" } else { "Drop to open" }),
                    ),
                )
            })
    }
}

fn brush_key(s: &Store, k: f64) -> (&'static str, f64) {
    let (key, size) = if s.tool == Tool::Eraser { ("tools.eraser.size", s.settings.tools.eraser.size) } else { ("tools.brush.size", s.settings.tools.brush.size) };
    (key, ((size as f64) * k).clamp(1.0, 2000.0).round())
}

fn page_step(ws: &mut Workspace, by: i64, cx: &mut Context<Workspace>) {
    let Some(d) = ws.store.read(cx).doc.clone() else { return };
    let i = d.active_index() as i64 + by;
    if i >= 0 && (i as usize) < d.pages.len() {
        ws.run("page.select", json!({ "page": (i + 1).to_string() }), cx);
    }
}

pub fn edit_smart_source(id:String,cx:&mut App) {
    let path=std::env::temp_dir().join(format!("nori-source-{}.nori",uuid::Uuid::new_v4()));
    cx.store().update(cx,|s,cx|s.run_then("layer.extractSmartObject",json!({"layerId":id,"path":path}),cx,move |s,v,cx|s.run("doc.open",json!({"path":v["path"]}),cx)));
}
pub fn replace_smart_source(id:String,cx:&mut App) {
    let store=cx.store();let doc_id=store.read(cx).session.snapshot().ok().map(|(_,_,id)|id);
    let rx=cx.prompt_for_paths(PathPromptOptions{files:true,directories:false,multiple:false,prompt:Some("Replace contents".into())});
    cx.spawn(async move |cx|{
        let Ok(Ok(Some(paths)))=rx.await else{return};let Some(path)=paths.first() else{return};
        store.update(cx,|s,cx|{
            if s.session.snapshot().ok().map(|(_,_,id)|id)!=doc_id {s.flash("Return to the original document to replace its contents.",cx);return;}
            s.run("layer.replaceSmartObject",json!({"layerId":id,"path":path}),cx);
        });
    }).detach();
}

fn quit_dialog(window:&mut Window,cx:&mut App) {
    let store=cx.store();let session=store.read(cx).session.clone();
    if !session.tabs().iter().any(|t|t["dirty"]==true){cx.quit();return;}
    let rx=window.prompt(gpui::PromptLevel::Warning,"Save changes before quitting?",Some("There are unsaved documents. Save all requires each untitled tab to have a file name."),&["Cancel","Save all and quit","Discard all and quit"],cx);
    cx.spawn(async move |cx|{
        match rx.await {
            Ok(1)=>match session.save_all(){Ok(())=>{cx.update(|cx|cx.quit());},Err(e)=>{store.update(cx,|s,cx|s.flash(e,cx));}},
            Ok(2)=>{cx.update(|cx|cx.quit());},_=>{}
        }
    }).detach();
}
pub fn close_document(window:&mut Window,cx:&mut App) {
    let store=cx.store();let s=store.read(cx);let Some((_,_,id))=s.session.snapshot().ok() else{return};
    if !s.session.tabs().iter().any(|t|t["id"]==id && t["dirty"]==true) {store.update(cx,|s,cx|s.run("doc.close",json!({}),cx));return;}
    let rx=window.prompt(gpui::PromptLevel::Warning,"Save changes before closing?",None,&["Cancel","Save","Discard"],cx);
    cx.spawn(async move |cx|{
        let Ok(answer)=rx.await else{return};
        store.update(cx,|s,cx|{
            if s.session.snapshot().ok().map(|(_,_,i)|i)!=Some(id){return;}
            match answer {
                1=>{ if s.saved_to.is_some(){s.run_then("doc.save",json!({}),cx,move |s,_,cx|{if s.session.snapshot().ok().map(|(_,_,i)|i)==Some(id){s.run("doc.close",json!({}),cx)}});}else{ s.flash("Save this untitled document with File › Save, then close it.",cx); }},
                2=>s.run("doc.close",json!({"discard":true}),cx),_=>{}
            }
        });
    }).detach();
}
