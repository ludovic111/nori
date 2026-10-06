//! Layers: pixels, solid fills, adjustments, text and groups, each with opacity, a blend mode
//! and an optional mask.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::raster::{Mask, Raster, Rect};

/// How a layer mixes with what is under it (the same modes, and the same maths, as other
/// editors: the W3C compositing spec).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum BlendMode {
    #[default]
    Normal,
    Darken,
    Multiply,
    ColorBurn,
    LinearBurn,
    Lighten,
    Screen,
    ColorDodge,
    LinearDodge,
    Overlay,
    SoftLight,
    HardLight,
    VividLight,
    LinearLight,
    PinLight,
    Difference,
    Exclusion,
    Subtract,
    Divide,
    Hue,
    Saturation,
    Color,
    Luminosity,
    /// Groups only: the children blend straight into what is under the group.
    PassThrough,
}

impl BlendMode {
    /// Every mode, in the order menus list them (grouped like other editors).
    pub const ALL: [BlendMode; 24] = [
        BlendMode::Normal,
        BlendMode::Darken,
        BlendMode::Multiply,
        BlendMode::ColorBurn,
        BlendMode::LinearBurn,
        BlendMode::Lighten,
        BlendMode::Screen,
        BlendMode::ColorDodge,
        BlendMode::LinearDodge,
        BlendMode::Overlay,
        BlendMode::SoftLight,
        BlendMode::HardLight,
        BlendMode::VividLight,
        BlendMode::LinearLight,
        BlendMode::PinLight,
        BlendMode::Difference,
        BlendMode::Exclusion,
        BlendMode::Subtract,
        BlendMode::Divide,
        BlendMode::Hue,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
        BlendMode::PassThrough,
    ];

    pub fn id(self) -> &'static str {
        match self {
            BlendMode::Normal => "normal",
            BlendMode::Darken => "darken",
            BlendMode::Multiply => "multiply",
            BlendMode::ColorBurn => "color-burn",
            BlendMode::LinearBurn => "linear-burn",
            BlendMode::Lighten => "lighten",
            BlendMode::Screen => "screen",
            BlendMode::ColorDodge => "color-dodge",
            BlendMode::LinearDodge => "linear-dodge",
            BlendMode::Overlay => "overlay",
            BlendMode::SoftLight => "soft-light",
            BlendMode::HardLight => "hard-light",
            BlendMode::VividLight => "vivid-light",
            BlendMode::LinearLight => "linear-light",
            BlendMode::PinLight => "pin-light",
            BlendMode::Difference => "difference",
            BlendMode::Exclusion => "exclusion",
            BlendMode::Subtract => "subtract",
            BlendMode::Divide => "divide",
            BlendMode::Hue => "hue",
            BlendMode::Saturation => "saturation",
            BlendMode::Color => "color",
            BlendMode::Luminosity => "luminosity",
            BlendMode::PassThrough => "pass-through",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BlendMode::Normal => "Normal",
            BlendMode::Darken => "Darken",
            BlendMode::Multiply => "Multiply",
            BlendMode::ColorBurn => "Color Burn",
            BlendMode::LinearBurn => "Linear Burn",
            BlendMode::Lighten => "Lighten",
            BlendMode::Screen => "Screen",
            BlendMode::ColorDodge => "Color Dodge",
            BlendMode::LinearDodge => "Linear Dodge (Add)",
            BlendMode::Overlay => "Overlay",
            BlendMode::SoftLight => "Soft Light",
            BlendMode::HardLight => "Hard Light",
            BlendMode::VividLight => "Vivid Light",
            BlendMode::LinearLight => "Linear Light",
            BlendMode::PinLight => "Pin Light",
            BlendMode::Difference => "Difference",
            BlendMode::Exclusion => "Exclusion",
            BlendMode::Subtract => "Subtract",
            BlendMode::Divide => "Divide",
            BlendMode::Hue => "Hue",
            BlendMode::Saturation => "Saturation",
            BlendMode::Color => "Color",
            BlendMode::Luminosity => "Luminosity",
            BlendMode::PassThrough => "Pass Through",
        }
    }

    /// From an id (`multiply`), a label (`Color Burn`) or a near spelling (`colorBurn`, `add`).
    pub fn parse(s: &str) -> Result<Self, String> {
        let key: String = s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
        let alias = match key.as_str() {
            "add" => Some(BlendMode::LinearDodge),
            "colour" => Some(BlendMode::Color),
            "colourburn" => Some(BlendMode::ColorBurn),
            "colourdodge" => Some(BlendMode::ColorDodge),
            "passthru" => Some(BlendMode::PassThrough),
            _ => None,
        };
        if let Some(m) = alias {
            return Ok(m);
        }
        BlendMode::ALL
            .into_iter()
            .find(|m| m.id().replace('-', "") == key)
            .ok_or_else(|| format!("`{s}` isn't a blend mode. Modes: {}.", BlendMode::ALL.map(|m| m.id()).join(", ")))
    }
}

