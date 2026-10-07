//! The registry end to end, as a client sees it.

use std::sync::{Arc, OnceLock};

use serde_json::{Value, json};

use crate::registry::call;
use crate::session::{Session, SessionOptions, Source};

/// A scratch `LSUITE_HOME` for the whole test run (set once: the environment is shared).
fn home() -> &'static std::path::Path {
    static HOME: OnceLock<tempfile::TempDir> = OnceLock::new();
    HOME.get_or_init(|| {
        let d = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("LSUITE_HOME", d.path()) };
        unsafe { std::env::set_var("NORI_NO_SYSTEM_FONTS", "1") };
        d
    })
    .path()
}

fn session() -> (Arc<Session>, tempfile::TempDir) {
    home();
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new(SessionOptions { data_dir: Some(dir.path().join("data")), config_dir: Some(dir.path().join("config")), secrets: None, headless: true }).unwrap();
    (s, dir)
}

async fn ok(s: &Arc<Session>, name: &str, params: Value) -> Value {
    call(s, Source::Cli, name, params).await.unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_photo_workflow() {
    let (s, dir) = session();
    ok(&s, "doc.new", json!({ "width": 400, "height": 300, "background": "#ffffff" })).await;
    let l = ok(&s, "layer.add", json!({ "name": "Paint" })).await;
    let id = l["layerId"].as_str().unwrap().to_string();
    ok(&s, "raster.stroke", json!({ "points": [[50, 150], [350, 150]], "color": "#ff0000", "size": 20, "hardness": 1 })).await;
    let px = ok(&s, "raster.pick", json!({ "x": 200, "y": 150 })).await;
    assert_eq!(px["color"], "#ff0000");
    // Select the left half and blur only there.
    ok(&s, "select.rect", json!({ "x": 0, "y": 0, "width": 200, "height": 300 })).await;
    ok(&s, "filter.apply", json!({ "filter": "gaussianBlur", "params": { "radius": 6 } })).await;
    let left = ok(&s, "raster.pick", json!({ "x": 100, "y": 133, "layerId": id })).await;
    let right = ok(&s, "raster.pick", json!({ "x": 300, "y": 133, "layerId": id })).await;
    assert_ne!(left["rgba"][3], json!(0), "the blur spread the stroke on the left");
    assert_eq!(right["rgba"][3], json!(0), "the right is untouched");
    ok(&s, "select.none", json!({})).await;
    ok(&s, "layer.addAdjustment", json!({ "kind": "invert" })).await;
    assert_eq!(ok(&s, "raster.pick", json!({ "x": 5, "y": 5 })).await["color"], "#000000");
    ok(&s, "history.undo", json!({})).await;
    assert_eq!(ok(&s, "raster.pick", json!({ "x": 5, "y": 5 })).await["color"], "#ffffff");
    // Save, export, reopen.
    let path = dir.path().join("photo.nori");
    ok(&s, "doc.save", json!({ "path": path })).await;
    let png = dir.path().join("photo.png");
    let r = ok(&s, "export.file", json!({ "path": png, "scale": 0.5 })).await;
    assert_eq!((r["width"].as_u64(), r["height"].as_u64()), (Some(200), Some(150)));
    ok(&s, "doc.open", json!({ "path": path })).await;
    let o = ok(&s, "doc.overview", json!({})).await;
    assert_eq!(o["pages"][0]["layers"].as_array().unwrap().len(), 2);
    ok(&s, "doc.open", json!({ "path": png })).await;
    assert_eq!(ok(&s, "doc.overview", json!({})).await["pages"][0]["width"], 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_booklet_workflow() {
    let (s, dir) = session();
    ok(&s, "doc.new", json!({ "preset": "a5", "pages": 2, "margins": 120 })).await;
    let o = ok(&s, "doc.overview", json!({})).await;
    assert_eq!(o["pages"].as_array().unwrap().len(), 2);
    assert_eq!(o["dpi"], 300.0);
    ok(&s, "text.defineStyle", json!({ "kind": "paragraph", "name": "Body", "size": 40, "font": "Manrope" })).await;
    let words = "lorem ipsum dolor sit amet ".repeat(80);
    let a = ok(&s, "text.add", json!({ "text": words, "x": 120, "y": 120, "frameWidth": 1500, "frameHeight": 400, "style": "Body" })).await["layerId"].as_str().unwrap().to_string();
    ok(&s, "page.select", json!({ "page": "2" })).await;
    let b = ok(&s, "text.add", json!({ "text": "", "x": 120, "y": 120, "frameWidth": 1500, "frameHeight": 2000 })).await["layerId"].as_str().unwrap().to_string();
    let o = ok(&s, "doc.overview", json!({})).await;
    assert!(o["problems"].as_array().unwrap().iter().any(|p| p.as_str().unwrap().contains("doesn't fit")), "{}", o["problems"]);
    let t = ok(&s, "text.thread", json!({ "from": a, "to": b })).await;
    assert_eq!(t["thread"].as_array().unwrap().len(), 2);
    let o = ok(&s, "doc.overview", json!({})).await;
    assert!(!o["problems"].as_array().unwrap().iter().any(|p| p.as_str().unwrap().contains("doesn't fit")), "{}", o["problems"]);
    // Changing the style re-sets the text.
    let r = ok(&s, "text.defineStyle", json!({ "kind": "paragraph", "name": "Body", "size": 44 })).await;
    assert_eq!(r["restyled"].as_array().unwrap().len(), 1);
    // A star and a rounded box, combined.
    let s1 = ok(&s, "vector.addShape", json!({ "shape": "rect", "x": 200, "y": 1200, "width": 600, "height": 400, "radius": 40, "fill": "#2244ff" })).await["layerId"].as_str().unwrap().to_string();
    let s2 = ok(&s, "vector.addShape", json!({ "shape": "star", "x": 500, "y": 1100, "width": 600, "height": 600, "fill": { "type": "linear", "x1": 500, "y1": 1100, "x2": 1100, "y2": 1700, "stops": ["#ff0000", "#ffcc00"] } })).await["layerId"].as_str().unwrap().to_string();
    ok(&s, "vector.combine", json!({ "layerIds": [s1, s2], "op": "union" })).await;
    let pdf = dir.path().join("booklet.pdf");
    let r = ok(&s, "export.file", json!({ "path": pdf })).await;
    assert_eq!(r["pages"], 2);
    let svg = dir.path().join("page.svg");
    ok(&s, "export.file", json!({ "path": svg, "page": "2" })).await;
    let look = ok(&s, "page.look", json!({ "page": "1", "width": 300 })).await;
    assert!(std::path::Path::new(look["path"].as_str().unwrap()).exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn batches_are_one_step_and_roll_back() {
    let (s, _dir) = session();
    ok(&s, "doc.new", json!({ "width": 64, "height": 64 })).await;
    let before = ok(&s, "history.list", json!({})).await["undo"].as_array().unwrap().len();
    ok(&s, "doc.batch", json!({ "commands": [
        { "command": "layer.add", "params": { "name": "A" } },
        { "command": "layer.add", "params": { "name": "B" } },
    ] })).await;
    let after = ok(&s, "history.list", json!({})).await["undo"].as_array().unwrap().len();
    assert_eq!(after, before + 1);
    let e = call(&s, Source::Cli, "doc.batch", json!({ "commands": [
        { "command": "layer.add", "params": { "name": "C" } },
        { "command": "layer.update", "params": { "layerId": "nope", "opacity": 0.5 } },
    ] })).await.unwrap_err();
    assert!(e.contains("Nothing was changed"), "{e}");
    let names: Vec<String> = ok(&s, "layer.list", json!({})).await["layers"].as_array().unwrap().iter().map(|l| l["name"].as_str().unwrap().to_string()).collect();
    assert!(!names.contains(&"C".to_string()));
    assert!(names.contains(&"B".to_string()));
}

#[tokio::test(flavor = "multi_thread")]
async fn permissions_and_validation() {
    let (s, _dir) = session();
    ok(&s, "doc.new", json!({})).await;
    // Agents can't sign in, build plugins (off by default) or change their own settings.
    assert!(call(&s, Source::Agent, "account.signIn", json!({})).await.unwrap_err().contains("stays with the person"));
    assert!(call(&s, Source::Mcp, "plugin.build", json!({ "name": "x" })).await.unwrap_err().contains("plugins"));
    assert!(call(&s, Source::Agent, "app.setSetting", json!({ "key": "agent.permissions.plugins", "value": true })).await.is_err());
    // Typos get a hint.
    let e = call(&s, Source::Cli, "layer.ad", json!({})).await.unwrap_err();
    assert!(e.contains("layer.add"), "{e}");
    let e = call(&s, Source::Cli, "layer.update", json!({ "opacty": 0.5 })).await.unwrap_err();
    assert!(e.contains("opacity"), "{e}");
    let e = call(&s, Source::Cli, "filter.apply", json!({ "filter": "gausianBlur" })).await.unwrap_err();
    assert!(e.contains("gaussianBlur"), "{e}");
    // Plugin sources stay inside their crate.
    ok(&s, "plugin.new", json!({ "name": "test-tint" })).await;
    let e = call(&s, Source::Cli, "plugin.writeSource", json!({ "name": "test-tint", "path": "../../escape.rs", "contents": "x" })).await.unwrap_err();
    assert!(e.contains("inside the crate"), "{e}");
    assert!(call(&s, Source::Cli, "plugin.new", json!({ "name": "Bad Name" })).await.is_err());
    let g = ok(&s, "plugin.guide", json!({})).await;
    assert!(g["guide"].as_str().unwrap().contains("export_plugins!"));
}

#[tokio::test(flavor = "multi_thread")]
async fn layers_move_group_merge_and_mask() {
    let (s, _dir) = session();
    ok(&s, "doc.new", json!({ "width": 100, "height": 100, "background": "#000000" })).await;
    let a = ok(&s, "layer.add", json!({ "kind": "fill", "color": "#ff0000", "name": "Red" })).await["layerId"].as_str().unwrap().to_string();
    ok(&s, "layer.addMask", json!({ "layerId": a, "from": "hide" })).await;
    assert_eq!(ok(&s, "raster.pick", json!({ "x": 50, "y": 50 })).await["color"], "#000000");
    ok(&s, "layer.mask", json!({ "layerId": a, "action": "invert" })).await;
    assert_eq!(ok(&s, "raster.pick", json!({ "x": 50, "y": 50 })).await["color"], "#ff0000");
    ok(&s, "layer.update", json!({ "layerId": "Red", "opacity": 50, "blend": "screen" })).await;
    let g = ok(&s, "layer.group", json!({ "layerIds": ["Red"] })).await["layerId"].as_str().unwrap().to_string();
    let list = ok(&s, "layer.list", json!({})).await;
    assert_eq!(list["layers"][0]["id"], json!(g));
    ok(&s, "layer.ungroup", json!({ "layerId": g })).await;
    ok(&s, "layer.merge", json!({ "layerId": "Red" })).await;
    let list = ok(&s, "layer.list", json!({})).await;
    assert_eq!(list["layers"].as_array().unwrap().len(), 1);
    let t = ok(&s, "text.add", json!({ "text": "Hi", "x": 10, "y": 10, "size": 30, "color": "#ffffff" })).await["layerId"].as_str().unwrap().to_string();
    ok(&s, "layer.move", json!({ "layerId": t, "dx": 20, "dy": 5 })).await;
    let got = ok(&s, "layer.get", json!({ "layerId": t })).await;
    assert_eq!(got["text"]["x"], 30.0);
    ok(&s, "layer.transform", json!({ "layerId": t, "scale": 2 })).await;
    assert_eq!(ok(&s, "layer.get", json!({ "layerId": t })).await["text"]["size"], 60.0);
    ok(&s, "doc.crop", json!({ "x": 10, "y": 10, "width": 50, "height": 40 })).await;
    let o = ok(&s, "doc.overview", json!({})).await;
    assert_eq!((o["pages"][0]["width"].as_u64(), o["pages"][0]["height"].as_u64()), (Some(50), Some(40)));
    ok(&s, "doc.rotate", json!({ "quarters": 1 })).await;
    assert_eq!(ok(&s, "doc.overview", json!({})).await["pages"][0]["width"], 40);
}

#[test]
fn commands_md_is_generated() {
    let want = crate::registry::markdown();
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/COMMANDS.md");
    let have = std::fs::read_to_string(path).unwrap_or_default();
    assert!(have == want, "docs/COMMANDS.md is out of date: run `cargo run -p nori-cli -- docs`");
}

#[test]
fn every_spec_is_handled_and_named_well() {
    let mut seen = std::collections::HashSet::new();
    for s in crate::registry::commands() {
        assert!(seen.insert(s.name), "{} twice", s.name);
        let (fam, verb) = s.name.split_once('.').unwrap();
        assert!(!fam.is_empty() && verb.chars().next().unwrap().is_ascii_lowercase(), "{}", s.name);
        assert!(s.doc.ends_with('.') || s.doc.ends_with(')'), "{}: the doc ends with a full stop", s.name);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn tabs_keep_independent_documents_history_and_refuse_unsaved_close() {
    let (s, _dir) = session();
    ok(&s, "doc.new", json!({ "width": 32, "height": 32 })).await;
    let first = s.current_id().unwrap();
    ok(&s, "doc.setInfo", json!({ "name": "First edited" })).await;
    ok(&s, "doc.new", json!({ "width": 64, "height": 64 })).await;
    let second = s.current_id().unwrap();
    assert_eq!(s.tabs().len(), 2);
    ok(&s, "doc.select", json!({ "id": first })).await;
    assert_eq!(s.document().unwrap().name, "First edited");
    assert!(call(&s, Source::Cli, "doc.close", json!({})).await.is_err());
    ok(&s, "history.undo", json!({})).await;
    assert_ne!(s.document().unwrap().name, "First edited");
    ok(&s, "doc.select", json!({ "id": second })).await;
    assert_eq!(s.document().unwrap().width(), 64);
    ok(&s, "doc.close", json!({ "discard": true })).await;
    assert_eq!(s.current_id(), Some(first));
    assert_eq!(s.tabs().len(), 1);
}

#[tokio::test(flavor="multi_thread")]
async fn smart_sources_survive_roundtrip_and_resizing_without_losing_original_pixels() {
    let (s,dir)=session();
    ok(&s,"doc.new",json!({"width":16,"height":16})).await;
    ok(&s,"raster.stroke",json!({"points":[[1,1],[14,14]],"size":3,"color":"#ff0033"})).await;
    let before=nori_render::flatten(&s.document().unwrap());
    ok(&s,"layer.makeSmartObject",json!({})).await;
    ok(&s,"layer.resizeSmartObject",json!({"width":4,"height":4})).await;
    ok(&s,"layer.resizeSmartObject",json!({"width":16,"height":16})).await;
    assert_eq!(before,nori_render::flatten(&s.document().unwrap()));
    assert!(call(&s,Source::Cli,"raster.stroke",json!({"points":[[8,8]],"size":4})).await.is_err());
    let path=dir.path().join("smart.nori");ok(&s,"doc.save",json!({"path":path})).await;
    ok(&s,"doc.open",json!({"path":path})).await;
    assert!(s.document().unwrap().all().iter().any(|l|l.smart_source.is_some()));
    let source=dir.path().join("source.nori");ok(&s,"layer.extractSmartObject",json!({"path":source})).await;
    assert_eq!(before,nori_render::flatten(&nori_core::file::open(&source).unwrap()));
    assert!(call(&s,Source::Cli,"layer.extractSmartObject",json!({"path":source})).await.is_err());
    ok(&s,"layer.replaceSmartObject",json!({"path":source})).await;
    ok(&s,"layer.rasterize",json!({})).await;
    assert!(s.document().unwrap().all().iter().all(|l|l.smart_source.is_none()));
}

#[tokio::test(flavor="multi_thread")]
async fn background_edits_reject_a_different_tab_with_the_same_revision() {
    let (s,_)=session();ok(&s,"doc.new",json!({"width":16,"height":16})).await;
    let (_,rev,id)=s.snapshot().unwrap();
    ok(&s,"doc.new",json!({"width":16,"height":16})).await;
    assert!(s.edit_at(id,rev,"old render",Source::Cli,|d|{d.name="WRONG".into();Ok(())}).is_err());
    assert_ne!(s.document().unwrap().name,"WRONG");
}
