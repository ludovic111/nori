//! `plugin.*`: the Plugins area's commands, and the recipe an agent follows to write a plugin in
//! Rust (PLUGINS.md): guide, toolchain, new, writeSource, build, publishLocal.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};

use crate::plugins::{plugins_dir, sources_dir};
use crate::registry::{Args, Ctx};
use crate::session::{CmdResult, Event, Session};

/// The SDK's sources, shipped inside nori so plugins build offline and before nori's
/// repository has a tag to point at.
const SDK_FILES: &[(&str, &str)] = &[
    ("Cargo.toml", "[package]\nname = \"nori-plugin\"\nversion = \"0.1.0\"\nedition = \"2024\"\nlicense = \"MIT\"\ndescription = \"The nori plugin SDK\"\n\n[dependencies]\n"),
    ("src/lib.rs", include_str!("../../../nori-plugin/src/lib.rs")),
    ("src/ffi.rs", include_str!("../../../nori-plugin/src/ffi.rs")),
    ("GUIDE.md", include_str!("../../../nori-plugin/GUIDE.md")),
];

/// Writes the bundled SDK under the sources folder (once per version).
fn sdk_dir() -> CmdResult<PathBuf> {
    let dir = sources_dir().join(".sdk").join(format!("nori-plugin-{}", env!("CARGO_PKG_VERSION")));
    for (rel, text) in SDK_FILES {
        let p = dir.join(rel);
        if std::fs::read_to_string(&p).ok().as_deref() != Some(*text) {
            std::fs::create_dir_all(p.parent().expect("parent")).map_err(|e| e.to_string())?;
            std::fs::write(&p, text).map_err(|e| e.to_string())?;
        }
    }
    Ok(dir)
}

fn crate_dir(name: &str) -> CmdResult<PathBuf> {
    if name.is_empty() || name.len() > 64 || !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') || name.starts_with('-') {
        return Err("A plugin's name is lowercase letters, digits and dashes (duotone, film-grain).".into());
    }
    Ok(sources_dir().join(name))
}

fn lib_file(name: &str) -> String {
    let n = name.replace('-', "_");
    if cfg!(target_os = "macos") {
        format!("lib{n}.dylib")
    } else if cfg!(windows) {
        format!("{n}.dll")
    } else {
        format!("lib{n}.so")
    }
}

/// `cargo`, from PATH or rustup's usual place.
pub fn cargo() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "cargo.exe" } else { "cargo" };
    if let Some(paths) = std::env::var_os("PATH") {
        for p in std::env::split_paths(&paths) {
            if p.join(exe).is_file() {
                return Some(p.join(exe));
            }
        }
    }
    let home = std::env::var_os("CARGO_HOME").map(PathBuf::from).or_else(|| dirs::home_dir().map(|h| h.join(".cargo")))?;
    let p = home.join("bin").join(exe);
    p.is_file().then_some(p)
}

pub const INSTALL_HINT: &str = "Install Rust with rustup (rustup.rs): on macOS and Linux run `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y`; on Windows download rustup-init.exe from rustup.rs.";

fn toolchain() -> Value {
    let Some(cargo) = cargo() else { return json!({ "ok": false, "cargo": null, "rustc": null, "version": null, "installHint": INSTALL_HINT }) };
    let ver = |bin: &Path| std::process::Command::new(bin).arg("--version").output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    let rustc = cargo.with_file_name(if cfg!(windows) { "rustc.exe" } else { "rustc" });
    let rustc = if rustc.is_file() { Some(rustc) } else { None };
    let v = ver(&cargo);
    json!({ "ok": v.is_some(), "cargo": cargo.display().to_string(), "rustc": rustc.map(|r| r.display().to_string()), "version": v, "installHint": INSTALL_HINT })
}

const TEMPLATE_LIB: &str = r#"//! A nori plugin. `plugin.guide` explains the SDK; build with `plugin.build`, install with
//! `plugin.publishLocal`.

