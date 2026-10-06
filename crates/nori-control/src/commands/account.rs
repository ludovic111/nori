//! `account.*`: lsuite AI, the subscription that makes the agent work without setup.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::account;
use crate::registry::{Args, Ctx};
use crate::session::{CmdResult, Event, Session, ToastKind};

/// What the window and the agent show: signed in or not, the plan, the allowance.
pub async fn status(refresh: bool) -> Value {
    match account::read() {
        None => json!({ "signedIn": false, "server": account::server() }),
        Some(a) => {
            let mut v = json!({ "signedIn": true, "account": a.public() });
            if refresh {
                match account::me(&a.server, &a.token).await {
                    Ok(me) => {
                        v["plan"] = me["plan"].clone();
                        v["status"] = me["status"].clone();
                        v["usage"] = me["usage"].clone();
                        v["models"] = me["models"].clone();
                        // Keep the file's plan up to date for the other apps.
                        if let Some(plan) = me["plan"].as_str()
                            && plan != a.plan
                        {
                            let mut b = a.clone();
                            b.plan = plan.to_string();
                            let _ = account::write(&b);
                        }
                    }
                    Err(e) => v["error"] = json!(e),
                }
            } else {
                v["plan"] = json!(a.plan);
            }
            v
        }
    }
}

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    match cx.spec.name {
        "account.status" => Ok(status(a.bool_or("refresh", true)).await),
        "account.plans" => account::plans(&account::server()).await,
        "account.signOut" => {
            if let Some(acc) = account::read() {
                let _ = reqwest::Client::new().post(format!("{}/api/account/signout", acc.server)).bearer_auth(&acc.token).timeout(std::time::Duration::from_secs(8)).send().await;
            }
            account::remove();
            s.emit(Event::AccountChanged);
            Ok(json!({ "signedIn": false }))
        }
        "account.signIn" => {
            if let Some(key) = a.opt_str("key") {
                let acc = account::sign_in_with_key(key).await?;
                s.emit(Event::AccountChanged);
                return Ok(json!({ "signedIn": true, "account": acc.public() }));
            }
            let (url, wait) = account::start_loopback().await?;
            account::open_url(&url).ok();
            // One sign-in at a time: a new one replaces the one waiting.
            if let Some(h) = s.sign_in.lock().take() {
                h.abort();
            }
            let s2 = s.clone();
            let h = tokio::spawn(async move {
                match wait.await {
                    Ok(acc) => {
                        s2.emit(Event::AccountChanged);
                        s2.toast(ToastKind::Success, format!("Signed in to lsuite AI as {}.", acc.email));
                    }
                    Err(e) => s2.toast(ToastKind::Error, e),
                }
            });
            *s.sign_in.lock() = Some(h);
            Ok(json!({ "opened": url, "waiting": true, "hint": "Finish signing in in the browser; nori connects when you press Connect nori." }))
        }
        _ => Err(super::unhandled(cx)),
    }
}