/// A layer mask: in canvas pixels, white shows the layer, black hides it.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerMask {
    #[serde(with = "plane_size")]
    pub mask: Mask,
    /// Off: the layer shows as if it had no mask (the mask is kept).
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

/// A non-destructive adjustment: changes the colours of everything under it (inside its group).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Adjustment {
    /// Input black and white points, midtone gamma, output range (0–255 like other editors).
    #[serde(rename_all = "camelCase")]
    Levels { input_black: f32, input_white: f32, gamma: f32, output_black: f32, output_white: f32 },
    /// Tone curves through points `[[in, out], …]` (0–255) for all channels (`rgb`), then per channel.
    #[serde(rename_all = "camelCase")]
    Curves {
        #[serde(default = "identity_curve")]
        rgb: Vec<[f32; 2]>,
        #[serde(default = "identity_curve")]
        red: Vec<[f32; 2]>,
        #[serde(default = "identity_curve")]
        green: Vec<[f32; 2]>,
        #[serde(default = "identity_curve")]
        blue: Vec<[f32; 2]>,
    },
    /// Hue shift in degrees (−180…180), saturation and lightness (−100…100); `colorize` tints
    /// everything with the hue.
    #[serde(rename_all = "camelCase")]
    HueSaturation {
        hue: f32,
        saturation: f32,
        lightness: f32,
        #[serde(default)]
        colorize: bool,
    },
    /// Exposure in stops (−5…5), offset (−0.5…0.5) and gamma (0.1…9.99), in linear light.
    #[serde(rename_all = "camelCase")]
    Exposure { exposure: f32, offset: f32, gamma: f32 },
    /// Brightness (−150…150) and contrast (−50…100).
    #[serde(rename_all = "camelCase")]
    BrightnessContrast { brightness: f32, contrast: f32 },
    /// Vibrance and saturation (−100…100): vibrance spares colours that are already strong.
    #[serde(rename_all = "camelCase")]
    Vibrance { vibrance: f32, saturation: f32 },
    /// Every colour turned around.
    Invert,
    /// Black and white, from a mix of the channels (percentages, default 30/59/11).
    #[serde(rename_all = "camelCase")]
    BlackWhite { red: f32, green: f32, blue: f32 },
    /// Pixels brighter than `level` (0–255) become white, the others black.
    #[serde(rename_all = "camelCase")]
    Threshold { level: f32 },
    /// `levels` tones per channel (2–255).
    #[serde(rename_all = "camelCase")]
    Posterize { levels: f32 },
    /// A 3D colour lookup table (a `.cube` file), mixed in at `amount` (0–1).
    #[serde(rename_all = "camelCase")]
    Lut {
        name: String,
        size: u32,
        #[serde(default = "one")]
        amount: f32,
        /// `size³` RGB triples, red changing fastest (the `.cube` order). Stored beside
        /// `document.json` in the file, never in it.
        #[serde(skip)]
        table: Arc<Vec<[f32; 3]>>,
    },
}

fn one() -> f32 {
    1.0
}

pub fn identity_curve() -> Vec<[f32; 2]> {
    vec![[0.0, 0.0], [255.0, 255.0]]
}

