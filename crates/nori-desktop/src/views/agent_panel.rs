//! The Agent panel (⌘J): the conversation with the built-in agent, one card per command it (or
//! an MCP client, or the CLI) ran, how each run ended, and "Revert this run". The agent runs
//! lsuite AI or the person's own model (`nori-agent`); this panel only draws it and sends.

use gpui::{AnyElement, Context, Entity, FontWeight, MouseButton, Render, ScrollHandle, Subscription, Window, div, prelude::*, px};
use nori_agent::{Entry, RunState, Snapshot};
use serde_json::json;

use crate::store::{Dialog, Store, StoreExt};
use crate::theme::{ActiveTheme, MONO, size as sz};
use crate::ui::input::{InputEvent, TextInput};
use crate::ui::{Button, GlassExt, caps, icon, logo};

pub struct AgentPanel {
    store: Entity<Store>,
    composer: Entity<TextInput>,
    snapshot: Snapshot,
    scroll: ScrollHandle,
    _subs: Vec<Subscription>,
    _watch: gpui::Task<()>,
}

/// A command in words, for its card.
fn words(command: &str) -> String {
    let w = crate::app::step_name(command);
    let mut c = w.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

impl AgentPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = cx.store();
        let composer = cx.new(|cx| {
            let mut i = TextInput::new(cx).multiline(2).placeholder("Ask the agent: \"make a poster for Friday's concert\", \"warm this photo up\"…");
            i.submit_on_enter = true;
            i
        });
        let subs = vec![
            cx.observe(&store, |_, _, cx| cx.notify()),
            cx.subscribe_in(&composer, window, |this: &mut Self, input, e: &InputEvent, _, cx| {
                if let InputEvent::Submit = e {
                    let text = input.read(cx).text().trim().to_string();
                    if !text.is_empty() {
                        input.update(cx, |i, cx| i.set_text("", cx));
                        this.send(text, cx);
                    }
                }
            }),
        ];
        let agent = store.read(cx).agent.clone();
        let mut rx = agent.subscribe();
        let watch = cx.spawn(async move |this, cx| {
            while rx.changed().await.is_ok() {
                if this
                    .update(cx, |p, cx| {
                        p.snapshot = p.store.read(cx).agent.snapshot();
                        p.scroll.scroll_to_bottom();
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        let snapshot = agent.snapshot();
        Self { store, composer, snapshot, scroll: ScrollHandle::new(), _subs: subs, _watch: watch }
    }

    fn send(&mut self, text: String, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| s.run("agent.send", json!({ "prompt": text }), cx));
    }

    fn provider_line(&self, cx: &Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let s = self.store.read(cx);
        let p = s.settings.agent.provider.clone();
        let name = match p.as_str() {
            "lsuite" => "lsuite AI",
            "claude-code" => "Claude Code",
            "codex" => "Codex",
            "anthropic" => "Anthropic API",
            "openai" => "OpenAI API",
            "ollama" => "Ollama",
            _ => "OpenAI-compatible",
        };
        let detail = if p == "lsuite" {
            let acc = &s.account;
            if acc["signedIn"] != json!(true) {
                "Not signed in".to_string()
            } else {
                let plan = acc["plan"].as_str().or(acc["account"]["plan"].as_str()).unwrap_or("").to_string();
                match (acc["usage"]["used"].as_f64(), acc["usage"]["limit"].as_f64()) {
                    (Some(u), Some(l)) if l > 0.0 => format!("{} · {:.0} % used", if plan.is_empty() { "No plan".into() } else { plan }, u / l * 100.0),
                    _ => plan,
                }
            }
        } else {
            s.settings.agent.model.clone()
        };
        let signed_out = p == "lsuite" && s.account["signedIn"] != json!(true);
        let pid = p.clone();
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(12.))
            .py(px(8.))
            .border_b_1()
            .border_color(t.line)
            .child(logo(&pid, px(18.)))
            .child(div().flex_1().min_w_0().flex().flex_col().child(div().text_size(px(sz::SM)).font_weight(FontWeight::MEDIUM).child(name)).when(!detail.is_empty(), |d| d.child(div().font_family(MONO).text_size(px(10.)).text_color(t.text_2).truncate().child(detail))))
            .when(signed_out, |d| d.child(Button::new("agent-sign-in", "Sign in").small().primary().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("account.signIn", json!({}), cx)))))
            .child(Button::new("agent-provider", "Change").small().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.open_dialog(Dialog::Settings { section: Some("agent".into()) }, cx))))
            .into_any_element()
    }

    fn entry(&self, i: usize, e: &Entry, cx: &Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        match e {
            Entry::User { text, source, .. } => div()
                .flex()
                .justify_end()
                .child(
                    div()
                        .max_w(px(300.))
                        .px(px(10.))
                        .py(px(8.))
                        .bg(t.bg_sunken)
                        .border_1()
                        .border_color(t.line_strong)
                        .text_size(px(sz::SM))
                        .child(text.clone())
                        .when(!matches!(source, nori_control::Source::Window), |d| d.child(div().mt(px(4.)).font_family(MONO).text_size(px(9.)).text_color(t.text_3).child(source.as_str().to_uppercase()))),
                )
                .into_any_element(),
            Entry::Assistant { text, .. } => div().text_size(px(sz::SM)).child(crate::ui::markdown::render(text, cx)).into_any_element(),
            Entry::Command { record, .. } => {
                let ok = record.ok;
                div()
                    .id(("card", i))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(10.))
                    .py(px(6.))
                    .border_1()
                    .border_color(t.line)
                    .bg(t.bg_raised.opacity(0.6))
                    .child(icon(if ok { "check" } else { "circle-alert" }).text_color(if ok { t.text_2 } else { t.danger }))
                    .child(div().flex_1().min_w_0().flex().flex_col().child(div().text_size(px(sz::SM)).child(words(&record.command))).child(div().font_family(MONO).text_size(px(10.)).text_color(t.text_3).truncate().child(record.command.clone())).when_some(record.error.clone(), |d, e| d.child(div().text_size(px(sz::XS)).text_color(t.danger).child(e))))
                    .when(record.source != nori_control::Source::Agent, |d| d.child(div().font_family(MONO).text_size(px(9.)).text_color(t.text_3).child(record.source.as_str().to_uppercase())))
                    .into_any_element()
            }
            Entry::Outcome { state, error, changes, run, .. } => {
                let allowance = error.as_deref().is_some_and(|e| e.to_lowercase().contains("allowance") || e.to_lowercase().contains("plan"));
                let run = *run;
                let reverted = self.snapshot.run(run).is_some_and(|r| !r.can_revert());
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(sz::XS))
                    .text_color(if *state == RunState::Error { t.danger } else { t.text_3 })
                    .child(div().flex_1().child(match state {
                        RunState::Done => format!("Done · {changes} change{}", if *changes == 1 { "" } else { "s" }),
                        RunState::Cancelled => "Stopped".to_string(),
                        RunState::Error => error.clone().unwrap_or_else(|| "It went wrong.".into()),
                        RunState::Running => "Working…".into(),
                    }))
                    .when(allowance, |d| {
                        let url = format!("{}/account", nori_control::account::server());
                        d.child(Button::new(("manage", i), "Manage plan").small().on_click(move |_, _, cx| cx.open_url(&url)))
                    })
                    .when(*changes > 0 && !reverted, |d| d.child(Button::new(("revert", i), "Revert this run").small().on_click(move |_, _, cx| cx.store().update(cx, |s, cx| s.run("agent.revert", json!({ "run": run }), cx)))))
                    .into_any_element()
            }
        }
    }
}

