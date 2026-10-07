//! `text.*`: point text and text frames, threads across pages, runs, paragraph and character
//! styles, fonts (InDesign's side of nori).

use std::sync::Arc;

use nori_core::{CharacterStyle, Color, Content, Document, ParagraphStyle, TextAlign, TextLayer, TextRun};
use serde_json::{Value, json};

use super::util;
use crate::registry::{Args, Ctx};
use crate::session::{CmdResult, Session};

/// Applies the style parameters given to a text layer.
fn set_style(t: &mut TextLayer, a: &Args) -> CmdResult<()> {
    if let Some(v) = a.opt_str("font") {
        t.font = nori_render::text::resolve_family(v);
    }
    if let Some(v) = a.opt_f64("size") {
        t.size = (v as f32).clamp(1.0, 4000.0);
    }
    if let Some(v) = a.opt_i64("weight") {
        t.weight = v.clamp(100, 900) as u16;
    }
    if let Some(v) = a.opt_bool("italic") {
        t.italic = v;
    }
    if let Some(v) = a.opt_str("color") {
        t.color = Color::parse(v)?;
    }
    if let Some(v) = a.opt_str("align") {
        t.align = serde_json::from_value(json!(v.to_ascii_lowercase())).map_err(|_| "align is left, center or right")?;
    }
    if let Some(v) = a.opt_f64("lineHeight") {
        t.line_height = (v as f32).clamp(0.5, 5.0);
    }
    if let Some(v) = a.opt_f64("letterSpacing") {
        t.letter_spacing = v as f32;
    }
    if let Some(v) = a.opt_f64("paragraphSpacing") {
        t.paragraph_spacing = (v as f32).max(0.0);
    }
    Ok(())
}

fn apply_paragraph(t: &mut TextLayer, p: &ParagraphStyle) {
    t.font = p.font.clone();
    t.size = p.size;
    t.weight = p.weight;
    t.italic = p.italic;
    t.color = p.color;
    t.align = p.align;
    t.line_height = p.line_height;
    t.letter_spacing = p.letter_spacing;
    t.paragraph_spacing = p.paragraph_spacing;
    t.style = Some(p.name.clone());
}

