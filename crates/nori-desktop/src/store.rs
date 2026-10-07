//! The window's view of the session.
//!
//! The window is one client of the command registry, never a privileged one: every change goes
//! through [`Store::run`] (`registry::call` with `Source::Window`), and the document shown is
//! whatever the session holds after the change. The store adds view state (the tool, panels,
//! dialogs, toasts) and mirrors what the session announces on its event stream.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{App, Context, Entity, EventEmitter, Global, Pixels, Point, SharedString, Window};
use nori_control::session::Colors;
use nori_control::{CmdResult, CommandRecord, Event, Session, Settings, Source, ToastKind, UiState};
use nori_core::{Document, StepInfo};
use serde_json::{Value, json};

/// The tools, as the tool column groups them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Move,
    Select,
    EllipseSelect,
    Lasso,
    Wand,
    Crop,
    Eyedropper,
    Brush,
    Eraser,
    Fill,
    Gradient,
    Pen,
    Direct,
    Shape,
    Text,
    Frame,
    Hand,
    Zoom,
}

impl Tool {
    pub const ALL: [Tool; 18] = [
        Tool::Move,
        Tool::Select,
        Tool::EllipseSelect,
        Tool::Lasso,
        Tool::Wand,
        Tool::Crop,
        Tool::Eyedropper,
        Tool::Brush,
        Tool::Eraser,
        Tool::Fill,
        Tool::Gradient,
        Tool::Pen,
        Tool::Direct,
        Tool::Shape,
        Tool::Text,
        Tool::Frame,
        Tool::Hand,
        Tool::Zoom,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Tool::Move => "move",
            Tool::Select => "select",
            Tool::EllipseSelect => "ellipseSelect",
            Tool::Lasso => "lasso",
            Tool::Wand => "wand",
            Tool::Crop => "crop",
            Tool::Eyedropper => "eyedropper",
            Tool::Brush => "brush",
            Tool::Eraser => "eraser",
            Tool::Fill => "fill",
            Tool::Gradient => "gradient",
            Tool::Pen => "pen",
            Tool::Direct => "direct",
            Tool::Shape => "shape",
            Tool::Text => "text",
            Tool::Frame => "frame",
            Tool::Hand => "hand",
            Tool::Zoom => "zoom",
        }
    }

    pub fn from_id(id: &str) -> Option<Tool> {
        Tool::ALL.into_iter().find(|t| t.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Tool::Move => "Move",
            Tool::Select => "Rectangle Select",
            Tool::EllipseSelect => "Ellipse Select",
            Tool::Lasso => "Lasso",
            Tool::Wand => "Magic Wand",
            Tool::Crop => "Crop",
            Tool::Eyedropper => "Eyedropper",
            Tool::Brush => "Brush",
            Tool::Eraser => "Eraser",
            Tool::Fill => "Paint Bucket",
            Tool::Gradient => "Gradient",
            Tool::Pen => "Pen",
            Tool::Direct => "Direct Selection",
            Tool::Shape => "Shape",
            Tool::Text => "Type",
            Tool::Frame => "Text Frame",
            Tool::Hand => "Hand",
            Tool::Zoom => "Zoom",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Tool::Move => "move",
            Tool::Select => "square-dashed",
            Tool::EllipseSelect => "circle-dashed",
            Tool::Lasso => "lasso",
            Tool::Wand => "wand",
            Tool::Crop => "crop",
            Tool::Eyedropper => "pipette",
            Tool::Brush => "brush",
            Tool::Eraser => "eraser",
            Tool::Fill => "paint-bucket",
            Tool::Gradient => "blend",
            Tool::Pen => "pen-tool",
            Tool::Direct => "square-mouse-pointer",
            Tool::Shape => "shapes",
            Tool::Text => "type",
            Tool::Frame => "frame",
            Tool::Hand => "hand",
            Tool::Zoom => "zoom-in",
        }
    }

    /// The key that picks it (as other editors use).
    pub fn key(self) -> &'static str {
        match self {
            Tool::Move => "v",
            Tool::Select | Tool::EllipseSelect => "m",
            Tool::Lasso => "l",
            Tool::Wand => "w",
            Tool::Crop => "c",
            Tool::Eyedropper => "i",
            Tool::Brush => "b",
            Tool::Eraser => "e",
            Tool::Fill => "g",
            Tool::Gradient => "shift-g",
            Tool::Pen => "p",
            Tool::Direct => "a",
            Tool::Shape => "u",
            Tool::Text => "t",
            Tool::Frame => "shift-t",
            Tool::Hand => "h",
            Tool::Zoom => "z",
        }
    }
}