use nori_plugin::prelude::*;

pub struct Plugin;

impl Filter for Plugin {
    const INFO: Info = Info::filter("{ID}", "{NAME}", "{VENDOR}", "other", "What it does, in one sentence.");

    fn params() -> Vec<Param> {
        vec![Param::number("amount", "Amount", 0.0, 100.0, 50.0, "%")]
    }

    fn new() -> Self {
        Plugin
    }

    fn process(&mut self, tile: &mut Tile, values: &[f64]) {
        let amount = (values[0] / 100.0) as f32;
        for p in tile.pixels.iter_mut() {
            let (c, a) = unpremultiply(*p);
            if a <= 0.0 {
                continue;
            }
            // Example: towards grey by `amount`.
            let l = luma(c);
            *p = premultiply([0, 1, 2].map(|i| c[i] + (l - c[i]) * amount), a);
        }
    }
}

export_plugins!(Plugin);
"#;

fn title(name: &str) -> String {
    name.split('-').filter(|w| !w.is_empty()).map(|w| { let mut c = w.chars(); c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default() }).collect::<Vec<_>>().join(" ")
}

fn plugin_toml(name: &str) -> String {
    format!(
        "id = \"local.nori.{name}\"\nname = \"{}\"\nversion = \"0.1.0\"\napp = \"nori\"\nkind = \"filter\"\nabi = {}\ndescription = \"\"\nauthors = []\n\n[library]\nmacos = \"lib{n}.dylib\"\nlinux = \"lib{n}.so\"\nwindows = \"{n}.dll\"\n",
        title(name),
        nori_plugin::ABI_VERSION,
        n = name.replace('-', "_")
    )
}

/// Runs `cargo build --release` and reads the compiler's messages.
fn build(dir: &Path) -> Value {
    let Some(cargo) = cargo() else { return json!({ "ok": false, "errors": [{ "file": null, "line": null, "message": format!("Rust isn't installed. {INSTALL_HINT}") }] }) };
    let out = std::process::Command::new(cargo).args(["build", "--release", "--message-format=json-diagnostic-rendered-ansi"]).current_dir(dir).env("CARGO_TERM_COLOR", "never").output();
    let out = match out {
        Ok(o) => o,
        Err(e) => return json!({ "ok": false, "errors": [{ "file": null, "line": null, "message": format!("Couldn't run cargo: {e}") }] }),
    };
    let mut errors = vec![];
    let mut warnings = 0;
    let mut artifact: Option<String> = None;
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        match v["reason"].as_str() {
            Some("compiler-message") => {
                let m = &v["message"];
                match m["level"].as_str() {
                    Some("error") => {
                        let span = m["spans"].as_array().and_then(|s| s.iter().find(|s| s["is_primary"] == json!(true)).or(s.first()));
                        let rendered: String = m["rendered"].as_str().unwrap_or(m["message"].as_str().unwrap_or("")).chars().filter(|c| *c != '\u{1b}').collect();
                        // Strip ANSI colour codes.
                        let clean = strip_ansi(&rendered);
                        errors.push(json!({ "file": span.and_then(|s| s["file_name"].as_str()), "line": span.and_then(|s| s["line_start"].as_u64()), "message": clean.trim() }));
                    }
                    Some("warning") => warnings += 1,
                    _ => {}
                }
            }
            Some("compiler-artifact") if v["target"]["kind"].as_array().is_some_and(|k| k.iter().any(|x| x == "cdylib")) => {
                artifact = v["filenames"].as_array().and_then(|f| f.iter().filter_map(Value::as_str).find(|f| f.ends_with(".so") || f.ends_with(".dylib") || f.ends_with(".dll")).map(str::to_string));
            }
            _ => {}
        }
    }
    if !out.status.success() && errors.is_empty() {
        let text = strip_ansi(&String::from_utf8_lossy(&out.stderr));
        let tail: Vec<&str> = text.lines().rev().take(20).collect();
        errors.push(json!({ "file": null, "line": null, "message": tail.into_iter().rev().collect::<Vec<_>>().join("\n") }));
    }
    json!({ "ok": out.status.success(), "errors": errors, "warnings": warnings, "library": artifact })
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for d in chars.by_ref() {
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let p = e.path();
        if p.is_dir() {
            copy_dir(&p, &to.join(e.file_name()))?;
        } else {
            std::fs::copy(&p, to.join(e.file_name()))?;
        }
    }
    Ok(())
}

