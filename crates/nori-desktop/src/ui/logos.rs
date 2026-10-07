//! The logos of the services and apps nori works with (AI providers, the editors people come
//! from, the formats it loads, the other lsuite apps), shown next to their names. The files are
//! the owners' own (`assets/logos/SOURCES.md` says where each came from); they keep their own
//! colours, so they are drawn with `img()`, not tinted like icons.

use gpui::{App, IntoElement, ObjectFit, Pixels, RenderOnce, Styled, Window, div, img, prelude::*};

use crate::theme::ActiveTheme;
use crate::ui::icon;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogoFile {
    pub file: &'static str,
    /// Has a `-dark` twin for the dark theme.
    pub dark: bool,
}

const fn one(file: &'static str) -> Option<LogoFile> {
    Some(LogoFile { file, dark: false })
}

const fn themed(file: &'static str) -> Option<LogoFile> {
    Some(LogoFile { file, dark: true })
}

/// Every id the window can name, with its logo (`None`: the generic icon).
pub const LOGOS: &[(&str, Option<LogoFile>)] = &[
    // Agent providers.
    ("lsuite", one("lsuite")),
    ("claude", one("claude")),
    ("claude-code", one("claude")),
    ("anthropic", one("claude")),
    ("openai", themed("openai")),
    ("codex", themed("openai")),
    ("ollama", themed("ollama")),
    ("openai-compatible", None),
    // Editors people come from (only those whose files nori opens: nori_io::APPS).
    ("photoshop", one("photoshop")),
    ("gimp", one("gimp")),
    ("affinityphoto", one("affinityphoto")),
    ("pixelmator", one("pixelmator")),
    ("krita", one("krita")),
    ("photopea", one("photopea")),
    ("illustrator", one("illustrator")),
    ("inkscape", one("inkscape")),
    ("figma", one("figma")),
    ("affinitydesigner", one("affinitydesigner")),
    ("canva", one("canva")),
    ("indesign", one("indesign")),
    ("affinitypublisher", one("affinitypublisher")),
    ("scribus", one("scribus")),
    // Tools and formats.
    ("rust", themed("rust")),
    // .cube LUTs (DaVinci Resolve makes them; Photoshop and Premiere read them).
    ("resolve", one("resolve")),
    // lsuite.
    ("kimchi", one("kimchi")),
    ("ryolune", one("ryolune")),
    ("zenith", one("zenith")),
];

pub fn logo_file(id: &str) -> Option<LogoFile> {
    let id = id.trim().to_ascii_lowercase();
    LOGOS.iter().find(|(k, _)| *k == id).and_then(|(_, f)| *f)
}

pub fn logo_path(f: LogoFile, dark: bool) -> String {
    if dark && f.dark { format!("logos/{}-dark.png", f.file) } else { format!("logos/{}.png", f.file) }
}

/// A logo, `size` square; ids without one show the generic `box` icon.
pub fn logo(id: &str, size: Pixels) -> Logo {
    Logo { file: logo_file(id), size }
}

#[derive(IntoElement)]
pub struct Logo {
    file: Option<LogoFile>,
    size: Pixels,
}

impl RenderOnce for Logo {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let size = self.size;
        match self.file {
            Some(f) if crate::assets::Assets::get(&logo_path(f, cx.theme().is_dark())).is_some() => {
                div().flex_none().size(size).child(img(logo_path(f, cx.theme().is_dark())).size(size).object_fit(ObjectFit::Contain).rounded(size * 0.22))
            }
            _ => div().flex_none().size(size).flex().items_center().justify_center().child(icon("box").size(size * 0.9)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::Assets;

    #[test]
    fn every_app_nori_names_has_a_logo_entry() {
        for a in nori_io::APPS {
            assert!(LOGOS.iter().any(|(k, _)| *k == a.id), "no logo entry for {}", a.id);
        }
        for p in nori_control::settings::AGENT_PROVIDERS {
            assert!(LOGOS.iter().any(|(k, _)| k == p), "no logo entry for provider {p}");
        }
    }

    #[test]
    fn every_bundled_logo_is_listed_and_sourced() {
        let sources = include_str!("../../assets/logos/SOURCES.md");
        for path in Assets::iter().filter(|p| p.starts_with("logos/") && p.ends_with(".png")) {
            let file = path.trim_start_matches("logos/");
            assert!(sources.contains(&format!("`{file}`")), "{file} isn't in assets/logos/SOURCES.md");
            let data = Assets::get(&path).unwrap();
            let img = image::load_from_memory(&data.data).unwrap_or_else(|e| panic!("{path}: {e}"));
            assert!(img.width() >= 32 && img.height() >= 32, "{path} is too small");
        }
    }
}
