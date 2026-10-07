//! The checks: each job lists them in its JSON (`{"check": "<kind>", …}`); every one is a fact
//! about the saved document, the files written or the agent's report.

use std::collections::BTreeSet;
use std::path::Path;

use nori_core::{Content, Document, Layer, Page};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::fixtures::{self, Baseline};
use crate::{Job, JobResult};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CheckResult {
    pub label: String,
    pub ok: bool,
    pub detail: String,
}

/// A job that couldn't be scored at all.
pub fn failed(job: &Job, why: String) -> JobResult {
    let checks: Vec<CheckResult> = job.checks.iter().map(|c| CheckResult { label: label(c), ok: false, detail: why.clone() }).collect();
    JobResult {
        name: job.name.clone(),
        title: job.title.clone(),
        passed: false,
        checks_passed: 0,
        checks_total: checks.len(),
        checks,
        agent_ok: false,
        agent_error: None,
        reply: String::new(),
        commands: 0,
        changes: 0,
        skills_loaded: vec![],
        looked: false,
        seconds: 0.0,
        input_tokens: 0,
        output_tokens: 0,
    }
}

fn label(c: &Value) -> String {
    if let Some(l) = c["label"].as_str() {
        return l.to_string();
    }
    let kind = c["check"].as_str().unwrap_or("?");
    let detail: Vec<String> = c.as_object().into_iter().flatten().filter(|(k, _)| *k != "check").map(|(k, v)| format!("{k}={}", v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))).collect();
    if detail.is_empty() { kind.to_string() } else { format!("{kind}({})", detail.join(" ")) }
}

/// What the checks look at.
struct Scene<'a> {
    doc: Document,
    checks: Value,
    report: &'a Value,
    baseline: &'a Baseline,
    out: std::path::PathBuf,
}

pub async fn score(job: &Job, dir: &Path, baseline: &Baseline, report: &Value) -> JobResult {
    let commands: Vec<&Value> = report["commands"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
    let skills: Vec<String> = commands.iter().filter(|c| c["command"] == "harness.skill").filter_map(|c| c["skill"].as_str().map(str::to_string)).collect();
    let looked = commands.iter().any(|c| matches!(c["command"].as_str(), Some("harness.look" | "page.look")));
    let mut r = failed(job, String::new());
    r.agent_ok = report["ok"].as_bool().unwrap_or(false);
    r.agent_error = report["error"].as_str().map(str::to_string);
    r.reply = report["reply"].as_str().unwrap_or("").to_string();
    r.commands = commands.len();
    r.changes = report["changes"].as_u64().unwrap_or(0) as usize;
    r.skills_loaded = skills;
    r.looked = looked;
    r.seconds = report["seconds"].as_f64().unwrap_or(0.0);
    r.input_tokens = report["usage"]["inputTokens"].as_u64().unwrap_or(0);
    r.output_tokens = report["usage"]["outputTokens"].as_u64().unwrap_or(0);

    let file = dir.join("job.nori");
    let scene = async {
        if !file.is_file() {
            return Err("no document was saved".to_string());
        }
        let s = fixtures::session(&dir.join("score"))?;
        nori_control::call(&s, nori_control::Source::Cli, "doc.open", json!({ "path": file })).await?;
        let checks = nori_control::call(&s, nori_control::Source::Cli, "harness.check", json!({ "all": true })).await?;
        Ok((s.document()?, checks))
    }
    .await;
    let (doc, checks) = match scene {
        Ok(x) => x,
        Err(e) => {
            for c in &mut r.checks {
                c.detail = e.clone();
            }
            return r;
        }
    };
    let _ = std::fs::write(dir.join("checks.json"), serde_json::to_vec_pretty(&checks).unwrap_or_default());
    let scene = Scene { doc, checks, report, baseline, out: dir.join("out") };
    r.checks = job
        .checks
        .iter()
        .map(|c| {
            let (ok, detail) = match run(&scene, c) {
                Ok(d) => (true, d),
                Err(d) => (false, d),
            };
            CheckResult { label: label(c), ok, detail }
        })
        .collect();
    r.checks_passed = r.checks.iter().filter(|c| c.ok).count();
    r.passed = r.checks_passed == r.checks_total;
    r
}

/// Visible layers of a page (inside visible groups), groups included.
fn shown(p: &Page) -> Vec<&Layer> {
    fn walk<'a>(list: &'a [Layer], out: &mut Vec<&'a Layer>) {
        for l in list {
            if l.visible {
                out.push(l);
                walk(l.children(), out);
            }
        }
    }
    let mut out = vec![];
    walk(&p.layers, &mut out);
    out
}