impl Render for AgentPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let running = self.snapshot.running.is_some();
        let entries: Vec<AnyElement> = self.snapshot.entries.iter().enumerate().map(|(i, e)| self.entry(i, e, cx)).collect();
        let empty = entries.is_empty();
        div()
            .size_full()
            .flex()
            .flex_col()
            .glass(t.glass1)
            .border_0()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(44.))
                    .px(px(12.))
                    .border_b_1()
                    .border_color(t.line)
                    .child(crate::ui::panel_title("Agent"))
                    .child(div().flex_1())
                    .child(Button::icon("agent-new", "plus", "New conversation").small().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("agent.newConversation", json!({}), cx))))
                    .child(Button::icon("agent-close", "x", "Close (⌘J)").small().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.set_agent_open(false, cx)))),
            )
            .child(self.provider_line(cx))
            .child(
                div()
                    .id("agent-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .p(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .when(empty, |d| {
                        d.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(8.))
                                .mt(px(24.))
                                .child(caps("Try", cx))
                                .children(["Warm this photo up and add a soft vignette", "Make a poster: a big title, the date, our logo", "Set the text in two columns with a heading style", "Write a plugin that adds film grain"].map(|ex| {
                                    div()
                                        .id(ex)
                                        .px(px(10.))
                                        .py(px(8.))
                                        .border_1()
                                        .border_color(t.line)
                                        .text_size(px(sz::SM))
                                        .text_color(t.text_2)
                                        .cursor_pointer()
                                        .hover(|s| s.border_color(t.line_strong).text_color(t.text))
                                        .on_click(cx.listener(move |p, _, _, cx| p.send(ex.to_string(), cx)))
                                        .child(ex)
                                })),
                        )
                    })
                    .children(entries)
                    .when(running, |d| d.child(div().flex().items_center().gap(px(6.)).text_size(px(sz::XS)).text_color(t.text_2).child(icon("loader-circle")).child("Working…"))),
            )
            .child(
                div()
                    .flex_none()
                    .p(px(10.))
                    .border_t_1()
                    .border_color(t.line)
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(self.composer.clone())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().flex_1().text_size(px(sz::XS)).text_color(t.text_3).child("Every change is one step you can undo."))
                            .child(if running {
                                Button::new("agent-stop", "Stop").small().danger().with_icon("square-stop").on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("agent.stop", json!({}), cx))).into_any_element()
                            } else {
                                Button::new("agent-send", "Send").small().primary().with_icon("send").on_click(cx.listener(|p, _, _, cx| {
                                    let text = p.composer.read(cx).text().trim().to_string();
                                    if !text.is_empty() {
                                        p.composer.update(cx, |i, cx| i.set_text("", cx));
                                        p.send(text, cx);
                                    }
                                })).into_any_element()
                            }),
                    ),
            )
    }
}
