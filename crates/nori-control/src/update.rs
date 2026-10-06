//! Updates: nori looks for a newer release on GitHub Releases (`app.checkUpdates`, and once at
//! start unless `NORI_NO_UPDATE=1` or the setting is off). Installing signed updates in place
//! (kimchi's updater, minisign-verified) comes with nori's first published release; until then
//! the window offers the download page.

use std::time::Duration;

use serde_json::{Value, json};

pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
pub const RELEASES: &str = "https://api.github.com/repos/ludovic111/nori/releases/latest";
pub const DOWNLOAD_PAGE: &str = "https://lsuite.xyz/nori";

/// Whether `a` is a newer version than `b` (semver; a leading `v` is fine).
pub fn is_newer(a: &str, b: &str) -> bool {
    let p = |v: &str| semver::Version::parse(v.trim().trim_start_matches('v')).ok();
    match (p(a), p(b)) {
        (Some(x), Some(y)) => x > y,
        _ => false,
    }
}

pub async fn check() -> Result<Value, String> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(15)).user_agent(concat!("nori/", env!("CARGO_PKG_VERSION"))).build().map_err(|e| e.to_string())?;
    let r = client.get(RELEASES).send().await.map_err(|e| format!("Couldn't reach GitHub: {e}"))?;
    if r.status().as_u16() == 404 {
        return Ok(json!({ "current": CURRENT, "available": null, "note": "nori has no published release yet." }));
    }
    let v: Value = r.json().await.map_err(|e| e.to_string())?;
    let tag = v["tag_name"].as_str().unwrap_or("").trim_start_matches('v').to_string();
    let newer = is_newer(&tag, CURRENT);
    Ok(json!({ "current": CURRENT, "latest": tag, "available": newer.then_some(tag.clone()), "download": DOWNLOAD_PAGE }))
}

#[cfg(test)]
mod tests {
    #[test]
    fn versions_compare() {
        assert!(super::is_newer("0.2.0", "0.1.9"));
        assert!(super::is_newer("v1.0.0", "0.9.0"));
        assert!(!super::is_newer("0.1.0", "0.1.0"));
        assert!(!super::is_newer("x", "0.1.0"));
    }
}
