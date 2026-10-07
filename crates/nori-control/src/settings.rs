//! App settings, saved as `settings.json` in the config folder.
//!
//! `agent.permissions` is the one place agent and MCP requests are checked against
//! (see [`crate::registry::Perm`]); off means off for both.

use std::path::Path;

use nori_render::brush::Brush;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
#[derive(Default)]
pub struct Settings {
    pub agent: AgentSettings,
    pub updates: UpdateSettings,
    pub appearance: Appearance,
    pub diagnostics: DiagnosticsSettings,
    pub onboarding: OnboardingSettings,
    pub tools: ToolSettings,
    pub history: HistorySettings,
    pub plugins: PluginSettings,
    /// Recently opened or saved files, newest first.
    pub recent: Vec<String>,
}


/// The first-run setup (`app.onboarding`, `app.finishOnboarding`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct OnboardingSettings {
    /// The nori version the setup was finished or skipped in; empty: never (it shows at start).
    pub completed: String,
    /// The editors the person said they came from (`nori_io::APPS` ids).
    pub coming_from: Vec<String>,
}

/// What can run the built-in agent (`settings.agent.provider`): lsuite AI (the subscription,
/// no setup), the person's coding CLIs, model APIs, and local servers.
pub const AGENT_PROVIDERS: &[&str] = &["lsuite", "claude-code", "codex", "anthropic", "openai", "ollama", "openai-compatible"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AgentSettings {
    /// The Agent panel is offered (off hides it; MCP and the CLI still work).
    pub enabled: bool,
    pub permissions: Permissions,
    /// Which model runs the built-in agent, one of [`AGENT_PROVIDERS`].
    pub provider: String,
    /// Model id (empty: the provider's default).
    pub model: String,
    /// Base URL for Ollama and OpenAI-compatible servers.
    pub base_url: String,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self { enabled: true, permissions: Permissions::default(), provider: "lsuite".into(), model: String::new(), base_url: String::new() }
    }
}

/// What an agent (the built-in one, MCP clients, `nori-cli --agent`) may do besides editing the
/// open document, which is always allowed and always undoable.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Permissions {
    /// Master switch: off refuses every agent and MCP request.
    pub enabled: bool,
    /// Open, save, export, place files.
    pub files: bool,
    /// Change settings other than these permissions and API keys.
    pub settings: bool,
    /// Quit the app, install an update.
    pub app_control: bool,
    /// Write, build, install and remove plugins (runs a compiler; off for agents until the
    /// person allows it, as PLUGINS.md asks).
    pub plugins: bool,
}