impl Adjustment {
    /// The kinds, as `layer.addAdjustment` names them.
    pub const KINDS: [&'static str; 10] = ["levels", "curves", "hueSaturation", "exposure", "brightnessContrast", "vibrance", "invert", "blackWhite", "threshold", "posterize"];

    /// A kind with its neutral settings (it changes nothing until edited, except invert,
    /// black & white, threshold and posterize, which do what they say at once).
    pub fn default_of(kind: &str) -> Result<Adjustment, String> {
        Ok(match kind {
            "levels" => Adjustment::Levels { input_black: 0.0, input_white: 255.0, gamma: 1.0, output_black: 0.0, output_white: 255.0 },
            "curves" => Adjustment::Curves { rgb: identity_curve(), red: identity_curve(), green: identity_curve(), blue: identity_curve() },
            "hueSaturation" => Adjustment::HueSaturation { hue: 0.0, saturation: 0.0, lightness: 0.0, colorize: false },
            "exposure" => Adjustment::Exposure { exposure: 0.0, offset: 0.0, gamma: 1.0 },
            "brightnessContrast" => Adjustment::BrightnessContrast { brightness: 0.0, contrast: 0.0 },
            "vibrance" => Adjustment::Vibrance { vibrance: 0.0, saturation: 0.0 },
            "invert" => Adjustment::Invert,
            "blackWhite" => Adjustment::BlackWhite { red: 30.0, green: 59.0, blue: 11.0 },
            "threshold" => Adjustment::Threshold { level: 128.0 },
            "posterize" => Adjustment::Posterize { levels: 4.0 },
            "lut" => return Err("A LUT adjustment is made from a .cube file: layer.addLut path=….".into()),
            other => {
                let hint = crate::closest(other, &Adjustment::KINDS).map(|c| format!(" Did you mean `{c}`?")).unwrap_or_default();
                return Err(format!("`{other}` isn't an adjustment.{hint} Kinds: {}.", Adjustment::KINDS.join(", ")));
            }
        })
    }

    /// The kind's id (`levels`, `hueSaturation`…).
    pub fn kind(&self) -> &'static str {
        match self {
            Adjustment::Levels { .. } => "levels",
            Adjustment::Curves { .. } => "curves",
            Adjustment::HueSaturation { .. } => "hueSaturation",
            Adjustment::Exposure { .. } => "exposure",
            Adjustment::BrightnessContrast { .. } => "brightnessContrast",
            Adjustment::Vibrance { .. } => "vibrance",
            Adjustment::Invert => "invert",
            Adjustment::BlackWhite { .. } => "blackWhite",
            Adjustment::Threshold { .. } => "threshold",
            Adjustment::Posterize { .. } => "posterize",
            Adjustment::Lut { .. } => "lut",
        }
    }