fn pages<'a>(d: &'a Document, c: &Value) -> Result<Vec<&'a Page>, String> {
    match c["page"].as_u64() {
        Some(n) => d.pages.get(n as usize - 1).map(|p| vec![p]).ok_or_else(|| format!("there is no page {n} ({} pages)", d.pages.len())),
        None => Ok(d.pages.iter().collect()),
    }
}

/// The words a text layer shows, with what flows into it from earlier frames counted once.
fn texts<'a>(layers: &[&'a Layer]) -> Vec<(&'a Layer, &'a nori_core::TextLayer)> {
    layers.iter().filter_map(|l| if let Content::Text { text } = &l.content { Some((*l, text)) } else { None }).collect()
}

fn kind_matches(l: &Layer, kind: &str) -> bool {
    match (kind, &l.content) {
        ("any", Content::Group { .. }) => false,
        ("any", Content::Raster { pixels, .. }) => pixels.used_tiles() > 0,
        ("any", _) => true,
        ("raster", Content::Raster { pixels, .. }) => pixels.used_tiles() > 0,
        (k, c) => c.kind() == k,
    }
}

fn hex(c: &str) -> Option<[f32; 3]> {
    let h = c.trim().trim_start_matches('#');
    if h.len() < 6 {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok().map(f32::from);
    Some([p(0)?, p(2)?, p(4)?])
}

fn problems<'a>(scene: &'a Scene) -> Vec<&'a Value> {
    scene.checks["pages"].as_array().into_iter().flatten().flat_map(|p| p["problems"].as_array().into_iter().flatten()).collect()
}