/// What the shape tool draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    Rect,
    Ellipse,
    Polygon,
    Star,
    Line,
}

impl ShapeKind {
    pub const ALL: [ShapeKind; 5] = [ShapeKind::Rect, ShapeKind::Ellipse, ShapeKind::Polygon, ShapeKind::Star, ShapeKind::Line];

    pub fn id(self) -> &'static str {
        match self {
            ShapeKind::Rect => "rect",
            ShapeKind::Ellipse => "ellipse",
            ShapeKind::Polygon => "polygon",
            ShapeKind::Star => "star",
            ShapeKind::Line => "line",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ShapeKind::Rect => "Rectangle",
            ShapeKind::Ellipse => "Ellipse",
            ShapeKind::Polygon => "Polygon",
            ShapeKind::Star => "Star",
            ShapeKind::Line => "Line",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            ShapeKind::Rect => "square",
            ShapeKind::Ellipse => "circle",
            ShapeKind::Polygon => "hexagon",
            ShapeKind::Star => "star",
            ShapeKind::Line => "slash",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Dialog {
    Settings { section: Option<String> },
    Export,
    NewDocument,
    Plugins { tab: Option<String> },
    Shortcuts,
    WhatsNew,
    /// A filter with its parameters before it runs.
    Filter { id: String },
}

impl Dialog {
    pub fn name(&self) -> &'static str {
        match self {
            Dialog::Settings { .. } => "settings",
            Dialog::Export => "export",
            Dialog::NewDocument => "new",
            Dialog::Plugins { .. } => "plugins",
            Dialog::Shortcuts => "shortcuts",
            Dialog::WhatsNew => "whatsNew",
            Dialog::Filter { .. } => "filter",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub id: u64,
    pub kind: ToastKind,
    pub text: SharedString,
    pub flash: bool,
    pub action: Option<(SharedString, Dialog)>,
}

#[derive(Clone)]
pub enum MenuEntry {
    Item(MenuItem),
    Separator,
}

pub type MenuAction = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(Clone)]
pub struct MenuItem {
    pub label: SharedString,
    pub icon: Option<&'static str>,
    pub logo: Option<&'static str>,
    pub shortcut: Option<SharedString>,
    pub danger: bool,
    pub disabled: bool,
    pub checked: bool,
    pub action: MenuAction,
}

impl MenuItem {
    pub fn new(label: impl Into<SharedString>, action: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        Self { label: label.into(), icon: None, logo: None, shortcut: None, danger: false, disabled: false, checked: false, action: Rc::new(action) }
    }
    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn logo(mut self, id: &'static str) -> Self {
        self.logo = Some(id);
        self
    }
    pub fn shortcut(mut self, s: impl Into<SharedString>) -> Self {
        self.shortcut = Some(s.into());
        self
    }
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }
    pub fn checked(mut self, c: bool) -> Self {
        self.checked = c;
        self
    }
    pub fn entry(self) -> MenuEntry {
        MenuEntry::Item(self)
    }
}

#[derive(Clone)]
pub struct ContextMenu {
    pub position: Point<Pixels>,
    pub entries: Vec<MenuEntry>,
}

/// The right column's tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RightTab {
    Layers,
    Pages,
}