    /// What people call it.
    pub fn label(&self) -> &'static str {
        label_of(self.kind())
    }

    /// Changes some settings from JSON (`{"gamma": 1.2}`); unknown names and wrong types are
    /// refused with the names it takes.
    pub fn merge(&self, changes: &serde_json::Map<String, serde_json::Value>) -> Result<Adjustment, String> {
        let mut v = serde_json::to_value(self).map_err(|e| e.to_string())?;
        let obj = v.as_object_mut().ok_or("adjustment isn't an object")?;
        let known: Vec<String> = obj.keys().filter(|k| *k != "type").cloned().collect();
        for (k, val) in changes {
            if k == "type" {
                continue;
            }
            if !known.contains(k) {
                let names: Vec<&str> = known.iter().map(String::as_str).collect();
                let hint = crate::closest(k, &names).map(|c| format!(" Did you mean `{c}`?")).unwrap_or_default();
                return Err(format!("{} has no setting `{k}`.{hint} Settings: {}.", self.label(), if names.is_empty() { "none".into() } else { names.join(", ") }));
            }
            obj.insert(k.clone(), val.clone());
        }
        let mut out: Adjustment = serde_json::from_value(v).map_err(|e| format!("{}: {e}", self.label()))?;
        if let (Adjustment::Lut { table, .. }, Adjustment::Lut { table: t2, .. }) = (self, &mut out) {
            *t2 = table.clone();
        }
        out.clamp();
        Ok(out)
    }

    /// Keeps every setting in its range.
    pub fn clamp(&mut self) {
        let c = |v: &mut f32, lo: f32, hi: f32| *v = if v.is_finite() { v.clamp(lo, hi) } else { lo.max(0.0).min(hi) };
        match self {
            Adjustment::Levels { input_black, input_white, gamma, output_black, output_white } => {
                c(input_black, 0.0, 253.0);
                c(input_white, *input_black + 2.0, 255.0);
                c(gamma, 0.1, 9.99);
                c(output_black, 0.0, 255.0);
                c(output_white, 0.0, 255.0);
            }
            Adjustment::Curves { rgb, red, green, blue } => {
                for curve in [rgb, red, green, blue] {
                    for p in curve.iter_mut() {
                        c(&mut p[0], 0.0, 255.0);
                        c(&mut p[1], 0.0, 255.0);
                    }
                    curve.sort_by(|a, b| a[0].total_cmp(&b[0]));
                    curve.dedup_by(|a, b| (a[0] - b[0]).abs() < 0.5);
                    if curve.len() < 2 {
                        *curve = identity_curve();
                    }
                }
            }
            Adjustment::HueSaturation { hue, saturation, lightness, .. } => {
                c(hue, -180.0, 180.0);
                c(saturation, -100.0, 100.0);
                c(lightness, -100.0, 100.0);
            }
            Adjustment::Exposure { exposure, offset, gamma } => {
                c(exposure, -20.0, 20.0);
                c(offset, -0.5, 0.5);
                c(gamma, 0.01, 9.99);
            }
            Adjustment::BrightnessContrast { brightness, contrast } => {
                c(brightness, -150.0, 150.0);
                c(contrast, -50.0, 100.0);
            }
            Adjustment::Vibrance { vibrance, saturation } => {
                c(vibrance, -100.0, 100.0);
                c(saturation, -100.0, 100.0);
            }
            Adjustment::Invert => {}
            Adjustment::BlackWhite { red, green, blue } => {
                c(red, -200.0, 300.0);
                c(green, -200.0, 300.0);
                c(blue, -200.0, 300.0);
            }
            Adjustment::Threshold { level } => c(level, 1.0, 255.0),
            Adjustment::Posterize { levels } => c(levels, 2.0, 255.0),
            Adjustment::Lut { amount, .. } => c(amount, 0.0, 1.0),
        }
    }
}

/// The label of an adjustment kind.
pub fn label_of(kind: &str) -> &'static str {
    match kind {
        "levels" => "Levels",
        "curves" => "Curves",
        "hueSaturation" => "Hue/Saturation",
        "exposure" => "Exposure",
        "brightnessContrast" => "Brightness/Contrast",
        "vibrance" => "Vibrance",
        "invert" => "Invert",
        "blackWhite" => "Black & White",
        "threshold" => "Threshold",
        "posterize" => "Posterize",
        "lut" => "Color Lookup",
        _ => "Adjustment",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// A part of a text layer's words set differently (a character style, or bold, a colour…).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct TextRun {
    /// Byte range in the text.
    pub start: usize,
    pub end: usize,
    /// A character style by name (`Document::styles`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
}

/// A text layer: point text (a line that grows as you type, from its top-left corner at (x, y))
/// or a text frame (`frame`: words wrap inside it; what doesn't fit flows on to the `next`
/// frame, on this page or another, as in a layout program). `{page}` and `{pages}` in the words
/// become the page number and the page count.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TextLayer {
    pub text: String,
    /// A family name; the bundled ones (Manrope, IBM Plex Mono) are always there.
    pub font: String,
    /// In pixels.
    pub size: f32,
    /// 100–900.
    pub weight: u16,
    pub italic: bool,
    pub color: Color,
    pub x: f32,
    pub y: f32,
    pub align: TextAlign,
    /// Line spacing as a multiple of the size.
    pub line_height: f32,
    /// Extra space between letters, in pixels.
    pub letter_spacing: f32,
    /// Extra space after each paragraph, in pixels.
    pub paragraph_spacing: f32,
    /// A text frame's width and height: words wrap at its width and stop at its height.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame: Option<[f32; 2]>,
    /// The text frame the words continue in (a layer id). Only the first frame of a thread
    /// holds the words; the others show what flows into them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    /// The paragraph style it was set with (`text.applyStyle`); changing the style changes it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    /// Parts set differently.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<TextRun>,
}