fn text_mut<'a>(d: &'a mut Document, id: &str) -> CmdResult<&'a mut TextLayer> {
    let l = d.layer_mut(id).ok_or("no layer")?;
    util::unlocked(l)?;
    let name = l.name.clone();
    match &mut l.content {
        Content::Text { text } => Ok(text),
        _ => Err(format!("`{name}` isn't a text layer.")),
    }
}

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    let tools = s.settings().tools;
    let fg = s.colors.read().foreground;
    match cx.spec.name {
        "text.add" => s.edit(cx.label(), cx.source, None, |d| {
            let mut t = TextLayer { text: words(a.str("text")?), x: a.f64("x")? as f32, y: a.f64("y")? as f32, font: tools.font.clone(), size: tools.font_size as f32, color: fg, ..Default::default() };
            if let (Some(w), Some(h)) = (a.opt_f64("frameWidth"), a.opt_f64("frameHeight")) {
                t.frame = Some([(w as f32).max(1.0), (h as f32).max(1.0)]);
            } else if let Some(w) = a.opt_f64("frameWidth") {
                t.frame = Some([(w as f32).max(1.0), (d.height() as f32 - t.y).max(t.size * 2.0)]);
            }
            if let Some(name) = a.opt_str("style") {
                let p = d.paragraph_style(name).cloned().ok_or_else(|| format!("No paragraph style `{name}` (text.styles)."))?;
                apply_paragraph(&mut t, &p);
            }
            set_style(&mut t, &a)?;
            let base: String = t.text.lines().next().unwrap_or("Text").chars().take(24).collect();
            let base = if base.trim().is_empty() { "Text".to_string() } else { base };
            let r = super::layer::add(d, &a, Content::Text { text: t }, &base)?;
            // A layer named after its words needs no number.
            if a.opt_str("name").is_none()
                && let Some(id) = r["layerId"].as_str()
                && !d.all().iter().any(|l| l.name == base && l.id != id)
                && let Some(l) = d.layer_mut(id)
            {
                l.name = base.clone();
            }
            Ok(r)
        }),
        "text.update" => s.edit(cx.label(), cx.source, a.coalesce(), |d| {
            let id = util::layer_id(d, &a)?;
            let t = text_mut(d, &id)?;
            if let Some(v) = a.opt_str("text") {
                let v = words(v);
                if v.len() != t.text.len() {
                    t.runs.retain(|r| r.end <= v.len());
                }
                t.text = v;
            }
            set_style(t, &a)?;
            if let Some(v) = a.opt_f64("x") {
                t.x = v as f32;
            }
            if let Some(v) = a.opt_f64("y") {
                t.y = v as f32;
            }
            match (a.opt_f64("frameWidth"), a.opt_f64("frameHeight")) {
                (Some(w), _) if w <= 0.0 => t.frame = None,
                (Some(w), Some(h)) => t.frame = Some([w as f32, h as f32]),
                (Some(w), None) => t.frame = Some([w as f32, t.frame.map_or(t.size * 4.0, |f| f[1])]),
                (None, Some(h)) => t.frame = Some([t.frame.map_or(400.0, |f| f[0]), h as f32]),
                _ => {}
            }
            Ok(json!({ "layerId": id, "text": t }))
        }),
        "text.setRuns" => s.edit(cx.label(), cx.source, None, |d| {
            let id = util::layer_id(d, &a)?;
            let runs: Vec<TextRun> = serde_json::from_value(Value::Array(a.array("runs").cloned().unwrap_or_default())).map_err(|e| format!("runs: {e}"))?;
            let styles: Vec<String> = d.styles.character.iter().map(|c| c.name.clone()).collect();
            let t = text_mut(d, &id)?;
            for r in &runs {
                if r.start >= r.end || r.end > t.text.len() || !t.text.is_char_boundary(r.start) || !t.text.is_char_boundary(r.end) {
                    return Err(format!("A run's start and end are byte offsets in the text (0–{}), on character boundaries; {}–{} isn't.", t.text.len(), r.start, r.end));
                }
                if let Some(st) = &r.style
                    && !styles.iter().any(|s| s.eq_ignore_ascii_case(st))
                {
                    return Err(format!("No character style `{st}` (text.defineStyle kind=character)."));
                }
            }
            t.runs = runs;
            Ok(json!({ "layerId": id, "runs": t.runs.len() }))
        }),
        "text.thread" => s.edit(cx.label(), cx.source, None, |d| {
            let from = d.resolve(a.str("from")?)?;
            let to = d.resolve(a.str("to")?)?;
            let thread = d.thread_of(&from);
            let (fi, ti) = (thread.iter().position(|x| *x == from), thread.iter().position(|x| *x == to));
            if from == to || matches!((fi, ti), (Some(f), Some(t)) if t < f) {
                return Err("That would make the thread loop.".into());
            }
            if d.all().iter().any(|l| l.id != from && matches!(&l.content, Content::Text { text } if text.next.as_deref() == Some(to.as_str()))) {
                return Err(format!("`{to}` already continues another frame: unthread that one first (text.unthread)."));
            }
            for id in [&from, &to] {
                let t = text_mut(d, id)?;
                if t.frame.is_none() {
                    return Err(format!("`{id}` is point text: only text frames thread (give it frameWidth and frameHeight)."));
                }
            }
            // What `to` led to stays after it; its own words go (the thread's words fill it).
            text_mut(d, &to)?.text.clear();
            text_mut(d, &from)?.next = Some(to.clone());
            Ok(json!({ "thread": d.thread_of(&from) }))
        }),
        "text.unthread" => s.edit(cx.label(), cx.source, None, |d| {
            let id = util::layer_id(d, &a)?;
            text_mut(d, &id)?.next = None;
            Ok(json!({ "layerId": id }))
        }),
        "text.styles" => {
            let d = s.document()?;
            Ok(json!(d.styles))
        }
        "text.defineStyle" => s.edit(cx.label(), cx.source, None, |d| {
            let name = a.str("name")?.trim().to_string();
            match a.str("kind")? {
                "paragraph" => {
                    let mut p = d.paragraph_style(&name).cloned().unwrap_or(ParagraphStyle { name: name.clone(), ..Default::default() });
                    let mut t = TextLayer::default();
                    apply_paragraph(&mut t, &p);
                    set_style(&mut t, &a)?;
                    p = ParagraphStyle { name: name.clone(), font: t.font, size: t.size, weight: t.weight, italic: t.italic, color: t.color, align: t.align, line_height: t.line_height, letter_spacing: t.letter_spacing, paragraph_spacing: t.paragraph_spacing };
                    d.styles.paragraph.retain(|s| !s.name.eq_ignore_ascii_case(&name));
                    d.styles.paragraph.push(p.clone());
                    // Every layer set with it follows.
                    let ids: Vec<String> = d.all().iter().filter(|l| matches!(&l.content, Content::Text { text } if text.style.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(&name)))).map(|l| l.id.clone()).collect();
                    for id in &ids {
                        if let Some(nori_core::Layer { content: Content::Text { text }, .. }) = d.layer_mut(id) {
                            apply_paragraph(text, &p);
                        }
                    }
                    Ok(json!({ "style": p, "restyled": ids }))
                }
                "character" => {
                    let mut c = d.character_style(&name).cloned().unwrap_or(CharacterStyle { name: name.clone(), ..Default::default() });
                    if let Some(v) = a.opt_str("font") {
                        c.font = Some(nori_render::text::resolve_family(v));
                    }
                    if let Some(v) = a.opt_f64("size") {
                        c.size = Some(v as f32);
                    }
                    if let Some(v) = a.opt_i64("weight") {
                        c.weight = Some(v.clamp(100, 900) as u16);
                    }
                    if let Some(v) = a.opt_bool("italic") {
                        c.italic = Some(v);
                    }
                    if let Some(v) = a.opt_str("color") {
                        c.color = Some(Color::parse(v)?);
                    }
                    c.name = name.clone();
                    d.styles.character.retain(|s| !s.name.eq_ignore_ascii_case(&name));
                    d.styles.character.push(c.clone());
                    Ok(json!({ "style": c }))
                }
                other => Err(format!("`{other}`: kind is paragraph or character.")),
            }
        }),
        "text.applyStyle" => s.edit(cx.label(), cx.source, None, |d| {
            let name = a.str("style")?;
            let p = d.paragraph_style(name).cloned().ok_or_else(|| format!("No paragraph style `{name}` (text.styles)."))?;
            let ids = util::layer_ids(d, &a, "layerIds")?;
            for id in &ids {
                apply_paragraph(text_mut(d, id)?, &p);
            }
            Ok(json!({ "styled": ids }))
        }),
        "text.deleteStyle" => s.edit(cx.label(), cx.source, None, |d| {
            let name = a.str("name")?.to_string();
            match a.str("kind")? {
                "paragraph" => {
                    d.styles.paragraph.retain(|s| !s.name.eq_ignore_ascii_case(&name));
                    let ids: Vec<String> = d.all().iter().map(|l| l.id.clone()).collect();
                    for id in ids {
                        if let Some(nori_core::Layer { content: Content::Text { text }, .. }) = d.layer_mut(&id)
                            && text.style.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(&name))
                        {
                            text.style = None;
                        }
                    }
                }
                "character" => d.styles.character.retain(|s| !s.name.eq_ignore_ascii_case(&name)),
                other => return Err(format!("`{other}`: kind is paragraph or character.")),
            }
            Ok(json!({ "deleted": name }))
        }),
        "text.fonts" => {
            let q = a.opt_str("search").unwrap_or("").to_lowercase();
            let fams: Vec<String> = tokio::task::spawn_blocking(nori_render::text::families).await.map_err(|e| e.to_string())?.into_iter().filter(|f| q.is_empty() || f.to_lowercase().contains(&q)).take(400).collect();
            Ok(json!({ "families": fams }))
        }
        _ => Err(super::unhandled(cx)),
    }
}

#[allow(dead_code)]
fn _align(_: TextAlign) {}

/// The words as given, with a typed `\n` (backslash, n: what shells and some agents send for a
/// line break) made a real one.
fn words(s: &str) -> String {
    s.replace("\\n", "\n")
}