#[derive(Clone, Debug)]
pub enum StoreEvent {
    /// The canvas should fit the page in view.
    FitView,
    /// Zoom by a factor about the view's centre (or to a level when `to` is set).
    Zoom { by: f32, to: Option<f32> },
    SetupClosed,
    /// Focus the text editor in the properties (a new text layer was made).
    EditText,
}

pub struct Store {
    pub session: Arc<Session>,
    pub agent: Arc<nori_agent::Host>,
    pub doc: Option<Arc<Document>>,
    pub doc_id: Option<u64>,
    pub revision: u64,
    pub undo: Vec<StepInfo>,
    pub redo: Vec<StepInfo>,
    pub saved_to: Option<PathBuf>,
    pub opened_from: Option<PathBuf>,
    pub dirty: bool,
    pub settings: Settings,
    pub colors: Colors,
    pub tool: Tool,
    pub shape: ShapeKind,
    /// How a new selection combines (replace, add, subtract, intersect).
    pub combine: &'static str,
    pub zoom: f32,
    pub dialog: Option<Dialog>,
    pub setup: Option<String>,
    pub menu: Option<ContextMenu>,
    pub toasts: Vec<Toast>,
    pub agent_open: bool,
    pub right: RightTab,
    pub account: Value,
    pub plugins: Value,
    pub commands: Vec<CommandRecord>,
    pub dropping: bool,
    /// Work in progress in the background (export, filter, plugin build): id → label.
    pub progress: Vec<(String, String)>,
    next_toast: u64,
    _pump: gpui::Task<()>,
}

impl EventEmitter<StoreEvent> for Store {}

pub struct GlobalStore(pub Entity<Store>);
impl Global for GlobalStore {}

pub trait StoreExt {
    fn store(&self) -> Entity<Store>;
}

impl StoreExt for App {
    fn store(&self) -> Entity<Store> {
        self.global::<GlobalStore>().0.clone()
    }
}