impl Default for TextLayer {
    fn default() -> Self {
        Self {
            text: "Text".into(),
            font: "Manrope".into(),
            size: 72.0,
            weight: 600,
            italic: false,
            color: Color::BLACK,
            x: 0.0,
            y: 0.0,
            align: TextAlign::Left,
            line_height: 1.2,
            letter_spacing: 0.0,
            paragraph_spacing: 0.0,
            frame: None,
            next: None,
            style: None,
            runs: vec![],
        }
    }
}

/// What a layer holds.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Content {
    /// Pixels, placed with their top-left corner at (x, y) on the canvas.
    #[serde(rename_all = "camelCase")]
    Raster {
        x: i32,
        y: i32,
        #[serde(with = "plane_size")]
        pixels: Raster,
    },
    /// One colour over the whole canvas (a solid fill layer).
    #[serde(rename_all = "camelCase")]
    Fill { color: Color },
    #[serde(rename_all = "camelCase")]
    Adjustment { adjustment: Adjustment },
    #[serde(rename_all = "camelCase")]
    Text { text: TextLayer },
    /// A vector shape or path (stays sharp at any size).
    #[serde(rename_all = "camelCase")]
    Vector { shape: crate::vector::Shape },
    /// Layers composited together, then onto what is under the group (unless its blend mode is
    /// pass-through). `children[0]` is the top one.
    #[serde(rename_all = "camelCase")]
    Group {
        children: Vec<Layer>,
        /// Shown open in the Layers panel.
        #[serde(default = "yes")]
        expanded: bool,
    },
}

impl Content {
    pub fn kind(&self) -> &'static str {
        match self {
            Content::Raster { .. } => "raster",
            Content::Fill { .. } => "fill",
            Content::Adjustment { .. } => "adjustment",
            Content::Text { .. } => "text",
            Content::Vector { .. } => "vector",
            Content::Group { .. } => "group",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    pub id: String,
    pub name: String,
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default)]
    pub locked: bool,
    /// 0..1.
    #[serde(default = "one")]
    pub opacity: f32,
    #[serde(default)]
    pub blend: BlendMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<LayerMask>,
    /// Clipped to the layer under it (shows only where that layer has pixels).
    #[serde(default)]
    pub clipped: bool,
    #[serde(flatten)]
    pub content: Content,
}

impl Layer {
    pub fn new(id: impl Into<String>, name: impl Into<String>, content: Content) -> Self {
        Self { id: id.into(), name: name.into(), visible: true, locked: false, opacity: 1.0, blend: BlendMode::Normal, mask: None, clipped: false, content }
    }

    pub fn is_group(&self) -> bool {
        matches!(self.content, Content::Group { .. })
    }

    pub fn children(&self) -> &[Layer] {
        match &self.content {
            Content::Group { children, .. } => children,
            _ => &[],
        }
    }

    pub fn children_mut(&mut self) -> Option<&mut Vec<Layer>> {
        match &mut self.content {
            Content::Group { children, .. } => Some(children),
            _ => None,
        }
    }

    /// The pixels and where they sit, for raster layers.
    pub fn raster(&self) -> Option<(i32, i32, &Raster)> {
        match &self.content {
            Content::Raster { x, y, pixels } => Some((*x, *y, pixels)),
            _ => None,
        }
    }

    pub fn raster_mut(&mut self) -> Option<(&mut i32, &mut i32, &mut Raster)> {
        match &mut self.content {
            Content::Raster { x, y, pixels } => Some((x, y, pixels)),
            _ => None,
        }
    }

    /// Where the layer's own pixels are on the canvas (raster layers), else `None`
    /// (fills and adjustments cover everything; text is measured by the renderer).
    pub fn raster_bounds(&self) -> Option<Rect> {
        self.raster().map(|(x, y, p)| Rect::new(x, y, p.width(), p.height()))
    }

