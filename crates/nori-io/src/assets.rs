//! Things other editors make that nori reads: GIMP brushes (`.gbr`, also what Krita and
//! Photopea use for picture tips) and Adobe Swatch Exchange palettes (`.ase`, from Photoshop,
//! Illustrator, InDesign and the Affinity apps).

use nori_core::Color;
use nori_render::brush::TipImage;

fn be32(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]])).ok_or_else(|| "the file ends early".to_string())
}

fn be16(b: &[u8], at: usize) -> Result<u16, String> {
    b.get(at..at + 2).map(|s| u16::from_be_bytes([s[0], s[1]])).ok_or_else(|| "the file ends early".to_string())
}

/// Reads a GIMP brush (version 1 or 2, grey or colour).
pub fn read_gbr(bytes: &[u8], fallback_name: &str) -> Result<TipImage, String> {
    let header = be32(bytes, 0)? as usize;
    let version = be32(bytes, 4)?;
    let (w, h) = (be32(bytes, 8)?, be32(bytes, 12)?);
    let depth = be32(bytes, 16)?;
    if !(1..=2).contains(&version) && version != 3 {
        return Err(format!("GIMP brush version {version} isn't supported"));
    }
    if w == 0 || h == 0 || w > 10_000 || h > 10_000 {
        return Err(format!("a {w}×{h} brush is out of range"));
    }
    if version >= 2 && bytes.get(20..24) != Some(b"GIMP") {
        return Err("not a GIMP brush (no GIMP magic)".into());
    }
    let name_start = if version >= 2 { 28 } else { 20 };
    let name = bytes.get(name_start..header).map(|n| String::from_utf8_lossy(n).trim_end_matches('\0').trim().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| fallback_name.to_string());
    let px = (w * h) as usize;
    let data = bytes.get(header..header + px * depth as usize).ok_or("the brush's pixels are cut short")?;
    let data: Vec<u8> = match depth {
        1 => data.to_vec(),
        4 => data.chunks_exact(4).map(|p| p[3]).collect(),
        d => return Err(format!("a brush with {d} bytes a pixel isn't supported")),
    };
    Ok(TipImage { name, width: w, height: h, data })
}

/// One colour of a swatch palette.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Swatch {
    pub name: String,
    pub color: Color,
    /// The palette group it was in, if any.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub group: String,
}

fn lab_to_rgb(l: f32, a: f32, b: f32) -> Color {
    // CIE L*a*b* (D50) → sRGB, as other apps do for swatches.
    let fy = (l + 16.0) / 116.0;
    let fx = fy + a / 500.0;
    let fz = fy - b / 200.0;
    let f = |t: f32| if t > 6.0 / 29.0 { t * t * t } else { 3.0 * (6.0f32 / 29.0).powi(2) * (t - 4.0 / 29.0) };
    let (x, y, z) = (0.9642 * f(fx), f(fy), 0.8249 * f(fz));
    let r = 3.1339 * x - 1.6169 * y - 0.4906 * z;
    let g = -0.9788 * x + 1.9161 * y + 0.0335 * z;
    let bl = 0.0719 * x - 0.2290 * y + 1.4052 * z;
    let s = |c: f32| nori_render::adjust::linear_to_srgb(c.clamp(0.0, 1.0));
    Color::rgb(s(r), s(g), s(bl))
}

/// Reads an Adobe Swatch Exchange file.
pub fn read_ase(bytes: &[u8]) -> Result<Vec<Swatch>, String> {
    if bytes.get(0..4) != Some(b"ASEF") {
        return Err("not an Adobe Swatch Exchange file".into());
    }
    let blocks = be32(bytes, 8)?;
    let mut at = 12;
    let mut out = vec![];
    let mut group = String::new();
    for _ in 0..blocks.min(100_000) {
        let kind = be16(bytes, at)?;
        let len = be32(bytes, at + 2)? as usize;
        let body = bytes.get(at + 6..at + 6 + len).ok_or("a block is cut short")?;
        at += 6 + len;
        let name = || -> Result<(String, usize), String> {
            let chars = be16(body, 0)? as usize;
            let raw = body.get(2..2 + chars * 2).ok_or("a name is cut short")?;
            let units: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).take_while(|u| *u != 0).collect();
            Ok((String::from_utf16_lossy(&units), 2 + chars * 2))
        };
        match kind {
            0xC001 => group = name()?.0,
            0xC002 => group.clear(),
            0x0001 => {
                let (n, off) = name()?;
                let model = body.get(off..off + 4).ok_or("a colour is cut short")?;
                let f = |i: usize| -> Result<f32, String> { Ok(f32::from_bits(be32(body, off + 4 + i * 4)?)) };
                let color = match model {
                    b"RGB " => Color::rgb(f(0)?, f(1)?, f(2)?),
                    b"CMYK" => {
                        let (c, m, y, k) = (f(0)?, f(1)?, f(2)?, f(3)?);
                        Color::rgb((1.0 - c) * (1.0 - k), (1.0 - m) * (1.0 - k), (1.0 - y) * (1.0 - k))
                    }
                    b"Gray" => {
                        let g = f(0)?;
                        Color::rgb(g, g, g)
                    }
                    b"LAB " => lab_to_rgb(f(0)? * 100.0, f(1)?, f(2)?),
                    _ => continue,
                };
                out.push(Swatch { name: n, color, group: group.clone() });
            }
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_gimp_brush() {
        let name = b"Dot\0";
        let header = 28 + name.len() as u32;
        let mut b = vec![];
        for v in [header, 2, 2, 2, 1] {
            b.extend_from_slice(&v.to_be_bytes());
        }
        b.extend_from_slice(b"GIMP");
        b.extend_from_slice(&25u32.to_be_bytes());
        b.extend_from_slice(name);
        b.extend_from_slice(&[0, 255, 255, 0]);
        let t = read_gbr(&b, "x").unwrap();
        assert_eq!((t.name.as_str(), t.width, t.height), ("Dot", 2, 2));
        assert_eq!(t.data, vec![0, 255, 255, 0]);
        assert!(read_gbr(&b[..10], "x").is_err());
    }

    #[test]
    fn reads_swatches() {
        let mut b = b"ASEF".to_vec();
        b.extend_from_slice(&[0, 1, 0, 0]);
        b.extend_from_slice(&1u32.to_be_bytes());
        let mut body = vec![];
        let name: Vec<u16> = "Red\0".encode_utf16().collect();
        body.extend_from_slice(&(name.len() as u16).to_be_bytes());
        for u in name {
            body.extend_from_slice(&u.to_be_bytes());
        }
        body.extend_from_slice(b"RGB ");
        for v in [1.0f32, 0.0, 0.0] {
            body.extend_from_slice(&v.to_bits().to_be_bytes());
        }
        body.extend_from_slice(&2u16.to_be_bytes());
        b.extend_from_slice(&1u16.to_be_bytes());
        b.extend_from_slice(&(body.len() as u32).to_be_bytes());
        b.extend_from_slice(&body);
        let s = read_ase(&b).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "Red");
        assert_eq!(s[0].color, Color::rgb(1.0, 0.0, 0.0));
    }
}
