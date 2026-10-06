//! The lsuite account (lsuite AI, see lsuite's AI.md): one account for every lsuite app on the
//! computer, in `~/.lsuite/account.json` (0600, written atomically, read when needed since
//! another app can change it). Signing in goes through the browser and a loopback callback,
//! as native apps do OAuth; a key (`lsk_…`) pasted from the account page works too.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::session::lsuite_home;

pub const DEFAULT_SERVER: &str = "https://lsuite.xyz";
const APP: &str = "nori";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub format: u32,
    pub server: String,
    pub email: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub plan: String,
    pub token: String,
    #[serde(default)]
    pub signed_in_at: String,
}

impl Account {
    /// The account without its secret, for clients.
    pub fn public(&self) -> Value {
        json!({ "server": self.server, "email": self.email, "name": self.name, "plan": self.plan, "signedInAt": self.signed_in_at, "token": mask(&self.token) })
    }
}

/// `lsk_ab…xyz` → `lsk_…xyz`: never the whole token.
pub fn mask(token: &str) -> String {
    let n = token.chars().count();
    if n <= 8 { "…".into() } else { format!("{}…{}", token.chars().take(4).collect::<String>(), token.chars().skip(n - 4).collect::<String>()) }
}

pub fn path() -> PathBuf {
    lsuite_home().join("account.json")
}

/// The server: `LSUITE_ACCOUNT_SERVER`, else the account's own, else lsuite.xyz.
pub fn server() -> String {
    if let Ok(s) = std::env::var("LSUITE_ACCOUNT_SERVER")
        && !s.trim().is_empty()
    {
        return s.trim().trim_end_matches('/').to_string();
    }
    read().map(|a| a.server).filter(|s| !s.is_empty()).unwrap_or_else(|| DEFAULT_SERVER.into())
}

/// The signed-in account, if any (a file in another format is ignored).
pub fn read() -> Option<Account> {
    let bytes = std::fs::read(path()).ok()?;
    let a: Account = serde_json::from_slice(&bytes).ok()?;
    (a.format == 1 && !a.token.is_empty()).then_some(a)
}

pub fn write(a: &Account) -> Result<(), String> {
    let p = path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = p.with_extension("json.tmp");
    let _ = std::fs::remove_file(&tmp);
    let bytes = serde_json::to_vec_pretty(a).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp).map_err(|e| e.to_string())?;
        f.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    std::fs::write(&tmp, &bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
}

pub fn remove() {
    let _ = std::fs::remove_file(path());
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().timeout(Duration::from_secs(20)).user_agent(concat!("nori/", env!("CARGO_PKG_VERSION"))).build().unwrap_or_default()
}

/// `GET /api/account/me` with the token: plan, status, usage, models.
pub async fn me(server: &str, token: &str) -> Result<Value, String> {
    let r = client().get(format!("{server}/api/account/me")).bearer_auth(token).send().await.map_err(|e| format!("Couldn't reach {server}: {e}"))?;
    let status = r.status();
    let v: Value = r.json().await.unwrap_or(Value::Null);
    if status.as_u16() == 401 {
        return Err("The lsuite sign-in has expired: sign in again.".into());
    }
    if !status.is_success() {
        return Err(format!("{server} answered {status}: {}", v.get("error").and_then(Value::as_str).unwrap_or("")));
    }
    Ok(v)
}

pub async fn plans(server: &str) -> Result<Value, String> {
    let r = client().get(format!("{server}/api/ai/plans")).send().await.map_err(|e| format!("Couldn't reach {server}: {e}"))?;
    if !r.status().is_success() {
        return Err(format!("{server} has no lsuite AI plans ({}).", r.status()));
    }
    r.json().await.map_err(|e| e.to_string())
}

