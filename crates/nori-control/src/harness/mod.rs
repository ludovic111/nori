//! The agent harness (lsuite's HARNESS.md): what makes an agent good at design work in nori,
//! whichever agent it is (the Agent panel's, an MCP client's, the lsuite app's).
//!
//! * The **expert brief** ([`brief`]): the trade's quality bar, nori's mental model, the commands
//!   for the common jobs, the usual mistakes and the finish routine. One source (`brief.md`):
//!   the built-in agent's system prompt and `nori-mcp`'s instructions are both made from it.
//! * **Skills** ([`skills`], `skills/*.md`): playbooks for the trade's jobs, loaded on demand with
//!   `harness.skill`; over MCP each is also a prompt and a resource (`nori://skills/<name>`).
//! * **Live context** ([`context`]): a compact summary of the document, refreshed before every
//!   model step, with what the person changed since the last one.
//! * **Eyes** ([`checks`], `harness.look`): the page as a picture with objective measures: text
//!   contrast, things outside the page or bleed, image resolution at print size, overflow.
//!
//! The finish routine (look, compare, fix up to three times, report) is written into the brief
//! and the skills; one undo per agent turn is the agent's checkpoint (`history.checkpoint`).

pub mod checks;
pub mod context;

use std::sync::{Arc, OnceLock};

use serde::Serialize;
use serde_json::{Value, json};

use crate::commands::util;
use crate::registry::{Args, Ctx};
use crate::session::{CmdResult, Session, Source};

/// The brief's source; `{{skills}}` becomes the index of [`skills`].
const BRIEF_SOURCE: &str = include_str!("brief.md");

/// The built-in skills, in the order they are listed.
const SKILL_FILES: &[&str] = &[
    include_str!("skills/retouch-photo.md"),
    include_str!("skills/cutout-composite.md"),
    include_str!("skills/poster.md"),
    include_str!("skills/social-set.md"),
    include_str!("skills/logo.md"),
    include_str!("skills/booklet.md"),
    include_str!("skills/brand-kit.md"),
    include_str!("skills/mockup.md"),
    include_str!("skills/export-print-web.md"),
    include_str!("skills/batch-edits.md"),
    include_str!("skills/write-plugin.md"),
];

/// One playbook: when to use it, the steps with the exact commands, the checks that prove it worked.
#[derive(Clone, Debug, Serialize)]
pub struct Skill {
    pub name: String,
    pub title: String,
    /// When to use it, in one line.
    pub when: String,
    /// The recipe (Markdown, without the front matter).
    pub markdown: String,
}

/// Reads a skill file: `---` front matter with `name`, `title` and `when`, then the Markdown.
fn parse_skill(text: &str) -> Result<Skill, String> {
    let rest = text.strip_prefix("---\n").ok_or("a skill starts with --- front matter")?;
    let (head, body) = rest.split_once("\n---\n").ok_or("the front matter ends with ---")?;
    let field = |key: &str| {
        head.lines()
            .find_map(|l| l.strip_prefix(key).and_then(|v| v.strip_prefix(':')))
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| format!("the front matter has no `{key}`"))
    };
    Ok(Skill { name: field("name")?, title: field("title")?, when: field("when")?, markdown: body.trim().to_string() })
}

/// Every built-in skill.
pub fn skills() -> &'static [Skill] {
    static SKILLS: OnceLock<Vec<Skill>> = OnceLock::new();
    SKILLS.get_or_init(|| SKILL_FILES.iter().map(|t| parse_skill(t).expect("a built-in skill is well formed")).collect())
}

/// One skill by name (`poster`), with a suggestion for a near miss.
pub fn skill(name: &str) -> Result<&'static Skill, String> {
    let key = name.trim().trim_start_matches("nori://skills/").to_ascii_lowercase().replace([' ', '_'], "-");
    if let Some(s) = skills().iter().find(|s| s.name == key) {
        return Ok(s);
    }
    let names: Vec<&str> = skills().iter().map(|s| s.name.as_str()).collect();
    let hint = crate::registry::closest(&key, &names).map(|c| format!(" Did you mean `{c}`?")).unwrap_or_default();
    Err(format!("There is no skill `{}`.{hint} Skills: {}.", name.trim(), names.join(", ")))
}

/// The skills' index, one line each, as the brief lists them.
pub fn skills_index() -> String {
    skills().iter().map(|s| format!("- `{}`: {}. When: {}", s.name, s.title, s.when)).collect::<Vec<_>>().join("\n")
}

/// The expert brief (Markdown), with the skills' index in it.
pub fn brief() -> &'static str {
    static BRIEF: OnceLock<String> = OnceLock::new();
    BRIEF.get_or_init(|| BRIEF_SOURCE.replace("{{skills}}", &skills_index()).trim().to_string())
}

/// Words in `text` (what the brief's length is measured in).
pub fn words(text: &str) -> usize {
    text.split_whitespace().filter(|w| w.chars().any(char::is_alphanumeric)).count()
}

