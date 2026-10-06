//! HTTP for the in-process providers: one client, retried POSTs, and a bounded line reader for
//! SSE and NDJSON streams.

use std::time::Duration;

use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::tools::bounded;

/// Longest stream line (one SSE `data:` record or NDJSON object).
const LINE_LIMIT: usize = 8 * 1024 * 1024;

/// lsuite AI's own refusals (AI.md): their message is one line to show as it is, and the person
/// can do something about it on the account page (`manage_url`). Never retried.
const LSUITE_REFUSALS: &[&str] = &["allowance_exhausted", "plan_required", "model_not_in_plan"];

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        // A stream may pause while the model thinks, but not forever.
        .read_timeout(Duration::from_secs(300))
        .user_agent(concat!("nori/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
}

/// A request that failed: the line to show, and what the service said about it.
#[derive(Debug, Clone)]
pub struct HttpError {
    pub message: String,
    pub status: Option<u16>,
    /// `error.type` from the body (`allowance_exhausted`, `authentication_error`…).
    pub kind: Option<String>,
    /// Where the plan is managed, for lsuite AI's refusals.
    pub manage_url: Option<String>,
}

impl HttpError {
    fn plain(message: String) -> Self {
        Self { message, status: None, kind: None, manage_url: None }
    }
}

impl From<HttpError> for String {
    fn from(e: HttpError) -> String {
        e.message
    }
}

/// POSTs `body` and returns the response once it is 2xx. Rate limits, overload and connection
/// failures are retried twice with a pause; other errors come back with the service's own
/// message (lsuite AI's refusals exactly as the server words them).
pub async fn post(cancel: &CancellationToken, label: &str, build: impl Fn() -> reqwest::RequestBuilder, body: &Value) -> Result<reqwest::Response, HttpError> {
    let mut attempt = 0;
    loop {
        attempt += 1;
        let retry = match build().json(body).send().await {
            Ok(r) if r.status().is_success() => return Ok(r),
            Ok(r) => {
                let status = r.status().as_u16();
                let text = r.text().await.unwrap_or_default();
                let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                let kind = v["error"]["type"].as_str().map(str::to_string);
                if let Some(k) = kind.as_deref().filter(|k| LSUITE_REFUSALS.contains(k)) {
                    let message = v["error"]["message"].as_str().map(one_line).unwrap_or_else(|| format!("{label} refused the request ({k})."));
                    return Err(HttpError { message, status: Some(status), kind, manage_url: v["error"]["manage_url"].as_str().map(str::to_string) });
                }
                let message = format!("{label} error {status}: {}", api_error(&text));
                if !matches!(status, 408 | 409 | 429 | 500 | 502 | 503 | 504 | 529) || attempt > 2 {
                    return Err(HttpError { message: hint(label, status, message), status: Some(status), kind, manage_url: None });
                }
                message
            }
            Err(e) if attempt > 2 || !(e.is_connect() || e.is_timeout()) => return Err(HttpError::plain(format!("Couldn't reach {label}: {e}"))),
            Err(e) => e.to_string(),
        };
        tracing::debug!("{label}: retrying after {retry}");
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(2 * attempt as u64)) => {}
            _ = cancel.cancelled() => return Err(HttpError::plain("Stopped".into())),
        }
    }
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn hint(label: &str, status: u16, message: String) -> String {
    let lsuite = label.starts_with("lsuite AI");
    match status {
        401 if lsuite => format!("{message}\nSign in to lsuite AI again in Settings › Account."),
        401 | 403 if !lsuite => format!("{message}\nCheck the API key in Settings › Agent."),
        404 => format!("{message}\nCheck the model name and address in Settings › Agent."),
        _ => message,
    }
}

/// The readable part of an error body (`{"error": {"message": …}}` and friends).
pub fn api_error(text: &str) -> String {
    let v: Option<Value> = serde_json::from_str(text).ok();
    v.as_ref()
        .and_then(|v| v["error"]["message"].as_str().or_else(|| v["error"].as_str()).or_else(|| v["message"].as_str()).map(str::to_string))
        .unwrap_or_else(|| bounded(text.trim(), 600))
}

/// Reads a response body line by line.
pub struct Lines {
    response: reqwest::Response,
    buf: Vec<u8>,
    done: bool,
}

impl Lines {
    pub fn new(response: reqwest::Response) -> Self {
        Self { response, buf: vec![], done: false }
    }

    /// The next line without its line ending; `None` at the end of the body.
    pub async fn next(&mut self) -> Result<Option<String>, String> {
        loop {
            if let Some(i) = self.buf.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = self.buf.drain(..=i).collect();
                return Ok(Some(String::from_utf8_lossy(&line).trim_end_matches(['\n', '\r']).to_string()));
            }
            if self.done {
                if self.buf.is_empty() {
                    return Ok(None);
                }
                let line = std::mem::take(&mut self.buf);
                return Ok(Some(String::from_utf8_lossy(&line).trim_end_matches('\r').to_string()));
            }
            match self.response.chunk().await {
                Ok(Some(chunk)) => {
                    if self.buf.len() + chunk.len() > LINE_LIMIT {
                        return Err("The response stream sent a record that is too large.".into());
                    }
                    self.buf.extend_from_slice(&chunk);
                }
                Ok(None) => self.done = true,
                Err(e) => return Err(format!("The response stream broke off: {e}")),
            }
        }
    }

    /// The next server-sent event's `data` (multi-line data joined), skipping comments and keep-alives.
    pub async fn next_sse(&mut self) -> Result<Option<String>, String> {
        let mut data = String::new();
        loop {
            match self.next().await? {
                None => return Ok((!data.is_empty()).then_some(data)),
                Some(line) if line.is_empty() => {
                    if !data.is_empty() {
                        return Ok(Some(data));
                    }
                }
                Some(line) => {
                    if let Some(d) = line.strip_prefix("data:") {
                        if !data.is_empty() {
                            data.push('\n');
                        }
                        data.push_str(d.strip_prefix(' ').unwrap_or(d));
                    }
                }
            }
        }
    }
}