fn installed_id(dir: &Path) -> CmdResult<String> {
    let text = std::fs::read_to_string(dir.join("plugin.toml")).map_err(|e| format!("{}: no plugin.toml ({e})", dir.display()))?;
    let m: crate::plugins::Manifest = toml::from_str(&text).map_err(|e| format!("plugin.toml: {e}"))?;
    if m.id.is_empty() || m.id.contains(['/', '\\', '.']) && m.id.contains("..") {
        return Err("plugin.toml's id isn't valid.".into());
    }
    Ok(m.id)
}

pub fn list(s: &Session) -> Value {
    let disabled = s.settings().plugins.disabled;
    let stock: Vec<Value> = nori_render::filters::STOCK
        .iter()
        .map(|f| json!({ "id": f.id, "name": f.name, "kind": "filter", "group": f.group, "format": "stock", "version": env!("CARGO_PKG_VERSION"), "description": f.description, "enabled": !disabled.contains(&f.id.to_string()) }))
        .collect();
    let installed: Vec<Value> = s
        .plugins
        .loaded()
        .iter()
        .map(|l| {
            json!({
                "id": l.bundle.id,
                "filter": format!("plugin:{}", l.info.id),
                "name": l.info.name,
                "kind": l.bundle.kind,
                "format": "lsuite",
                "vendor": l.info.vendor,
                "version": l.bundle.version,
                "path": l.dir.display().to_string(),
                "description": l.info.description,
                "enabled": !disabled.contains(&l.bundle.id) && !l.poisoned.load(std::sync::atomic::Ordering::Relaxed),
                "crashed": l.poisoned.load(std::sync::atomic::Ordering::Relaxed),
            })
        })
        .collect();
    let formats = json!([
        { "id": "lsuite", "name": "lsuite plugins (nori SDK)", "loads": "filters", "where": plugins_dir().display().to_string(), "logo": "lsuite" },
        { "id": "cube", "name": ".cube colour lookup tables", "loads": "Color Lookup adjustment layers (layer.addLut)", "where": "any file", "makers": ["Blackmagic Design", "Adobe"], "logo": "resolve" },
        { "id": "gbr", "name": ".gbr brushes", "loads": "brush tips (brushes.import)", "where": s.data_dir.join("brushes").display().to_string(), "makers": ["GIMP", "Krita"], "logo": "gimp" },
        { "id": "ase", "name": ".ase swatches", "loads": "swatches (color.importSwatches)", "where": "any file", "makers": ["Adobe", "Affinity"], "logo": "photoshop" },
    ]);
    json!({ "stock": stock, "installed": installed, "formats": formats, "problems": s.plugins.problems() })
}

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    match cx.spec.name {
        "plugin.list" => Ok(list(s)),
        "plugin.info" => {
            let id = a.str("id")?;
            if let Some(f) = nori_render::filters::spec(id) {
                return Ok(json!({ "id": f.id, "name": f.name, "kind": "filter", "format": "stock", "description": f.description, "params": f.params, "source": "ships with nori" }));
            }
            let l = s.plugins.find(id.trim_start_matches("plugin:")).ok_or_else(|| format!("No plugin `{id}` (plugin.list)."))?;
            Ok(json!({ "id": l.bundle.id, "filter": format!("plugin:{}", l.info.id), "name": l.info.name, "kind": l.bundle.kind, "format": "lsuite", "vendor": l.info.vendor, "version": l.bundle.version, "description": l.info.description, "params": l.info.params, "path": l.dir.display().to_string(), "authors": l.bundle.authors }))
        }
        "plugin.enable" | "plugin.disable" => {
            let id = a.str("id")?.trim_start_matches("plugin:").to_string();
            let on = cx.spec.name == "plugin.enable";
            s.update_settings(|st| {
                st.plugins.disabled.retain(|d| *d != id);
                if !on {
                    st.plugins.disabled.push(id.clone());
                }
            })?;
            if on && let Some(l) = s.plugins.find(&id) {
                l.poisoned.store(false, std::sync::atomic::Ordering::Relaxed);
            }
            s.emit(Event::PluginsChanged);
            Ok(json!({ "id": id, "enabled": on }))
        }
        "plugin.rescan" => {
            let r = s.plugins.rescan(&s.data_dir);
            s.emit(Event::PluginsChanged);
            Ok(r)
        }
        "plugin.install" => {
            let from = super::util::path(&a, "path")?;
            let id = installed_id(&from)?;
            let to = plugins_dir().join(&id);
            let _ = std::fs::remove_dir_all(&to);
            copy_dir(&from, &to).map_err(|e| format!("Couldn't install it: {e}"))?;
            let r = s.plugins.rescan(&s.data_dir);
            s.emit(Event::PluginsChanged);
            Ok(json!({ "installed": id, "path": to.display().to_string(), "scan": r }))
        }
        "plugin.remove" => {
            let id = a.str("id")?.trim_start_matches("plugin:").to_string();
            if nori_render::filters::spec(&id).is_some() {
                return Err("Stock filters can only be switched off (plugin.disable).".into());
            }
            let dir = plugins_dir().join(&id);
            if !dir.join("plugin.toml").is_file() {
                return Err(format!("No installed plugin `{id}`."));
            }
            std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
            let r = s.plugins.rescan(&s.data_dir);
            s.emit(Event::PluginsChanged);
            Ok(json!({ "removed": id, "scan": r }))
        }
        "plugin.guide" => Ok(json!({ "guide": nori_plugin::GUIDE, "sdk": sdk_dir().ok().map(|p| p.display().to_string()), "sources": sources_dir().display().to_string(), "installed": plugins_dir().display().to_string() })),
        "plugin.toolchain" => Ok(toolchain()),
        "plugin.new" => {
            let name = a.str("name")?.trim().to_string();
            if a.opt_str("kind").is_some_and(|k| k != "filter") {
                return Err("nori plugins are filters for now (kind filter).".into());
            }
            let dir = crate_dir(&name)?;
            if dir.join("Cargo.toml").exists() {
                return Err(format!("{} already exists: write to it with plugin.writeSource, or pick another name.", dir.display()));
            }
            let sdk = sdk_dir()?;
            let cargo = format!(
                "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n\n[dependencies]\n# The SDK nori ships (works offline). Once nori is on GitHub this can be:\n# nori-plugin = {{ git = \"https://github.com/ludovic111/nori\", tag = \"v{}\" }}\nnori-plugin = {{ path = \"{}\" }}\n\n[profile.release]\nopt-level = 3\n\n[workspace]\n",
                env!("CARGO_PKG_VERSION"),
                sdk.display().to_string().replace('\\', "/")
            );
            let lib = TEMPLATE_LIB.replace("{ID}", &format!("local.nori.{name}")).replace("{NAME}", &title(&name)).replace("{VENDOR}", "Me");
            let files = [("Cargo.toml", cargo), ("src/lib.rs", lib), ("plugin.toml", plugin_toml(&name))];
            for (rel, text) in &files {
                let p = dir.join(rel);
                std::fs::create_dir_all(p.parent().expect("parent")).map_err(|e| e.to_string())?;
                std::fs::write(&p, text).map_err(|e| e.to_string())?;
            }
            Ok(json!({ "name": name, "path": dir.display().to_string(), "files": files.iter().map(|(r, t)| json!({ "path": r, "contents": t })).collect::<Vec<_>>() }))
        }
        "plugin.writeSource" => {
            let dir = crate_dir(a.str("name")?)?;
            if !dir.join("Cargo.toml").is_file() {
                return Err("No such plugin crate: make it with plugin.new.".into());
            }
            let rel = PathBuf::from(a.str("path")?);
            if rel.is_absolute() || rel.components().any(|c| !matches!(c, Component::Normal(_))) {
                return Err("The path must be inside the crate (like src/lib.rs).".into());
            }
            let p = dir.join(&rel);
            std::fs::create_dir_all(p.parent().expect("parent")).map_err(|e| e.to_string())?;
            let contents = a.str("contents")?;
            std::fs::write(&p, contents).map_err(|e| e.to_string())?;
            Ok(json!({ "written": p.display().to_string(), "bytes": contents.len() }))
        }
        "plugin.build" => {
            let dir = crate_dir(a.str("name")?)?;
            if !dir.join("Cargo.toml").is_file() {
                return Err("No such plugin crate: make it with plugin.new.".into());
            }
            Ok(tokio::task::spawn_blocking(move || build(&dir)).await.map_err(|e| e.to_string())?)
        }
        "plugin.publishLocal" => {
            let name = a.str("name")?.to_string();
            let dir = crate_dir(&name)?;
            if !dir.join("Cargo.toml").is_file() {
                return Err("No such plugin crate: make it with plugin.new.".into());
            }
            let d2 = dir.clone();
            let b = tokio::task::spawn_blocking(move || build(&d2)).await.map_err(|e| e.to_string())?;
            if b["ok"] != json!(true) {
                return Ok(json!({ "ok": false, "build": b }));
            }
            let lib = b["library"].as_str().map(PathBuf::from).unwrap_or_else(|| dir.join("target/release").join(lib_file(&name)));
            let id = installed_id(&dir)?;
            let to = plugins_dir().join(&id);
            std::fs::create_dir_all(&to).map_err(|e| e.to_string())?;
            // plugin.toml names the library each system loads; this build's goes in.
            let toml_text = std::fs::read_to_string(dir.join("plugin.toml")).map_err(|e| e.to_string())?;
            std::fs::write(to.join("plugin.toml"), toml_text).map_err(|e| e.to_string())?;
            let here = {
                let m: crate::plugins::Manifest = toml::from_str(&std::fs::read_to_string(to.join("plugin.toml")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
                m.library.here().to_string()
            };
            std::fs::copy(&lib, to.join(if here.is_empty() { lib_file(&name) } else { here })).map_err(|e| format!("Couldn't copy the library: {e}"))?;
            let scan = s.plugins.rescan(&s.data_dir);
            s.emit(Event::PluginsChanged);
            let filters: Vec<Value> = s.plugins.filters().into_iter().filter(|f| f["pluginId"] == json!(id)).collect();
            Ok(json!({ "ok": true, "installed": id, "path": to.display().to_string(), "filters": filters, "scan": scan, "warnings": b["warnings"] }))
        }
        _ => Err(super::unhandled(cx)),
    }
}

/// Installs Rust with rustup (the window's button, after the person agreed).
pub fn install_rust() -> Result<(), String> {
    #[cfg(unix)]
    {
        let st = std::process::Command::new("sh").args(["-c", "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal"]).status().map_err(|e| e.to_string())?;
        if st.success() { Ok(()) } else { Err("rustup didn't finish; see rustup.rs".into()) }
    }
    #[cfg(not(unix))]
    {
        std::process::Command::new("cmd").args(["/C", "start", "", "https://rustup.rs"]).spawn().map(|_| ()).map_err(|e| format!("Couldn't open the browser ({e}). Open https://rustup.rs yourself."))
    }
}