impl Default for Permissions {
    fn default() -> Self {
        Self { enabled: true, files: true, settings: false, app_control: false, plugins: false }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct UpdateSettings {
    /// Install verified updates in the background when enabled.
    pub auto_install: bool,
    /// Check GitHub Releases when the app starts. `NORI_NO_UPDATE=1` also turns it off.
    pub check_on_start: bool,
    /// Show what's new once after nori updates.
    pub show_whats_new: bool,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self { auto_install: false, check_on_start: true, show_whats_new: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct DiagnosticsSettings {
    /// `info`, `debug` or `trace`. `RUST_LOG` wins when set.
    pub log_level: String,
}

impl Default for DiagnosticsSettings {
    fn default() -> Self {
        Self { log_level: "info".into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Appearance {
    /// `system`, `dark` or `light`.
    pub mode: String,
    /// Glass chrome; off gives opaque surfaces (also follows the OS "reduce transparency").
    pub transparency: bool,
    /// The canvas's surround: `dark`, `mid` or `light` grey (whatever the interface mode).
    pub pasteboard: String,
}

impl Default for Appearance {
    fn default() -> Self {
        Self { mode: "system".into(), transparency: true, pasteboard: "dark".into() }
    }
}

/// The tools' last settings (as other editors remember them).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct ToolSettings {
    pub brush: Brush,
    pub eraser: Brush,
    /// Paint bucket and magic wand.
    pub tolerance: f64,
    pub contiguous: bool,
    pub sample_all: bool,
    /// Gradient: `linear`, `radial` or `reflected`.
    pub gradient: String,
    /// Shapes: what new shapes look like.
    pub shape_fill: String,
    pub shape_stroke: String,
    pub shape_stroke_width: f64,
    /// Text: the font and size new text starts with.
    pub font: String,
    pub font_size: f64,
}

impl Default for ToolSettings {
    fn default() -> Self {
        Self {
            brush: Brush::default(),
            eraser: Brush { hardness: 0.9, ..Brush::default() },
            tolerance: 32.0,
            contiguous: true,
            sample_all: false,
            gradient: "linear".into(),
            shape_fill: "#1a1a1a".into(),
            shape_stroke: String::new(),
            shape_stroke_width: 2.0,
            font: "Manrope".into(),
            font_size: 72.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct HistorySettings {
    /// Undo steps kept.
    pub steps: u32,
    /// Most memory (MB) the undo history keeps for pixels.
    pub memory_mb: u32,
}

impl Default for HistorySettings {
    fn default() -> Self {
        Self { steps: 100, memory_mb: 2048 }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct PluginSettings {
    /// Plugin ids switched off (`plugin.disable`); stock ones included.
    pub disabled: Vec<String>,
    /// More folders to look for LUTs, brushes and plugin bundles in.
    pub folders: Vec<String>,
}

impl Settings {
    /// Reads `settings.json`. A file that can't be read is kept as `settings.json.bad` and the
    /// defaults are used.
    pub fn load(dir: &Path) -> Self {
        let path = dir.join("settings.json");
        let Ok(bytes) = std::fs::read(&path) else { return Self::default() };
        match serde_json::from_slice(&bytes) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("{} isn't valid ({e}); kept as settings.json.bad, using the defaults", path.display());
                let _ = std::fs::rename(&path, dir.join("settings.json.bad"));
                Self::default()
            }
        }
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        let tmp = dir.join("settings.json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(tmp, dir.join("settings.json"))
    }

    pub fn update_check_enabled(&self) -> bool {
        self.updates.check_on_start && !std::env::var("NORI_NO_UPDATE").is_ok_and(|v| !v.is_empty() && v != "0")
    }

    pub fn get(&self, key: &str) -> Option<Value> {
        let mut v = serde_json::to_value(self).ok()?;
        for part in key.split('.').filter(|p| !p.is_empty()) {
            v = v.get(part)?.clone();
        }
        Some(v)
    }

    /// Sets a setting by dotted key, keeping the value's type.
    pub fn set(&mut self, key: &str, value: Value) -> Result<(), String> {
        let mut root = serde_json::to_value(&*self).map_err(|e| e.to_string())?;
        let parts: Vec<&str> = key.split('.').filter(|p| !p.is_empty()).collect();
        let (last, path) = parts.split_last().ok_or("empty setting key")?;
        let mut node = &mut root;
        for p in path {
            node = node.get_mut(*p).ok_or_else(|| unknown(key))?;
        }
        let slot = node.get_mut(*last).ok_or_else(|| unknown(key))?;
        let same_type = matches!((&*slot, &value), (Value::Bool(_), Value::Bool(_)) | (Value::String(_), Value::String(_)) | (Value::Number(_), Value::Number(_)))
            || matches!((&*slot, &value), (Value::Array(_), Value::Array(items)) if items.iter().all(Value::is_string));
        if !same_type {
            return Err(format!("`{key}` expects {}, got {value}", type_name(slot)));
        }
        if let Some(allowed) = choices(key)
            && !value.as_str().is_some_and(|v| allowed.contains(&v))
        {
            return Err(format!("`{key}` is one of {}, not {value}.", allowed.iter().map(|a| format!("\"{a}\"")).collect::<Vec<_>>().join(", ")));
        }
        if let (Some((lo, hi)), Some(v)) = (range(key), value.as_f64())
            && !(lo..=hi).contains(&v)
        {
            return Err(format!("`{key}` goes from {lo} to {hi}, not {v}."));
        }
        *slot = value;
        *self = serde_json::from_value(root).map_err(|e| e.to_string())?;
        Ok(())
    }
}

pub fn choices(key: &str) -> Option<&'static [&'static str]> {
    Some(match key {
        "appearance.mode" => &["system", "dark", "light"],
        "appearance.pasteboard" => &["dark", "mid", "light"],
        "agent.provider" => AGENT_PROVIDERS,
        "diagnostics.logLevel" => crate::diagnostics::LEVELS,
        "tools.gradient" => &["linear", "radial", "reflected"],
        _ => return None,
    })
}

pub fn range(key: &str) -> Option<(f64, f64)> {
    Some(match key {
        "tools.tolerance" => (0.0, 255.0),
        "history.steps" => (1.0, 1000.0),
        "history.memoryMb" => (64.0, 65536.0),
        "tools.fontSize" => (1.0, 4000.0),
        _ => return None,
    })
}

fn unknown(key: &str) -> String {
    format!("Unknown setting `{key}`. app.settings lists them.")
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Bool(_) => "true or false",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "a list of strings",
        _ => "an object",
    }
}
