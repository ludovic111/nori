//! The plugin host (lsuite's PLUGINS.md): lsuite plugin bundles (`plugin.toml` + a library
//! built with `nori-plugin`) found in `~/.lsuite/plugins/nori/`, loaded through the SDK's
//! frozen C ABI, and run off the interface thread. A library is copied before it is loaded, so
//! a rebuilt plugin loads fresh (hot reload) while the old copy stays mapped for anything still
//! using it; a plugin that panics is switched off, never the app.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use nori_plugin::ffi::{Entry, EntryFn, FilterVTable, PANICKED, RawTile};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::session::lsuite_home;

/// A bundle's `plugin.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: String,
    pub app: String,
    #[serde(default = "filter_kind")]
    pub kind: String,
    pub abi: u32,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub authors: Vec<String>,
    pub library: Libraries,
}

fn filter_kind() -> String {
    "filter".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Libraries {
    #[serde(default)]
    pub macos: String,
    #[serde(default)]
    pub linux: String,
    #[serde(default)]
    pub windows: String,
}

impl Libraries {
    pub fn here(&self) -> &str {
        if cfg!(target_os = "macos") {
            &self.macos
        } else if cfg!(windows) {
            &self.windows
        } else {
            &self.linux
        }
    }
}

/// One filter a library offers (what its manifest JSON says).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FilterInfo {
    pub abi: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub vendor: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub params: Vec<ParamInfo>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ParamInfo {
    pub name: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub kind: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub options: Vec<String>,
}

struct Table(*const FilterVTable);
unsafe impl Send for Table {}
unsafe impl Sync for Table {}

/// A filter from a loaded library.
pub struct Loaded {
    pub info: FilterInfo,
    pub bundle: Manifest,
    pub dir: PathBuf,
    table: Table,
    _lib: Arc<libloading::Library>,
    pub poisoned: AtomicBool,
}

/// A bundle that couldn't load, and why (shown in the Plugins area).
#[derive(Debug, Clone, Serialize)]
pub struct Problem {
    pub path: String,
    pub error: String,
}

#[derive(Default)]
struct Inner {
    loaded: RwLock<Vec<Arc<Loaded>>>,
    problems: RwLock<Vec<Problem>>,
    /// Library path → (its modification time, the copy loaded).
    seen: RwLock<HashMap<PathBuf, SystemTime>>,
    /// Libraries replaced by a newer build: kept mapped, never unloaded.
    retired: RwLock<Vec<Arc<libloading::Library>>>,
}

#[derive(Clone, Default)]
pub struct Host(Arc<Inner>);

/// `~/.lsuite/plugins/nori`.
pub fn plugins_dir() -> PathBuf {
    lsuite_home().join("plugins").join("nori")
}

/// `~/.lsuite/plugins-src/nori`.
pub fn sources_dir() -> PathBuf {
    lsuite_home().join("plugins-src").join("nori")
}

impl Host {
    pub fn new(_settings: &crate::settings::Settings) -> Self {
        Self::default()
    }

    /// Looks in the plugin folder again: new bundles load, changed ones reload, removed ones
    /// go. Returns what changed.
    pub fn rescan(&self, data_dir: &Path) -> Value {
        let dir = plugins_dir();
        let mut found: Vec<PathBuf> = vec![];
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                if e.path().join("plugin.toml").is_file() {
                    found.push(e.path());
                }
            }
        }
        found.sort();
        let mut problems = vec![];
        let mut loaded: Vec<Arc<Loaded>> = vec![];
        let mut added = vec![];
        let previous = self.0.loaded.read().clone();
        for b in found {
            match self.load_bundle(&b, data_dir, &previous) {
                Ok((list, fresh)) => {
                    if fresh {
                        added.extend(list.iter().map(|l| l.info.id.clone()));
                    }
                    loaded.extend(list);
                }
                Err(e) => problems.push(Problem { path: b.display().to_string(), error: e }),
            }
        }
        let removed: Vec<String> = previous.iter().filter(|p| !loaded.iter().any(|l| l.info.id == p.info.id)).map(|p| p.info.id.clone()).collect();
        *self.0.loaded.write() = loaded;
        *self.0.problems.write() = problems.clone();
        json!({ "loaded": self.0.loaded.read().len(), "new": added, "removed": removed, "problems": problems })
    }

    fn load_bundle(&self, dir: &Path, data_dir: &Path, previous: &[Arc<Loaded>]) -> Result<(Vec<Arc<Loaded>>, bool), String> {
        let text = std::fs::read_to_string(dir.join("plugin.toml")).map_err(|e| e.to_string())?;
        let m: Manifest = toml::from_str(&text).map_err(|e| format!("plugin.toml: {e}"))?;
        if m.app != "nori" {
            return Err(format!("it is a plugin for {}, not nori", m.app));
        }
        if m.abi != nori_plugin::ABI_VERSION {
            return Err(format!("it speaks plugin ABI {}; this nori speaks {}", m.abi, nori_plugin::ABI_VERSION));
        }
        let lib_name = m.library.here();
        if lib_name.is_empty() {
            return Err(format!("it has no library for {}", std::env::consts::OS));
        }
        let lib_path = dir.join(lib_name);
        let modified = std::fs::metadata(&lib_path).and_then(|md| md.modified()).map_err(|e| format!("{}: {e}", lib_path.display()))?;
        // Unchanged since last time: keep what is loaded.
        if self.0.seen.read().get(&lib_path) == Some(&modified) {
            let same: Vec<Arc<Loaded>> = previous.iter().filter(|p| p.dir == dir).cloned().collect();
            if !same.is_empty() {
                return Ok((same, false));
            }
        }
        // A fresh copy under a new name, so the system loads this build, not a cached one.
        let cache = data_dir.join("plugin-cache");
        std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
        let stamp = modified.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let copy = cache.join(format!("{}-{stamp}-{lib_name}", m.id));
        if !copy.exists() {
            std::fs::copy(&lib_path, &copy).map_err(|e| format!("couldn't copy the library: {e}"))?;
        }
        let lib = unsafe { libloading::Library::new(&copy) }.map_err(|e| format!("couldn't load {}: {e}", lib_path.display()))?;
        let lib = Arc::new(lib);
        let entry: libloading::Symbol<EntryFn> = unsafe { lib.get(nori_plugin::ENTRY_SYMBOL.as_bytes()) }.map_err(|_| format!("{} has no {} (export_plugins! makes it)", lib_name, nori_plugin::ENTRY_SYMBOL))?;
        let e: *const Entry = unsafe { entry() };
        if e.is_null() {
            return Err("its entry returned nothing".into());
        }
        let e = unsafe { &*e };
        if e.abi_version != nori_plugin::ABI_VERSION {
            return Err(format!("its library speaks ABI {}; nori speaks {}", e.abi_version, nori_plugin::ABI_VERSION));
        }
        let mut out = vec![];
        for i in 0..e.plugin_count.min(64) {
            let t = unsafe { (e.plugin)(i) };
            if t.is_null() {
                continue;
            }
            let json = unsafe { nori_plugin::ffi::manifest_json(&*t) }?;
            let info: FilterInfo = serde_json::from_str(&json).map_err(|e| format!("its manifest: {e}"))?;
            if info.abi != nori_plugin::ABI_VERSION || info.id.is_empty() || info.name.is_empty() {
                return Err(format!("filter {i} has a bad manifest"));
            }
            for p in &info.params {
                if !(p.min.is_finite() && p.max.is_finite() && p.min <= p.max && p.default.is_finite()) {
                    return Err(format!("{}: parameter `{}` has bad bounds", info.id, p.name));
                }
            }
            out.push(Arc::new(Loaded { info, bundle: m.clone(), dir: dir.to_path_buf(), table: Table(t), _lib: lib.clone(), poisoned: AtomicBool::new(false) }));
        }
        // The library it replaces stays mapped.
        for p in previous.iter().filter(|p| p.dir == dir) {
            self.0.retired.write().push(p._lib.clone());
        }
        self.0.seen.write().insert(lib_path, modified);
        Ok((out, true))
    }

    pub fn find(&self, id: &str) -> Option<Arc<Loaded>> {
        self.0.loaded.read().iter().find(|l| l.info.id == id || l.bundle.id == id).cloned()
    }

    pub fn loaded(&self) -> Vec<Arc<Loaded>> {
        self.0.loaded.read().clone()
    }

    pub fn problems(&self) -> Vec<Problem> {
        self.0.problems.read().clone()
    }

    /// Plugin filters as `filter.list` shows them.
    pub fn filters(&self) -> Vec<Value> {
        self.loaded()
            .iter()
            .filter(|l| !l.poisoned.load(Ordering::Relaxed))
            .map(|l| json!({ "id": format!("plugin:{}", l.info.id), "pluginId": l.bundle.id, "name": l.info.name, "group": l.info.group, "description": l.info.description, "params": l.info.params, "source": "plugin", "vendor": l.info.vendor }))
            .collect()
    }

    /// Values for a plugin filter from JSON, by its manifest (defaults, ranges, choices).
    pub fn values(&self, id: &str, given: &Map<String, Value>) -> Result<Vec<f64>, String> {
        let l = self.find(id).ok_or_else(|| format!("No plugin filter `{id}` (filter.list)."))?;
        for k in given.keys() {
            if !l.info.params.iter().any(|p| p.name == *k) {
                let names: Vec<&str> = l.info.params.iter().map(|p| p.name.as_str()).collect();
                return Err(format!("{} has no parameter `{k}`. Parameters: {}.", l.info.name, names.join(", ")));
            }
        }
        l.info
            .params
            .iter()
            .map(|p| {
                let v = match given.get(&p.name) {
                    None | Some(Value::Null) => p.default,
                    Some(Value::Bool(b)) => f64::from(u8::from(*b)),
                    Some(Value::Number(n)) => n.as_f64().unwrap_or(p.default),
                    Some(Value::String(s)) => match p.options.iter().position(|o| o.eq_ignore_ascii_case(s)) {
                        Some(i) => i as f64,
                        None => s.parse().map_err(|_| format!("`{}` should be a number{}", p.name, if p.options.is_empty() { String::new() } else { format!(" or one of {}", p.options.join(", ")) }))?,
                    },
                    Some(other) => return Err(format!("`{}` should be a number, not {other}", p.name)),
                };
                Ok(v.clamp(p.min, p.max))
            })
            .collect()
    }

    pub fn margin(&self, id: &str, values: &[f64]) -> Result<u32, String> {
        let l = self.find(id).ok_or_else(|| format!("No plugin filter `{id}`."))?;
        let t = unsafe { &*l.table.0 };
        let inst = unsafe { (t.create)() };
        if inst.is_null() {
            return Err(format!("{} couldn't start.", l.info.name));
        }
        let m = unsafe { (t.margin)(inst, values.as_ptr(), values.len() as u32) };
        unsafe { (t.destroy)(inst) };
        Ok(m)
    }

    /// Runs a plugin filter on premultiplied pixels.
    pub fn run(&self, id: &str, values: &[f64], px: &mut [[f32; 4]], w: u32, h: u32, origin: (i32, i32)) -> Result<(), String> {
        let l = self.find(id).ok_or_else(|| format!("No plugin filter `{id}`."))?;
        if l.poisoned.load(Ordering::Relaxed) {
            return Err(format!("{} crashed earlier and is switched off; rebuild it and rescan.", l.info.name));
        }
        let t = unsafe { &*l.table.0 };
        let inst = unsafe { (t.create)() };
        if inst.is_null() {
            return Err(format!("{} couldn't start.", l.info.name));
        }
        let mut tile = RawTile { pixels: px.as_mut_ptr() as *mut f32, width: w, height: h, x: origin.0, y: origin.1 };
        let code = unsafe { (t.process)(inst, &mut tile, values.as_ptr(), values.len() as u32) };
        unsafe { (t.destroy)(inst) };
        match code {
            0 => Ok(()),
            c if c == PANICKED => {
                l.poisoned.store(true, Ordering::Relaxed);
                Err(format!("{} crashed and was switched off. The picture is as it was.", l.info.name))
            }
            c => Err(format!("{} failed (code {c}).", l.info.name)),
        }
    }
}