/// Turns a callback code into a token and the account (`POST /api/account/token`).
pub async fn exchange(server: &str, code: &str) -> Result<Account, String> {
    let r = client().post(format!("{server}/api/account/token")).json(&json!({ "code": code })).send().await.map_err(|e| format!("Couldn't reach {server}: {e}"))?;
    let status = r.status();
    let v: Value = r.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(v.get("error").and_then(Value::as_str).unwrap_or("The sign-in code was refused.").to_string());
    }
    let token = v["token"].as_str().ok_or("The server sent no token.")?.to_string();
    let acc = &v["account"];
    Ok(Account {
        format: 1,
        server: server.into(),
        email: acc["email"].as_str().unwrap_or("").into(),
        name: acc["name"].as_str().unwrap_or("").into(),
        plan: acc["plan"].as_str().unwrap_or("").into(),
        token,
        signed_in_at: chrono::Utc::now().to_rfc3339(),
    })
}

/// Signs in with a key from the account page.
pub async fn sign_in_with_key(key: &str) -> Result<Account, String> {
    let server = server();
    let key = key.trim();
    if !key.starts_with("lsk_") {
        return Err("A key from the lsuite account page starts with lsk_.".into());
    }
    let v = me(&server, key).await?;
    let a = Account {
        format: 1,
        server: server.clone(),
        email: v["email"].as_str().unwrap_or("").into(),
        name: v["name"].as_str().unwrap_or("").into(),
        plan: v["plan"].as_str().unwrap_or("").into(),
        token: key.into(),
        signed_in_at: chrono::Utc::now().to_rfc3339(),
    };
    write(&a)?;
    Ok(a)
}

/// A random `state` for the loopback sign-in.
fn state() -> String {
    crate::bridge::token()[..32].to_string()
}

/// Opens a URL in the person's browser.
pub fn open_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    r.map(|_| ()).map_err(|e| format!("Couldn't open the browser ({e}). Open {url} yourself."))
}

/// Starts the loopback sign-in: listens on 127.0.0.1, returns the URL to open and a future that
/// finishes when the browser comes back (or after ten minutes).
pub async fn start_loopback() -> Result<(String, impl std::future::Future<Output = Result<Account, String>>), String> {
    let server = server();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let st = state();
    let url = format!("{server}/account/connect?app={APP}&port={port}&state={st}");
    let fut = async move {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(600);
        loop {
            let accepted = tokio::time::timeout_at(deadline, listener.accept()).await.map_err(|_| "The sign-in timed out.".to_string())?;
            let (mut stream, peer) = accepted.map_err(|e| e.to_string())?;
            if !peer.ip().is_loopback() {
                continue;
            }
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut buf = vec![0u8; 8192];
            let n = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buf)).await.map_err(|_| "timeout".to_string())?.map_err(|e| e.to_string())?;
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let target = req.split_whitespace().nth(1).unwrap_or("/").to_string();
            let Some(query) = target.strip_prefix("/callback?") else {
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                continue;
            };
            let params: std::collections::HashMap<String, String> = url::form_urlencoded::parse(query.as_bytes()).into_owned().collect();
            let page = |title: &str, line: &str| {
                let body = format!(
                    "<!doctype html><meta charset=utf-8><title>{title}</title><body style=\"font:16px system-ui;background:#050505;color:#f2f2f2;display:grid;place-items:center;height:100vh;margin:0\"><div><h1 style=\"font-weight:600\">{title}</h1><p>{line}</p></div>"
                );
                format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
            };
            if params.get("state").map(String::as_str) != Some(st.as_str()) {
                let _ = stream.write_all(page("That didn't work", "The sign-in didn't come from nori. Try again from nori.").as_bytes()).await;
                return Err("The sign-in came back with the wrong state; try again.".into());
            }
            let Some(code) = params.get("code").cloned() else {
                let _ = stream.write_all(page("Not connected", "No code came back. Try again from nori.").as_bytes()).await;
                return Err("The sign-in was cancelled.".into());
            };
            match exchange(&server, &code).await {
                Ok(acc) => {
                    write(&acc)?;
                    let _ = stream.write_all(page("nori is connected", "Your lsuite AI works in every lsuite app now. You can close this tab.").as_bytes()).await;
                    return Ok(acc);
                }
                Err(e) => {
                    let _ = stream.write_all(page("Not connected", &e).as_bytes()).await;
                    return Err(e);
                }
            }
        }
    };
    Ok((url, fut))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_tokens() {
        assert_eq!(mask("lsk_abcdefghijkl"), "lsk_…ijkl");
        assert_eq!(mask("short"), "…");
    }
}