    /// Every layer in this one (itself first), depth first.
    pub fn walk(&self) -> Vec<&Layer> {
        let mut out = vec![self];
        for c in self.children() {
            out.extend(c.walk());
        }
        out
    }
}

/// Planes are written to the file as PNGs beside `document.json`; in JSON they are only their
/// size (and, for masks, the value outside any painted tile).
mod plane_size {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use crate::raster::Plane;

    #[derive(Serialize, Deserialize)]
    struct Size {
        width: u32,
        height: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fill: Option<u8>,
    }

    pub fn serialize<S: Serializer, const C: usize>(p: &Plane<C>, s: S) -> Result<S::Ok, S::Error> {
        let fill = (C == 1).then(|| p.fill()[0]);
        Size { width: p.width(), height: p.height(), fill }.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>, const C: usize>(d: D) -> Result<Plane<C>, D::Error> {
        let s = Size::deserialize(d)?;
        if s.width == 0 || s.height == 0 || s.width > crate::MAX_SIDE || s.height > crate::MAX_SIDE {
            return Err(serde::de::Error::custom(format!("a layer of {}×{} pixels is out of range", s.width, s.height)));
        }
        let mut fill = [0u8; C];
        if C == 1 {
            fill[0] = s.fill.unwrap_or(255);
        }
        Ok(Plane::new(s.width, s.height, fill))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_modes_parse_loosely() {
        assert_eq!(BlendMode::parse("multiply").unwrap(), BlendMode::Multiply);
        assert_eq!(BlendMode::parse("Color Burn").unwrap(), BlendMode::ColorBurn);
        assert_eq!(BlendMode::parse("colorBurn").unwrap(), BlendMode::ColorBurn);
        assert_eq!(BlendMode::parse("add").unwrap(), BlendMode::LinearDodge);
        assert!(BlendMode::parse("sparkle").is_err());
        for m in BlendMode::ALL {
            assert_eq!(BlendMode::parse(m.id()).unwrap(), m);
            assert_eq!(serde_json::to_value(m).unwrap(), serde_json::json!(m.id()));
        }
    }

    #[test]
    fn adjustments_merge_and_refuse_unknown_settings() {
        let a = Adjustment::default_of("levels").unwrap();
        let mut ch = serde_json::Map::new();
        ch.insert("gamma".into(), serde_json::json!(1.5));
        let b = a.merge(&ch).unwrap();
        assert!(matches!(b, Adjustment::Levels { gamma, .. } if (gamma - 1.5).abs() < 1e-6));
        ch.insert("gama".into(), serde_json::json!(2));
        let e = a.merge(&ch).unwrap_err();
        assert!(e.contains("Did you mean `gamma`"), "{e}");
        assert!(Adjustment::default_of("levles").unwrap_err().contains("levels"));
        for k in Adjustment::KINDS {
            let a = Adjustment::default_of(k).unwrap();
            assert_eq!(a.kind(), k);
            let back: Adjustment = serde_json::from_value(serde_json::to_value(&a).unwrap()).unwrap();
            assert_eq!(back, a);
        }
    }

    #[test]
    fn layers_round_trip_through_json() {
        let mut l = Layer::new("L1", "Sky", Content::Raster { x: 4, y: -2, pixels: Raster::transparent(10, 20) });
        l.mask = Some(LayerMask { mask: Mask::filled(10, 20, 255), enabled: true });
        let v = serde_json::to_value(&l).unwrap();
        assert_eq!(v["kind"], "raster");
        assert_eq!(v["pixels"]["width"], 10);
        assert_eq!(v["mask"]["mask"]["fill"], 255);
        let back: Layer = serde_json::from_value(v).unwrap();
        assert_eq!(back.raster_bounds(), Some(Rect::new(4, -2, 10, 20)));
        let g = Layer::new("L2", "Group", Content::Group { children: vec![back], expanded: true });
        let v = serde_json::to_value(&g).unwrap();
        let back: Layer = serde_json::from_value(v).unwrap();
        assert_eq!(back.children().len(), 1);
    }
}
