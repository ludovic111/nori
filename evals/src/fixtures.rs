//! Fixtures: the pictures and the starting document of a job, and what they were before the
//! agent touched them (the baseline some checks compare with).

use std::collections::BTreeMap;
use std::path::Path;

use nori_control::{Session, SessionOptions, Source};
use nori_core::{Content, Document};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::Job;

/// A picture a fixture makes: `make` is one of `dull-photo`, `product`, `beach`, `app-screen`,
/// `pastel`.
#[derive(Clone, Debug, Deserialize)]
pub struct FileSpec {
    pub path: String,
    pub make: String,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

/// The starting document as it was.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Baseline {
    pub had_document: bool,
    /// Mean luminance (0–255) of page 1 as drawn.
    pub luminance: Option<f64>,
    /// Raster layers' pixels by id: a hash.
    pub rasters: BTreeMap<String, u64>,
    /// Text layers by id: name, text, font, size, weight, colour, style.
    pub texts: BTreeMap<String, Value>,
}

pub fn load_baseline(dir: &Path) -> Baseline {
    std::fs::read(dir.join("baseline.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// A session of the runner's own (fixtures and scoring), in `dir`.
pub fn session(dir: &Path) -> Result<std::sync::Arc<Session>, String> {
    Session::new(SessionOptions { data_dir: Some(dir.join("data")), config_dir: Some(dir.join("config")), secrets: None, headless: true }).map_err(|e| e.to_string())
}

/// Makes the job's pictures and starting document (`job.nori` in `dir`), and its baseline.
pub async fn prepare(job: &Job, dir: &Path) -> Result<Baseline, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(dir.join("job.nori"));
    for f in &job.files {
        let (w, h) = (f.width.unwrap_or(1600), f.height.unwrap_or(1067));
        let rgba = make(&f.make, w, h)?;
        let path = dir.join(&f.path);
        image::save_buffer(&path, &rgba, w, h, image::ExtendedColorType::Rgba8).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let mut baseline = Baseline::default();
    if !job.setup.is_empty() {
        let s = session(&dir.join("fixture"))?;
        for (i, c) in job.setup.iter().enumerate() {
            let name = c["command"].as_str().ok_or(format!("setup[{i}] has no command"))?;
            let text = serde_json::to_string(&c["params"]).unwrap_or_default().replace("{dir}", &dir.display().to_string().replace('\\', "/"));
            let params: Value = serde_json::from_str(&text).unwrap_or(json!({}));
            nori_control::call(&s, Source::Cli, name, params).await.map_err(|e| format!("setup[{i}] {name}: {e}"))?;
        }
        s.save(Some(dir.join("job.nori")))?;
        let d = s.document()?;
        baseline = of(&d);
    }
    let _ = std::fs::write(dir.join("baseline.json"), serde_json::to_vec_pretty(&baseline).unwrap_or_default());
    Ok(baseline)
}

pub fn of(d: &Document) -> Baseline {
    let mut b = Baseline { had_document: true, luminance: d.pages.first().map(|p| mean_luminance(d, p)), ..Default::default() };
    for p in &d.pages {
        for l in p.all() {
            match &l.content {
                Content::Raster { pixels, .. } => {
                    b.rasters.insert(l.id.clone(), hash(&pixels.to_vec()));
                }
                Content::Text { text } => {
                    b.texts.insert(l.id.clone(), text_summary(&l.name, text));
                }
                _ => {}
            }
        }
    }
    b
}

pub fn text_summary(name: &str, t: &nori_core::TextLayer) -> Value {
    json!({ "name": name, "text": t.text, "font": t.font, "size": t.size, "weight": t.weight, "color": t.color.hex(), "style": t.style })
}

pub fn hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

/// The mean luminance (0–255) of a page as drawn, over white.
pub fn mean_luminance(d: &Document, p: &nori_core::Page) -> f64 {
    let rgba = nori_render::composite::flatten_page(d, p);
    let n = (rgba.len() / 4).max(1) as f64;
    rgba.as_chunks::<4>().0.iter()
        .map(|c| {
            let a = c[3] as f64 / 255.0;
            let v = |x: u8| x as f64 * a + 255.0 * (1.0 - a);
            0.2126 * v(c[0]) + 0.7152 * v(c[1]) + 0.0722 * v(c[2])
        })
        .sum::<f64>()
        / n
}

/// A small deterministic noise source.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }
}

fn put(out: &mut [u8], w: u32, x: u32, y: u32, c: [f32; 3]) {
    let i = ((y * w + x) * 4) as usize;
    out[i] = c[0].clamp(0.0, 255.0) as u8;
    out[i + 1] = c[1].clamp(0.0, 255.0) as u8;
    out[i + 2] = c[2].clamp(0.0, 255.0) as u8;
    out[i + 3] = 255;
}

/// The fixtures' pictures, drawn here so the evals need no image files.
pub fn make(kind: &str, w: u32, h: u32) -> Result<Vec<u8>, String> {
    let mut out = vec![0u8; (w * h * 4) as usize];
    let mut n = Noise(7);
    let (fw, fh) = (w as f32, h as f32);
    match kind {
        // A landscape (sky, hills, a lake, a sun) that came out dark, flat and blue.
        "dull-photo" | "beach" => {
            let dull = kind == "dull-photo";
            for y in 0..h {
                for x in 0..w {
                    let (u, v) = (x as f32 / fw, y as f32 / fh);
                    let horizon = 0.55 + 0.06 * (u * 9.0).sin() + 0.03 * (u * 23.0).cos();
                    let mut c = if v < horizon {
                        let t = v / horizon;
                        [120.0 + 110.0 * t, 170.0 + 60.0 * t, 235.0 - 20.0 * t]
                    } else if kind == "beach" && v < horizon + 0.12 {
                        [40.0, 120.0 + 40.0 * (u * 40.0).sin().abs(), 170.0]
                    } else if kind == "beach" {
                        [225.0, 200.0, 150.0]
                    } else {
                        let t = (v - horizon) / (1.0 - horizon);
                        [70.0 + 40.0 * t, 120.0 - 30.0 * t, 60.0]
                    };
                    let (dx, dy) = (u - 0.72, v - 0.25);
                    if (dx * dx * (fw / fh).powi(2) + dy * dy).sqrt() < 0.06 {
                        c = [255.0, 236.0, 180.0];
                    }
                    let grain = (n.next() - 0.5) * 28.0;
                    let mut c = c.map(|v| v + grain);
                    if dull {
                        // Dark, flat and cold: what the retouch has to undo.
                        c = [c[0] * 0.45 + 30.0, c[1] * 0.5 + 30.0, c[2] * 0.55 + 55.0];
                    }
                    put(&mut out, w, x, y, c);
                }
            }
        }
        // A red ball on a plain white background (a product shot).
        "product" => {
            let (cx, cy, r) = (fw * 0.5, fh * 0.52, fw.min(fh) * 0.3);
            for y in 0..h {
                for x in 0..w {
                    let (dx, dy) = (x as f32 - cx, y as f32 - cy);
                    let d = (dx * dx + dy * dy).sqrt();
                    let c = if d < r {
                        let shade = 1.0 - 0.45 * ((dx + dy) / r + 1.0) / 2.0;
                        let spec = (-((dx + r * 0.35).powi(2) + (dy + r * 0.35).powi(2)) / (r * r * 0.02)).exp() * 120.0;
                        [220.0 * shade + spec, 40.0 * shade + spec, 45.0 * shade + spec]
                    } else {
                        [255.0, 255.0, 255.0]
                    };
                    put(&mut out, w, x, y, c);
                }
            }
        }
        // An app screen: a coloured header, cards and a button.
        "app-screen" => {
            for y in 0..h {
                for x in 0..w {
                    let (u, v) = (x as f32 / fw, y as f32 / fh);
                    let mut c = [246.0, 244.0, 240.0];
                    if v < 0.28 {
                        c = [40.0 + 60.0 * u, 70.0, 160.0 + 60.0 * v];
                    }
                    for k in 0..3 {
                        let top = 0.33 + k as f32 * 0.17;
                        if v > top && v < top + 0.14 && u > 0.07 && u < 0.93 {
                            c = [255.0, 255.0, 255.0];
                            if u < 0.3 && v > top + 0.02 && v < top + 0.12 {
                                c = [250.0, 180.0 - 50.0 * k as f32, 90.0 + 60.0 * k as f32];
                            }
                        }
                    }
                    if v > 0.86 && v < 0.93 && u > 0.2 && u < 0.8 {
                        c = [255.0, 95.0, 109.0];
                    }
                    put(&mut out, w, x, y, c);
                }
            }
        }
        // A pale pastel gradient (low contrast for white text on it).
        "pastel" => {
            for y in 0..h {
                for x in 0..w {
                    let (u, v) = (x as f32 / fw, y as f32 / fh);
                    let c = [250.0 - 20.0 * v, 225.0 + 20.0 * u, 215.0 + 25.0 * (1.0 - u)];
                    put(&mut out, w, x, y, c);
                }
            }
        }
        other => return Err(format!("no fixture picture `{other}`")),
    }
    Ok(out)
}