/// `nori-mcp`'s instructions: how this server runs (`mode`), then the brief.
pub fn mcp_instructions(mode: &str) -> String {
    format!(
        "{mode}\nThe whole document: doc_overview (also the resource nori://doc/overview). The live context (nori://harness/context) comes with tool results when the document changed. Skills are also prompts and resources (nori://skills/<name>).\n\n{}",
        brief()
    )
}

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    match cx.spec.name {
        "harness.brief" => {
            let b = brief();
            Ok(json!({ "markdown": b, "words": words(b), "skills": skills().len() }))
        }
        "harness.skills" => Ok(json!({ "skills": skills().iter().map(|k| json!({ "name": k.name, "title": k.title, "when": k.when })).collect::<Vec<_>>() })),
        "harness.skill" => {
            let k = skill(a.str("name")?)?;
            Ok(json!({ "name": k.name, "title": k.title, "when": k.when, "markdown": k.markdown }))
        }
        "harness.context" => {
            let since = a.opt_i64("since").map(|n| n.max(0) as u64);
            // The changes a caller is told about are everyone else's: an agent knows its own.
            let mine: Vec<Source> = match a.opt_str("exclude") {
                Some(list) => list.split(',').filter_map(|x| serde_json::from_value(json!(x.trim())).ok()).collect(),
                None if cx.source.is_agent() => vec![cx.source],
                None => vec![],
            };
            let g = context::glance(s, since, &mine);
            Ok(json!({ "text": g.text(), "lines": g.lines, "short": g.short, "seq": g.seq, "changes": g.changes }))
        }
        "harness.check" => {
            let d = s.document()?;
            let r = util::page_ref(&d, &a)?;
            let pages: Vec<nori_core::PageRef> = if a.bool_or("all", false) { (0..d.pages.len()).map(nori_core::PageRef::Page).collect() } else { vec![r] };
            let reports = tokio::task::spawn_blocking(move || pages.into_iter().map(|p| checks::check(&d, p, checks::Depth::Full)).collect::<Result<Vec<_>, String>>())
                .await
                .map_err(|e| e.to_string())??;
            let errors: usize = reports.iter().map(|r| r.errors).sum();
            let warnings: usize = reports.iter().map(|r| r.warnings).sum();
            Ok(json!({ "ok": errors == 0, "errors": errors, "warnings": warnings, "pages": reports }))
        }
        "harness.look" => look(s, &a).await,
        _ => Err(crate::commands::unhandled(cx)),
    }
}

/// The page as the model's picture, with the checks' numbers.
async fn look(s: &Arc<Session>, a: &Args) -> CmdResult {
    let d = s.document()?;
    let r = util::page_ref(&d, a)?;
    let page = d.page_at(r).ok_or("no page")?.clone();
    let width = a.opt_u32("width").unwrap_or(1024).clamp(64, 2048);
    let region = a.array("region").map(|v| v.iter().filter_map(Value::as_f64).collect::<Vec<f64>>());
    let s2 = s.clone();
    tokio::task::spawn_blocking(move || {
        let area = match region.as_deref() {
            Some([x, y, w, h]) => nori_core::Rect::new(*x as i32, *y as i32, (*w as u32).max(1), (*h as u32).max(1)).intersect(&page.bounds()),
            _ => page.bounds(),
        };
        if area.is_empty() {
            return Err("That region is outside the page.".to_string());
        }
        let prep = nori_render::prepare(&d);
        let rgba = nori_render::composite::flatten_page_with(&d, &page, &prep, area);
        let rgba = crate::commands::page::checker(&rgba, area.w, area.h);
        let (w, h, small) = nori_render::transform::fit_rgba(&rgba, area.w, area.h, width);
        let path = util::save_look(&s2, &format!("look-{}", page.id), w, h, &small)?;
        let report = checks::check(&d, r, checks::Depth::Full)?;
        Ok(json!({
            "path": path,
            "width": w,
            "height": h,
            "page": page.id,
            "region": [area.x, area.y, area.w, area.h],
            "scale": (w as f64 / area.w as f64 * 1000.0).round() / 1000.0,
            "checks": report,
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_brief_is_whole_and_the_right_length() {
        let b = brief();
        assert!(!b.contains("{{"), "the skills index is filled in");
        let n = words(b);
        assert!((800..=1500).contains(&n), "the brief has {n} words; HARNESS.md asks for 800 to 1,500");
        for k in skills() {
            assert!(b.contains(&format!("`{}`", k.name)), "{} is in the brief's index", k.name);
        }
        for part in ["finish routine", "bleed", "contrast", "harness_look", "harness_skill"] {
            assert!(b.contains(part), "the brief mentions {part}");
        }
    }

    #[test]
    fn skills_are_well_formed_and_name_real_commands() {
        let all = skills();
        assert!((8..=15).contains(&all.len()), "{} skills", all.len());
        let tools: Vec<String> = crate::registry::commands().iter().map(|s| s.tool_name()).collect();
        for k in all {
            assert!(!k.title.is_empty() && !k.when.is_empty(), "{}", k.name);
            assert!(k.markdown.contains("## Steps") && k.markdown.contains("## Checks"), "{} has steps and checks", k.name);
            // Every `family_verb` in a recipe is a real command.
            for word in k.markdown.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
                let Some((family, verb)) = word.split_once('_') else { continue };
                let families = ["doc", "page", "layer", "raster", "vector", "text", "select", "filter", "color", "history", "export", "plugin", "harness", "app"];
                if families.contains(&family) && !verb.is_empty() && verb.chars().next().is_some_and(char::is_lowercase) {
                    assert!(tools.iter().any(|t| t == word), "skill {} names `{word}`, which isn't a command", k.name);
                }
            }
        }
        let mut names: Vec<&str> = all.iter().map(|k| k.name.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), all.len(), "skill names are unique");
    }

    #[test]
    fn a_near_miss_names_the_skill() {
        assert_eq!(skill("Poster").unwrap().name, "poster");
        assert_eq!(skill("nori://skills/logo").unwrap().name, "logo");
        let e = skill("postr").unwrap_err();
        assert!(e.contains("`poster`"), "{e}");
    }
}