impl Store {
    pub fn new(session: Arc<Session>, agent: Arc<nori_agent::Host>, cx: &mut Context<Self>) -> Self {
        let mut rx = session.subscribe();
        let pump = cx.spawn(async move |this, cx| {
            loop {
                let event = match rx.recv().await {
                    Ok(e) => e,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        if this.update(cx, |s, cx| s.refresh_all(cx)).is_err() {
                            break;
                        }
                        continue;
                    }
                    Err(_) => break,
                };
                if this.update(cx, |s, cx| s.on_event(event, cx)).is_err() {
                    break;
                }
            }
        });
        let settings = session.settings();
        let colors = *session.colors.read();
        let mut s = Self {
            session: session.clone(),
            agent,
            doc: None,
            doc_id: None,
            revision: 0,
            undo: vec![],
            redo: vec![],
            saved_to: None,
            opened_from: None,
            dirty: false,
            settings,
            colors,
            tool: Tool::Move,
            shape: ShapeKind::Rect,
            combine: "replace",
            zoom: 1.0,
            dialog: None,
            setup: None,
            menu: None,
            toasts: vec![],
            agent_open: false,
            right: RightTab::Layers,
            account: json!({ "signedIn": nori_control::account::read().is_some() }),
            plugins: Value::Null,
            commands: vec![],
            dropping: false,
            progress: vec![],
            next_toast: 1,
            _pump: pump,
        };
        s.refresh_doc();
        s.refresh_plugins();
        s.refresh_account(cx);
        s
    }

    // ---- commands ---------------------------------------------------------------------------

    /// Runs a registry command as the window. Errors become toasts.
    pub fn run(&mut self, name: &str, params: Value, cx: &mut Context<Self>) {
        self.run_then(name, params, cx, |_, _, _| {});
    }

    pub fn run_then(&mut self, name: &str, params: Value, cx: &mut Context<Self>, then: impl FnOnce(&mut Self, Value, &mut Context<Self>) + 'static) {
        let task = self.call(name, params, cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |s, cx| match result {
                Ok(v) => then(s, v, cx),
                Err(e) => s.error(e, cx),
            })
            .ok();
        })
        .detach();
    }

    pub fn call(&self, name: &str, params: Value, cx: &mut Context<Self>) -> gpui::Task<CmdResult> {
        let session = self.session.clone();
        let name = name.to_string();
        let task = gpui_tokio::Tokio::spawn(cx, async move { nori_control::call(&session, Source::Window, &name, params).await });
        cx.spawn(async move |this, cx| {
            let result = task.await.unwrap_or_else(|e| Err(format!("the command stopped: {e}")));
            this.update(cx, |s, cx| {
                s.refresh_doc();
                cx.notify();
            })
            .ok();
            result
        })
    }

    // ---- events -----------------------------------------------------------------------------

    fn on_event(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::DocChanged { .. } => self.refresh_doc(),
            Event::DocSwitched { .. } => {
                self.refresh_doc();
                self.menu = None;
                cx.emit(StoreEvent::FitView);
            }
            Event::Toast { kind, text } => self.toast(kind, text, cx),
            Event::Command { record } => {
                if record.source != Source::Window {
                    self.commands.push(record);
                    if self.commands.len() > 500 {
                        self.commands.remove(0);
                    }
                }
            }
            Event::SettingsChanged => {
                self.settings = self.session.settings();
                cx.defer(crate::app::apply_theme_setting);
            }
            Event::ToolsChanged => self.colors = *self.session.colors.read(),
            Event::AccountChanged => self.refresh_account(cx),
            Event::PluginsChanged => self.refresh_plugins(),
            Event::Progress { id, label, done, error } => {
                self.progress.retain(|(i, _)| *i != id);
                if !done {
                    self.progress.push((id, label));
                } else if let Some(e) = error {
                    self.error(e, cx);
                }
            }
        }
        self.sync_ui(cx);
        cx.notify();
    }

    fn refresh_all(&mut self, cx: &mut Context<Self>) {
        self.refresh_doc();
        self.settings = self.session.settings();
        self.colors = *self.session.colors.read();
        self.refresh_plugins();
        cx.notify();
    }

    pub fn refresh_doc(&mut self) {
        match self.session.snapshot() {
            Ok((d, rev, id)) => {
                if self.doc_id != Some(id) || self.revision != rev {
                    self.doc = Some(Arc::new(d));
                    self.revision = rev;
                    self.doc_id = Some(id);
                }
                if let Ok((u, r)) = self.session.read(|ed| (ed.undo_steps(), ed.redo_steps())) {
                    self.undo = u;
                    self.redo = r;
                }
                if let Some((p, src, dirty)) = self.session.paths() {
                    self.saved_to = p;
                    self.opened_from = src;
                    self.dirty = dirty;
                }
            }
            Err(_) => {
                self.doc = None;
                self.doc_id = None;
                self.undo.clear();
                self.redo.clear();
                self.saved_to = None;
                self.opened_from = None;
                self.dirty = false;
            }
        }
    }

    pub fn refresh_plugins(&mut self) {
        self.plugins = nori_control::commands::plugin::list(&self.session);
    }

    /// The account file now, then the server (plan, allowance) in the background.
    pub fn refresh_account(&mut self, cx: &mut Context<Self>) {
        let signed = nori_control::account::read();
        self.account = match &signed {
            Some(a) => json!({ "signedIn": true, "account": a.public(), "plan": a.plan }),
            None => json!({ "signedIn": false }),
        };
        if signed.is_some() {
            let task = gpui_tokio::Tokio::spawn(cx, nori_control::commands::account::status(true));
            cx.spawn(async move |this, cx| {
                if let Ok(v) = task.await {
                    this.update(cx, |s, cx| {
                        s.account = v;
                        cx.notify();
                    })
                    .ok();
                }
            })
            .detach();
        }
    }

    /// Tells the session what the window shows (`ui.state`).
    pub fn sync_ui(&self, cx: &App) {
        let mut open = vec![];
        if self.agent_open {
            open.push("agent".to_string());
        }
        if let Some(d) = &self.dialog {
            open.push(d.name().to_string());
        }
        if self.setup.is_some() {
            open.push("onboarding".to_string());
        }
        let theme = if crate::theme::ActiveTheme::theme(cx).is_dark() { "dark" } else { "light" };
        self.session.set_ui_state(UiState {
            screen: if self.doc.is_some() { "editor".into() } else { "home".into() },
            tool: self.tool.id().into(),
            zoom: self.zoom as f64,
            open,
            theme: theme.into(),
            panel: match self.right {
                RightTab::Layers => "layers".into(),
                RightTab::Pages => "pages".into(),
            },
        });
    }

    // ---- toasts -----------------------------------------------------------------------------

    pub fn toast(&mut self, kind: ToastKind, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.push(Toast { id: 0, kind, text: text.into(), flash: false, action: None }, cx);
    }

    /// A passing status ("Undid brush stroke"): the next one replaces it.
    pub fn flash(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.push(Toast { id: 0, kind: ToastKind::Info, text: text.into(), flash: true, action: None }, cx);
    }

    fn push(&mut self, mut toast: Toast, cx: &mut Context<Self>) {
        toast.id = self.next_toast;
        self.next_toast += 1;
        if toast.flash {
            self.toasts.retain(|t| !t.flash);
        }
        let id = toast.id;
        let secs = if toast.kind == ToastKind::Error { 8 } else if toast.flash { 2 } else { 4 };
        self.toasts.push(toast);
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(secs)).await;
            this.update(cx, |s, cx| s.dismiss(id, cx)).ok();
        })
        .detach();
        cx.notify();
    }

    pub fn error(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.toast(ToastKind::Error, text, cx);
    }

    pub fn dismiss(&mut self, id: u64, cx: &mut Context<Self>) {
        self.toasts.retain(|t| t.id != id);
        cx.notify();
    }

    // ---- view state -------------------------------------------------------------------------

    pub fn set_tool(&mut self, tool: Tool, cx: &mut Context<Self>) {
        if self.tool != tool {
            self.tool = tool;
            self.sync_ui(cx);
            cx.notify();
        }
    }

    pub fn open_dialog(&mut self, d: Dialog, cx: &mut Context<Self>) {
        self.dialog = Some(d);
        self.menu = None;
        self.sync_ui(cx);
        cx.notify();
    }

    pub fn close_dialog(&mut self, cx: &mut Context<Self>) {
        self.dialog = None;
        self.sync_ui(cx);
        cx.notify();
    }

    pub fn open_setup(&mut self, step: Option<String>, cx: &mut Context<Self>) {
        self.setup = Some(step.unwrap_or_else(|| "welcome".into()));
        self.sync_ui(cx);
        cx.notify();
    }

    pub fn close_setup(&mut self, cx: &mut Context<Self>) {
        self.setup = None;
        cx.emit(StoreEvent::SetupClosed);
        self.sync_ui(cx);
        cx.notify();
    }

    pub fn set_agent_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.agent_open = open && self.settings.agent.enabled;
        self.sync_ui(cx);
        cx.notify();
    }

    pub fn open_menu(&mut self, position: Point<Pixels>, entries: Vec<MenuEntry>, cx: &mut Context<Self>) {
        self.menu = Some(ContextMenu { position, entries });
        cx.notify();
    }

    pub fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            cx.notify();
        }
    }

    /// The active layer's id.
    pub fn active(&self) -> Option<String> {
        self.doc.as_ref().and_then(|d| d.active_id())
    }

    pub fn doc(&self) -> Option<&Document> {
        self.doc.as_deref()
    }
}