fn run(scene: &Scene, c: &Value) -> Result<String, String> {
    let d = &scene.doc;
    let min = c["min"].as_f64();
    let max = c["max"].as_f64();
    let at_least = |n: f64, what: &str| -> Result<String, String> {
        match (min, max) {
            (Some(m), _) if n < m => Err(format!("{n} {what}, wanted at least {m}")),
            (_, Some(m)) if n > m => Err(format!("{n} {what}, wanted at most {m}")),
            _ => Ok(format!("{n} {what}")),
        }
    };
    match c["check"].as_str().unwrap_or("") {
        "pages" => at_least(d.pages.len() as f64, "pages"),
        "pageSize" => {
            let (w, h) = (c["width"].as_u64().unwrap_or(0) as i64, c["height"].as_u64().unwrap_or(0) as i64);
            let tol = c["tolerance"].as_i64().unwrap_or(2);
            let fits = |p: &&Page| (p.width as i64 - w).abs() <= tol && (p.height as i64 - h).abs() <= tol;
            let list = pages(d, c)?;
            let sizes: Vec<String> = list.iter().map(|p| format!("{}×{}", p.width, p.height)).collect();
            if list.iter().any(fits) { Ok(format!("a page is {w}×{h}")) } else { Err(format!("no page is {w}×{h} (pages: {})", sizes.join(", "))) }
        }
        "dpi" => {
            if d.dpi as f64 >= min.unwrap_or(0.0) { Ok(format!("{} dpi", d.dpi)) } else { Err(format!("{} dpi", d.dpi)) }
        }
        "text" => {
            let needle = c["contains"].as_str().unwrap_or("").to_lowercase();
            let every = c["everyPage"].as_bool().unwrap_or(false);
            let list = pages(d, c)?;
            let has = |p: &Page| texts(&shown(p)).iter().any(|(_, t)| t.text.to_lowercase().contains(&needle));
            let n = list.iter().filter(|p| has(p)).count();
            if (every && n == list.len()) || (!every && n > 0) { Ok(format!("on {n} page(s)")) } else { Err(format!("\"{needle}\" is on {n} of {} page(s)", list.len())) }
        }
        "textCount" => {
            let list = pages(d, c)?;
            if c["everyPage"].as_bool().unwrap_or(false) {
                let counts: Vec<usize> = list.iter().map(|p| texts(&shown(p)).iter().filter(|(_, t)| !t.text.trim().is_empty()).count()).collect();
                let low = counts.iter().copied().min().unwrap_or(0);
                return at_least(low as f64, "text layers on the page with fewest");
            }
            let n: usize = list.iter().map(|p| texts(&shown(p)).iter().filter(|(_, t)| !t.text.trim().is_empty()).count()).sum();
            at_least(n as f64, "text layers")
        }
        "layers" => {
            let kind = c["kind"].as_str().unwrap_or("any");
            let n: usize = pages(d, c)?.iter().map(|p| shown(p).into_iter().filter(|l| kind_matches(l, kind)).count()).sum();
            at_least(n as f64, &format!("{kind} layers"))
        }
        "largestText" => {
            let biggest = pages(d, c)?.iter().flat_map(|p| texts(&shown(p)).into_iter().map(|(_, t)| t.size)).fold(0.0f32, f32::max);
            let want = c["minPx"].as_f64().unwrap_or(0.0) as f32;
            if biggest >= want { Ok(format!("{biggest} px")) } else { Err(format!("the largest text is {biggest} px, wanted {want}")) }
        }
        "smallestText" => {
            let want = c["minPx"].as_f64().unwrap_or(0.0) as f32;
            let small: Vec<String> = pages(d, c)?.iter().flat_map(|p| texts(&shown(p))).filter(|(_, t)| !t.text.trim().is_empty() && t.size < want).map(|(l, t)| format!("{} {} px", l.id, t.size)).collect();
            if small.is_empty() { Ok(format!("all text ≥ {want} px")) } else { Err(format!("too small: {}", small.join(", "))) }
        }
        "fonts" => {
            let fams: BTreeSet<String> = d.pages.iter().flat_map(|p| texts(&shown(p))).filter(|(_, t)| !t.text.trim().is_empty()).map(|(_, t)| t.font.to_lowercase()).collect();
            at_least(fams.len() as f64, "font families").map(|s| format!("{s}: {}", fams.into_iter().collect::<Vec<_>>().join(", ")))
        }
        "problems" => {
            let kinds: Vec<&str> = c["kinds"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            let found: Vec<String> = problems(scene)
                .into_iter()
                .filter(|p| if kinds.is_empty() { p["severity"] == "error" } else { p["kind"].as_str().is_some_and(|k| kinds.contains(&k)) })
                .map(|p| p["message"].as_str().unwrap_or("").to_string())
                .collect();
            if found.is_empty() { Ok("none".into()) } else { Err(found.join(" ")) }
        }
        "contrast" => {
            let all: Vec<&Value> = scene.checks["pages"].as_array().into_iter().flatten().flat_map(|p| p["contrast"].as_array().into_iter().flatten()).collect();
            if all.is_empty() {
                return Err("no text to measure".into());
            }
            let bad: Vec<String> = all.iter().filter(|x| x["ok"] != true).map(|x| format!("{} {}:1 (needs {})", x["layerId"].as_str().unwrap_or(""), x["ratio"], x["required"])).collect();
            let low = all.iter().filter_map(|x| x["ratio"].as_f64()).fold(f64::MAX, f64::min);
            if bad.is_empty() { Ok(format!("{} text layers, lowest {low:.1}:1", all.len())) } else { Err(bad.join(", ")) }
        }
        "export" => {
            let path = scene.out.join(c["path"].as_str().unwrap_or(""));
            let bytes = std::fs::metadata(&path).map(|m| m.len()).map_err(|_| format!("{} wasn't written", path.display()))?;
            if bytes < c["minBytes"].as_u64().unwrap_or(100) {
                return Err(format!("{} is only {bytes} bytes", path.display()));
            }
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            let head = std::fs::read(&path).map(|b| b.into_iter().take(512).collect::<Vec<u8>>()).unwrap_or_default();
            match ext.as_str() {
                "pdf" if !head.starts_with(b"%PDF") => return Err("not a PDF".into()),
                "svg" if !String::from_utf8_lossy(&head).contains("<svg") => return Err("not an SVG".into()),
                "png" | "jpg" | "jpeg" | "webp" => {
                    let (w, h) = image::image_dimensions(&path).map_err(|e| format!("not a picture: {e}"))?;
                    let tol = c["tolerance"].as_u64().unwrap_or(2) as i64;
                    for (want, got, what) in [(c["width"].as_u64(), w, "wide"), (c["height"].as_u64(), h, "high")] {
                        if let Some(want) = want
                            && (want as i64 - got as i64).abs() > tol
                        {
                            return Err(format!("{w}×{h}: wanted {want} px {what}"));
                        }
                    }
                    return Ok(format!("{w}×{h}, {bytes} bytes"));
                }
                _ => {}
            }
            Ok(format!("{bytes} bytes"))
        }
        "adjustments" => at_least(d.pages.iter().flat_map(|p| shown(p)).filter(|l| matches!(l.content, Content::Adjustment { .. })).count() as f64, "adjustment layers"),
        "masked" => {
            let masked = |l: &&Layer| l.mask.as_ref().is_some_and(|m| m.enabled);
            if let Some(name) = c["name"].as_str() {
                let l = d.pages.iter().flat_map(|p| p.all()).find(|l| l.name.eq_ignore_ascii_case(name)).ok_or(format!("no layer named {name}"))?;
                return if masked(&l) { Ok(format!("{name} has a mask")) } else { Err(format!("{name} has no mask")) };
            }
            at_least(d.pages.iter().flat_map(|p| shown(p)).filter(masked).count() as f64, "masked layers")
        }
        "clippedOrMasked" => at_least(d.pages.iter().flat_map(|p| shown(p)).filter(|l| l.clipped || l.mask.as_ref().is_some_and(|m| m.enabled)).count() as f64, "clipped or masked layers"),
        "vectorOnly" => {
            let pixels: Vec<String> = d.pages.iter().flat_map(|p| shown(p)).filter(|l| kind_matches(l, "raster")).map(|l| format!("{} ({})", l.id, l.name)).collect();
            if pixels.is_empty() { Ok("no pixel layers".into()) } else { Err(format!("pixel layers: {}", pixels.join(", "))) }
        }
        "master" => at_least(d.pages.iter().filter(|p| p.master.is_some()).count() as f64, "pages with a master"),
        "threads" => at_least(d.pages.iter().flat_map(|p| p.all()).filter(|l| matches!(&l.content, Content::Text { text } if text.next.is_some())).count() as f64, "threaded frames"),
        "styles" => at_least(d.styles.paragraph.len() as f64, "paragraph styles"),
        "bleed" => {
            let low = pages(d, c)?.iter().map(|p| p.bleed).fold(f32::MAX, f32::min);
            if low as f64 >= min.unwrap_or(1.0) { Ok(format!("bleed {low} px")) } else { Err(format!("bleed {low} px")) }
        }
        "brighter" => {
            let before = scene.baseline.luminance.ok_or("no baseline")?;
            let p = d.pages.first().ok_or("no page")?;
            let after = fixtures::mean_luminance(d, p);
            let by = c["by"].as_f64().unwrap_or(5.0);
            if after - before >= by { Ok(format!("{before:.0} → {after:.0}")) } else { Err(format!("{before:.0} → {after:.0}, wanted +{by}")) }
        }
        "unchangedPixels" => {
            if scene.baseline.rasters.is_empty() {
                return Err("no baseline pixels".into());
            }
            let mut kept = 0;
            for (id, h) in &scene.baseline.rasters {
                match d.pages.iter().flat_map(|p| p.all()).find(|l| &l.id == id) {
                    Some(l) => match &l.content {
                        Content::Raster { pixels, .. } if fixtures::hash(&pixels.to_vec()) == *h => kept += 1,
                        _ => return Err(format!("{id}'s pixels changed")),
                    },
                    None => return Err(format!("{id} was deleted")),
                }
            }
            Ok(format!("{kept} original layer(s) intact"))
        }
        "textStyle" => {
            let prefix = c["namePrefix"].as_str().unwrap_or("").to_lowercase();
            let list: Vec<(&Layer, &nori_core::TextLayer)> = d.pages.iter().flat_map(|p| texts(&p.all())).filter(|(l, _)| l.name.to_lowercase().starts_with(&prefix)).collect();
            if list.len() < min.unwrap_or(1.0) as usize {
                return Err(format!("{} layers named {prefix}…", list.len()));
            }
            let mut off = vec![];
            for (l, t) in &list {
                let s = fixtures::text_summary(&l.name, t);
                for key in ["font", "weight", "size", "color", "style"] {
                    let want = &c[key];
                    if want.is_null() {
                        continue;
                    }
                    let got = &s[key];
                    let same = match (want, got) {
                        (Value::String(a), Value::String(b)) => a.eq_ignore_ascii_case(b),
                        (Value::Number(a), Value::Number(b)) => (a.as_f64().unwrap_or(0.0) - b.as_f64().unwrap_or(0.0)).abs() < 0.5,
                        _ => false,
                    };
                    if !same {
                        off.push(format!("{} {key} {got}", l.id));
                    }
                }
            }
            if off.is_empty() { Ok(format!("{} layers match", list.len())) } else { Err(off.join(", ")) }
        }
        "unchangedText" => {
            let prefix = c["namePrefix"].as_str().unwrap_or("").to_lowercase();
            let mut n = 0;
            for (id, before) in &scene.baseline.texts {
                if !before["name"].as_str().unwrap_or("").to_lowercase().starts_with(&prefix) {
                    continue;
                }
                let l = d.layer(id).ok_or(format!("{id} was deleted"))?;
                let Content::Text { text } = &l.content else { return Err(format!("{id} isn't text any more")) };
                let now = fixtures::text_summary(&l.name, text);
                for key in ["text", "font", "size", "weight", "color"] {
                    if now[key] != before[key] {
                        return Err(format!("{id} {key} changed: {} → {}", before[key], now[key]));
                    }
                }
                n += 1;
            }
            if n == 0 { Err("no such layers in the fixture".into()) } else { Ok(format!("{n} layers unchanged")) }
        }
        "pixel" => {
            let p = pages(d, c)?.into_iter().next().ok_or("no page")?;
            let (x, y) = (c["x"].as_i64().unwrap_or(0) as i32, c["y"].as_i64().unwrap_or(0) as i32);
            let prep = nori_render::prepare(d);
            let px = nori_render::composite::flatten_page_with(d, p, &prep, nori_core::Rect::new(x, y, 1, 1));
            let a = px.get(3).copied().unwrap_or(0) as f32 / 255.0;
            let got = [0, 1, 2].map(|i| px.get(i).copied().unwrap_or(255) as f32 * a + 255.0 * (1.0 - a));
            let dist = |want: [f32; 3]| ((got[0] - want[0]).powi(2) + (got[1] - want[1]).powi(2) + (got[2] - want[2]).powi(2)).sqrt();
            let tol = c["tolerance"].as_f64().unwrap_or(40.0) as f32;
            let shown = format!("#{:02x}{:02x}{:02x}", got[0] as u8, got[1] as u8, got[2] as u8);
            if let Some(w) = c["near"].as_str().and_then(hex)
                && dist(w) > tol
            {
                return Err(format!("{x},{y} is {shown}, wanted near {}", c["near"]));
            }
            if let Some(w) = c["not"].as_str().and_then(hex)
                && dist(w) <= tol
            {
                return Err(format!("{x},{y} is still {shown}"));
            }
            Ok(format!("{x},{y} is {shown}"))
        }
        "hexCodes" => {
            let mut codes = BTreeSet::new();
            for (_, t) in d.pages.iter().flat_map(|p| texts(&shown(p))) {
                let b = t.text.as_bytes();
                for i in 0..b.len() {
                    if b[i] == b'#' && i + 7 <= b.len() && b[i + 1..i + 7].iter().all(u8::is_ascii_hexdigit) {
                        codes.insert(t.text[i..i + 7].to_ascii_lowercase());
                    }
                }
            }
            at_least(codes.len() as f64, "hex codes")
        }
        "looked" => {
            let n = scene.report["commands"].as_array().into_iter().flatten().filter(|c| matches!(c["command"].as_str(), Some("harness.look" | "page.look"))).count();
            if n > 0 { Ok(format!("looked {n} times")) } else { Err("never looked at the result".into()) }
        }
        "changes" => at_least(scene.report["changes"].as_f64().unwrap_or(0.0), "changes"),
        other => Err(format!("unknown check `{other}`")),
    }
}
