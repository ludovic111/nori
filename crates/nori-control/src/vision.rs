//! Pictures for agents that can see: the commands that draw something to a PNG (`page.look`,
//! `layer.look`, `ui.screenshot`) hand their picture to the model as well as its path.

use std::path::PathBuf;

use base64::Engine;
use serde_json::Value;

/// A picture ready to send to a model.
#[derive(Debug, Clone)]
pub struct Picture {
    pub media_type: &'static str,
    /// Base64.
    pub data: String,
}

/// The pictures a command's result points at.
pub fn pictures_in(command: &str, result: &Value) -> Vec<PathBuf> {
    match command {
        "page.look" | "layer.look" | "ui.screenshot" => result.get("path").and_then(Value::as_str).map(PathBuf::from).into_iter().collect(),
        _ => vec![],
    }
}

/// Reads a PNG for a model (at most 8 MB).
pub async fn picture(path: &std::path::Path) -> Result<Picture, String> {
    let bytes = tokio::fs::read(path).await.map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() > 8 << 20 {
        return Err("the picture is over 8 MB".into());
    }
    Ok(Picture { media_type: "image/png", data: base64::engine::general_purpose::STANDARD.encode(bytes) })
}
