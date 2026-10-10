//! The first-run setup: what nori is, which editors the person comes from (with what nori opens
//! from each), and which of the person's own models runs the agent. Everything here can be
//! changed later in Settings; skipping is fine.

use gpui::{AnyElement, Context, Entity, FontWeight, MouseButton, Render, SharedString, Subscription, Window, div, prelude::*, px};
use serde_json::json;

use crate::store::{Store, StoreExt};
use crate::theme::{ActiveTheme, MONO, size as sz};
use crate::ui::{Button, GlassExt, caps, icon, logo};

pub struct Onboarding {
    store: Entity<Store>,
    picked: Vec<&'static str>,
    _sub: Subscription,
}

impl Onboarding {
    pub fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = cx.store();
        let sub = cx.observe(&store, |_, _, cx| cx.notify());
        Self { store, picked: vec![], _sub: sub }
    }

    fn step(&self, cx: &Context<Self>) -> String {
        self.store.read(cx).setup.clone().unwrap_or_else(|| "welcome".into())
    }

    fn go(&self, step: &str, cx: &mut Context<Self>) {
        let step = step.to_string();
        self.store.update(cx, |s, cx| {
            s.setup = Some(step);
            cx.notify();
        });
    }

    fn finish(&self, cx: &mut Context<Self>) {
        let from = self.picked.clone();
        self.store.update(cx, |s, cx| {
            s.run("app.finishOnboarding", json!({ "comingFrom": from }), cx);
            s.close_setup(cx);
        });
    }
}

impl Render for Onboarding {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let step = self.step(cx);
        let steps = ["welcome", "comingFrom", "agent"];
        let at = steps.iter().position(|s| *s == step).unwrap_or(0);
        let content: AnyElement = match step.as_str() {
            "comingFrom" => {
                let picked = self.picked.clone();
                let apps = div().flex().flex_wrap().gap(px(10.)).children(nori_io::APPS.iter().map(|a| {
                    let on = picked.contains(&a.id);
                    let id = a.id;
                    div()
                        .id(SharedString::from(format!("coming-{id}")))
                        .w(px(196.))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .p(px(10.))
                        .border_1()
                        .border_color(if on { t.accent } else { t.line })
                        .when(on, |d| d.bg(t.accent_soft))
                        .cursor_pointer()
                        .hover(|s| s.border_color(t.line_strong))
                        .on_click(cx.listener(move |o, _, _, cx| {
                            if let Some(i) = o.picked.iter().position(|x| *x == id) {
                                o.picked.remove(i);
                            } else {
                                o.picked.push(id);
                            }
                            cx.notify();
                        }))
                        .child(logo(id, px(26.)))
                        .child(div().flex_1().min_w_0().flex().flex_col().child(div().font_weight(FontWeight::MEDIUM).child(a.name)).child(div().font_family(MONO).text_size(px(10.)).text_color(t.text_2).child(format!("opens {}", a.formats.join(", ").to_uppercase()))))
                        .when(on, |d| d.child(icon("check")))
                }));
                let how: Vec<AnyElement> = picked
                    .iter()
                    .filter_map(|id| nori_io::app(id))
                    .map(|a| div().flex().gap(px(8.)).text_size(px(sz::SM)).child(logo(a.id, px(16.))).child(div().text_color(t.text_2).child(a.how)).into_any_element())
                    .collect();
                div()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(div().text_size(px(sz::XXL)).font_weight(FontWeight::SEMIBOLD).child("Coming from another editor?"))
                    .child(div().text_color(t.text_2).child("Pick the ones you use: nori shows how to bring your work over. Only apps whose files nori really opens are here."))
                    .child(apps)
                    .when(!how.is_empty(), |d| d.child(div().flex().flex_col().gap(px(6.)).child(caps("Bringing your work over", cx)).children(how)))
                    .into_any_element()
            }
            "agent" => {
                let provider = self.store.read(cx).settings.agent.provider.clone();
                let providers: [(&str, &str); 5] = [("claude-code", "Claude Code"), ("codex", "Codex"), ("anthropic", "Anthropic API"), ("openai", "OpenAI API"), ("ollama", "Ollama")];
                div()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(div().text_size(px(sz::XXL)).font_weight(FontWeight::SEMIBOLD).child("Your agent"))
                    .child(div().text_color(t.text_2).child("Ask nori's agent to retouch, draw, lay out a page or write a plugin. Everything it does is a step you can undo."))
                    .child(caps("What runs it", cx))
                    .child(div().flex().flex_wrap().gap(px(8.)).children(providers.iter().map(|(id, name)| {
                        let on = provider == *id;
                        let pid = id.to_string();
                        div()
                            .id(SharedString::from(format!("onb-{id}")))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .h(px(36.))
                            .px(px(12.))
                            .border_1()
                            .border_color(if on { t.accent } else { t.line })
                            .cursor_pointer()
                            .hover(|s| s.bg(t.hover))
                            .on_click(move |_, _, cx| { let p = pid.clone(); cx.store().update(cx, |s, cx| s.run("agent.setProvider", json!({ "provider": p }), cx)) })
                            .child(logo(id, px(18.)))
                            .child(*name)
                            .when(on, |d| d.child(icon("check")))
                    })))
                    .child(div().text_size(px(sz::SM)).text_color(t.text_2).child("Keys go in Settings › Agent. You can change all of this later."))
                    .into_any_element()
            }
            _ => div()
                .flex()
                .flex_col()
                .gap(px(18.))
                .child(crate::views::home::mark(64., cx))
                .child(div().text_size(px(40.)).font_weight(FontWeight::SEMIBOLD).child("Welcome to nori"))
                .child(div().text_size(px(sz::MD)).text_color(t.text_2).child("Pictures, drawings and pages in one app: retouch photos with layers and adjustments, draw with the pen and shapes, lay out posters and booklets with text that flows across pages. Part of lsuite — free and open source."))
                .child(div().flex().gap(px(14.)).children([("image", "Photos & painting"), ("pen-tool", "Vectors"), ("book-open", "Pages & layout"), ("bot", "An agent that edits with you")].map(|(ic, label)| div().flex().items_center().gap(px(8.)).text_size(px(sz::SM)).child(icon(ic)).child(label))))
                .into_any_element(),
        };
        let last = at + 1 == steps.len();
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .relative()
                    .child(
                        div()
                            .w(px(760.))
                            .flex()
                            .flex_col()
                            .glass(t.glass3)
                            .shadow(t.glass_shadow())
                            .child(div().p(px(36.)).min_h(px(420.)).child(content))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .px(px(36.))
                                    .py(px(16.))
                                    .border_t_1()
                                    .border_color(t.line)
                                    .child(div().flex().gap(px(6.)).children(steps.iter().enumerate().map(|(i, _)| div().w(px(18.)).h(px(4.)).bg(if i <= at { t.text } else { t.line_strong }))))
                                    .child(div().flex_1())
                                    .child(Button::new("onb-skip", "Skip").ghost().on_click(cx.listener(|o, _, _, cx| o.finish(cx))))
                                    .when(at > 0, |d| d.child(Button::new("onb-back", "Back").on_click(cx.listener(move |o, _, _, cx| o.go(steps[at - 1], cx)))))
                                    .child(Button::new("onb-next", if last { "Start" } else { "Next" }).primary().on_click(cx.listener(move |o, _, _, cx| if last { o.finish(cx) } else { o.go(steps[at + 1], cx) }))),
                            ),
                    )
                    .child(crate::ui::grain::brackets(14., -9., t.text_3)),
            )
    }
}
