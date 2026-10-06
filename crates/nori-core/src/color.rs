//! Colours: straight RGBA, 0..1 a channel, written `#rrggbb` or `#rrggbbaa` in files and
//! commands.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const BLACK: Color = Color::rgb(0.0, 0.0, 0.0);
    pub const WHITE: Color = Color::rgb(1.0, 1.0, 1.0);
    pub const TRANSPARENT: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };

    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    pub fn from_u8(p: [u8; 4]) -> Self {
        Self { r: p[0] as f32 / 255.0, g: p[1] as f32 / 255.0, b: p[2] as f32 / 255.0, a: p[3] as f32 / 255.0 }
    }

    pub fn to_u8(self) -> [u8; 4] {
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        [q(self.r), q(self.g), q(self.b), q(self.a)]
    }

    /// `#rrggbb`, `#rrggbbaa`, `#rgb`, or a CSS colour name (black, white, red, green, blue,
    /// grey/gray, transparent).
    pub fn parse(s: &str) -> Result<Self, String> {
        let t = s.trim().to_ascii_lowercase();
        let named = match t.as_str() {
            "black" => Some(Color::BLACK),
            "white" => Some(Color::WHITE),
            "red" => Some(Color::rgb(1.0, 0.0, 0.0)),
            "green" => Some(Color::rgb(0.0, 0.5, 0.0)),
            "blue" => Some(Color::rgb(0.0, 0.0, 1.0)),
            "grey" | "gray" => Some(Color::rgb(0.5, 0.5, 0.5)),
            "transparent" => Some(Color::TRANSPARENT),
            _ => None,
        };
        if let Some(c) = named {
            return Ok(c);
        }
        let hex = t.strip_prefix('#').unwrap_or(&t);
        let bad = || format!("`{s}` isn't a colour: use #rrggbb or #rrggbbaa");
        if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(bad());
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| bad());
        match hex.len() {
            3 => {
                let d = |i: usize| u8::from_str_radix(&hex[i..i + 1], 16).map(|v| v * 17).map_err(|_| bad());
                Ok(Color::from_u8([d(0)?, d(1)?, d(2)?, 255]))
            }
            6 => Ok(Color::from_u8([byte(0)?, byte(2)?, byte(4)?, 255])),
            8 => Ok(Color::from_u8([byte(0)?, byte(2)?, byte(4)?, byte(6)?])),
            _ => Err(bad()),
        }
    }

    /// `#rrggbb` when opaque, else `#rrggbbaa`.
    pub fn hex(self) -> String {
        let [r, g, b, a] = self.to_u8();
        if a == 255 { format!("#{r:02x}{g:02x}{b:02x}") } else { format!("#{r:02x}{g:02x}{b:02x}{a:02x}") }
    }

    pub fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    /// Rec. 709 luma of the colour (0..1).
    pub fn luma(self) -> f32 {
        0.2126 * self.r + 0.7152 * self.g + 0.0722 * self.b
    }
}

impl Default for Color {
    fn default() -> Self {
        Color::BLACK
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.hex())
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Color::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_writes_hex() {
        assert_eq!(Color::parse("#ff8000").unwrap().to_u8(), [255, 128, 0, 255]);
        assert_eq!(Color::parse("FF800080").unwrap().to_u8(), [255, 128, 0, 128]);
        assert_eq!(Color::parse("#fff").unwrap(), Color::WHITE);
        assert_eq!(Color::parse("white").unwrap(), Color::WHITE);
        assert!(Color::parse("#12345").is_err());
        assert!(Color::parse("nope").is_err());
        assert_eq!(Color::from_u8([1, 2, 3, 255]).hex(), "#010203");
        assert_eq!(Color::from_u8([1, 2, 3, 4]).hex(), "#01020304");
    }
}
