//! The real window in GPUI's headless test platform: a session with a document, the workspace
//! as the root view, simulated keys and pointer. Commands run on Tokio as in the app, so checks
//! wait for the session to settle.

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px};
use nori_control::{Session, SessionOptions, Source};
use serde_json::{Value, json};

use crate::app::Workspace;
use crate::store::{Dialog, StoreExt, Tool};

pub(crate) struct Fixture {
    pub rt: tokio::runtime::Runtime,
    _dir: tempfile::TempDir,
    pub session: Arc<Session>,
}

impl Fixture {
    pub fn call(&self, name: &str, params: Value) -> Value {
        self.rt.block_on(nori_control::call(&self.session, Source::Cli, name, params)).unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    /// Lets the window and Tokio work until `done` holds (or `PATIENCE` passes).
    pub fn settle(&self, cx: &mut VisualTestContext, done: impl Fn(&nori_core::Document) -> bool) -> nori_core::Document {
        let start = Instant::now();
        loop {
            cx.run_until_parked();
            let d = self.session.document().unwrap();
            if done(&d) || start.elapsed() > PATIENCE {
                return d;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

pub(crate) const PATIENCE: Duration = Duration::from_secs(20);

#[cfg(target_os = "macos")]
const M: &str = "cmd";
#[cfg(not(target_os = "macos"))]
const M: &str = "ctrl";

pub(crate) fn setup(cx: &mut TestAppContext) -> (Fixture, gpui::Entity<Workspace>, &mut VisualTestContext) {
    cx.executor().allow_parking();
    unsafe { std::env::set_var("NORI_NO_SYSTEM_FONTS", "1") };
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("LSUITE_HOME", dir.path().join("lsuite")) };
    let session = {
        let _g = rt.enter();
        Session::new(SessionOptions { data_dir: Some(dir.path().join("data")), config_dir: Some(dir.path().join("config")), secrets: None, headless: false }).unwrap()
    };
    let f = Fixture { rt, _dir: dir, session };
    f.call("app.finishOnboarding", json!({}));
    f.call("doc.new", json!({ "width": 400, "height": 300, "background": "#ffffff" }));
    let (handle, session) = (f.rt.handle().clone(), f.session.clone());
    cx.update(|cx| {
        gpui_tokio::init_from_handle(cx, handle);
        crate::app::init(session, cx);
    });
    let (view, vcx) = cx.add_window_view(Workspace::new);
    vcx.run_until_parked();
    (f, view, vcx)
}

fn layers(d: &nori_core::Document) -> usize {
    d.page().all().len()
}

#[gpui::test]
fn shortcuts_edit_through_the_registry_and_undo(cx: &mut TestAppContext) {
    let (f, _, cx) = setup(cx);
    cx.simulate_keystrokes(&format!("{M}-shift-n"));
    let d = f.settle(cx, |d| layers(d) == 2);
    assert_eq!(layers(&d), 2, "⇧⌘N adds a layer");
    cx.simulate_keystrokes(&format!("{M}-z"));
    let d = f.settle(cx, |d| layers(d) == 1);
    assert_eq!(layers(&d), 1, "undo undoes it");
    let steps = f.call("history.list", json!({}));
    assert_eq!(steps["redo"][0]["by"], "window");
}

#[gpui::test]
fn tool_keys_and_dialogs(cx: &mut TestAppContext) {
    let (_f, _, cx) = setup(cx);
    cx.simulate_keystrokes("b");
    cx.run_until_parked();
    assert_eq!(cx.update(|_, cx| cx.store().read(cx).tool), Tool::Brush);
    cx.simulate_keystrokes("m");
    cx.simulate_keystrokes("m");
    cx.run_until_parked();
    assert_eq!(cx.update(|_, cx| cx.store().read(cx).tool), Tool::EllipseSelect, "M twice: the ellipse");
    cx.simulate_keystrokes(&format!("{M}-shift-e"));
    cx.run_until_parked();
    assert_eq!(cx.update(|_, cx| cx.store().read(cx).dialog.clone()), Some(Dialog::Export));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(cx.update(|_, cx| cx.store().read(cx).dialog.clone()), None);
}

#[gpui::test]
fn the_brush_paints_what_the_command_paints(cx: &mut TestAppContext) {
    let (f, view, cx) = setup(cx);
    f.call("layer.add", json!({ "name": "Paint" }));
    f.call("color.set", json!({ "foreground": "#ff0000" }));
    cx.run_until_parked();
    cx.simulate_keystrokes("b");
    cx.run_until_parked();
    // The canvas: where the page is on screen.
    let canvas = cx.update(|_, cx| view.read(cx).editor().read(cx).canvas.clone());
    let (zoom, origin) = cx.update(|_, cx| {
        let c = canvas.read(cx);
        (c.zoom, c.page_origin())
    });
    let at = |x: f32, y: f32| point(px(f32::from(origin.x) + x * zoom), px(f32::from(origin.y) + y * zoom));
    cx.simulate_mouse_down(at(50., 150.), MouseButton::Left, Modifiers::none());
    for i in 1..=10 {
        cx.simulate_mouse_move(at(50. + i as f32 * 30., 150.), Some(MouseButton::Left), Modifiers::none());
    }
    cx.simulate_mouse_up(at(350., 150.), MouseButton::Left, Modifiers::none());
    let d = f.settle(cx, |d| d.page().layers[0].raster().is_some_and(|(_, _, p)| p.used_tiles() > 0));
    let (x, y, p) = d.page().layers[0].raster().unwrap();
    assert_eq!(p.get(200 - x, 150 - y)[..3], [255, 0, 0], "a red stroke across the middle");
    assert_eq!(f.call("history.list", json!({}))["undo"].as_array().unwrap().last().unwrap()["command"], "raster.stroke");
}
