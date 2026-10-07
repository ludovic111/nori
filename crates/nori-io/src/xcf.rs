#![allow(clippy::chunks_exact_to_as_chunks)] // Slice iterators preserve existing comparison and byte-decoding types.
//! Bounded XCF reader following https://developer.gimp.org/core/standards/xcf/.
//! RGB/grayscale 8-bit files, layers, groups, masks, offsets, opacity and common blend modes;
//! raw, RLE and zlib tiles. High-bit-depth and newer effect formats are refused explicitly.
use nori_core::{BlendMode, Content, Document, Layer, LayerMask, Mask, Raster, Rect};
use std::collections::BTreeMap;
use std::io::Read;
type Result<T> = std::result::Result<T, String>;
#[derive(Clone)]
struct Reader<'a> {
    data: &'a [u8],
    at: usize,
    wide: bool,
}
impl<'a> Reader<'a> {
    fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).ok_or("XCF offset overflow")?;
        let out = self.data.get(self.at..end).ok_or("Truncated XCF")?;
        self.at = end;
        Ok(out)
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    fn pointer(&mut self) -> Result<usize> {
        let v = if self.wide { u64::from_be_bytes(self.bytes(8)?.try_into().unwrap()) } else { self.u32()? as u64 };
        if v > self.data.len() as u64 {
            return Err("XCF pointer is outside the file".into());
        }
        Ok(v as usize)
    }
    fn at(&self, at: usize) -> Self {
        Self { at, ..self.clone() }
    }
    fn string(&mut self) -> Result<String> {
        let n = self.u32()? as usize;
        if n > 1_000_000 {
            return Err("XCF string is too large".into());
        }
        Ok(String::from_utf8_lossy(self.bytes(n)?).trim_end_matches('\0').to_string())
    }
    fn props(&mut self) -> Result<BTreeMap<u32, &'a [u8]>> {
        let mut out = BTreeMap::new();
        for _ in 0..1024 {
            let kind = self.u32()?;
            let n = self.u32()? as usize;
            let data = self.bytes(n)?;
            if kind == 0 {
                return Ok(out);
            }
            out.insert(kind, data);
        }
        Err("Too many XCF properties".into())
    }
}
fn number(p: &BTreeMap<u32, &[u8]>, key: u32, default: u32) -> u32 {
    p.get(&key).and_then(|v| v.get(..4)).map(|v| u32::from_be_bytes(v.try_into().unwrap())).unwrap_or(default)
}
fn size(w: u32, h: u32) -> Result<usize> {
    if w == 0 || h == 0 || w > nori_core::MAX_SIDE || h > nori_core::MAX_SIDE || w as u64 * h as u64 > 64_000_000 {
        return Err("XCF image exceeds the 64 million pixel import limit".into());
    }
    Ok(w as usize * h as usize)
}
fn plane(r: &Reader, ptr: usize, w: u32, h: u32, channels: usize, compression: u8) -> Result<Vec<u8>> {
    let pixels = size(w, h)?;
    let mut hierarchy = r.at(ptr);
    if hierarchy.u32()? != w || hierarchy.u32()? != h || hierarchy.u32()? as usize != channels {
        return Err("Inconsistent XCF hierarchy".into());
    }
    let mut level = r.at(hierarchy.pointer()?);
    if level.u32()? != w || level.u32()? != h {
        return Err("Inconsistent XCF level".into());
    }
    let mut out = vec![0; pixels * channels];
    for ty in (0..h).step_by(64) {
        for tx in (0..w).step_by(64) {
            let mut tile = r.at(level.pointer()?);
            let tw = (w - tx).min(64) as usize;
            let th = (h - ty).min(64) as usize;
            let count = tw * th;
            let data = match compression {
                0 => tile.bytes(count * channels)?.to_vec(),
                2 => {
                    let mut out = Vec::new();
                    flate2::read::ZlibDecoder::new(&r.data[tile.at..]).take((count * channels + 1) as u64).read_to_end(&mut out).map_err(|e| e.to_string())?;
                    if out.len() != count * channels {
                        return Err("Invalid zlib tile size".into());
                    }
                    out
                }
                1 => {
                    let mut out = vec![0; count * channels];
                    for c in 0..channels {
                        let mut pos = 0;
                        while pos < count {
                            let op = tile.bytes(1)?[0];
                            let n = match op {
                                127 | 128 => u16::from_be_bytes(tile.bytes(2)?.try_into().unwrap()) as usize,
                                0..=126 => op as usize + 1,
                                _ => 256 - op as usize,
                            };
                            if n == 0 || pos + n > count {
                                return Err("Invalid XCF RLE run".into());
                            }
                            if op <= 127 {
                                let value = tile.bytes(1)?[0];
                                for i in pos..pos + n {
                                    out[i * channels + c] = value;
                                }
                            } else {
                                let values = tile.bytes(n)?;
                                for (i, value) in values.iter().enumerate() {
                                    out[(pos + i) * channels + c] = *value;
                                }
                            }
                            pos += n;
                        }
                    }
                    out
                }
                _ => return Err("Unsupported XCF compression".into()),
            };
            for row in 0..th {
                let dest = ((ty as usize + row) * w as usize + tx as usize) * channels;
                out[dest..dest + tw * channels].copy_from_slice(&data[row * tw * channels..(row + 1) * tw * channels]);
            }
        }
    }
    Ok(out)
}
fn mode(id: u32) -> Result<BlendMode> {
    Ok(match id {
        0 | 28 => BlendMode::Normal,
        3 | 30 => BlendMode::Multiply,
        4 | 31 => BlendMode::Screen,
        5 | 19 | 45 => BlendMode::SoftLight,
        6 | 32 => BlendMode::Difference,
        7 | 33 => BlendMode::LinearDodge,
        8 | 34 => BlendMode::Subtract,
        9 | 35 => BlendMode::Darken,
        10 | 36 => BlendMode::Lighten,
        13 | 39 => BlendMode::Color,
        15 | 41 => BlendMode::Divide,
        16 | 42 => BlendMode::ColorDodge,
        17 | 43 => BlendMode::ColorBurn,
        18 | 44 => BlendMode::HardLight,
        23 => BlendMode::Overlay,
        48 => BlendMode::VividLight,
        49 => BlendMode::PinLight,
        50 => BlendMode::LinearLight,
        52 => BlendMode::Exclusion,
        53 => BlendMode::LinearBurn,
        56 => BlendMode::Luminosity,
        61 => BlendMode::PassThrough,
        _ => return Err(format!("XCF layer mode {id} isn't supported. Export OpenRaster from GIMP to preserve its appearance.")),
    })
}
pub fn read(bytes: &[u8], name: &str) -> Result<Document> {
    let mut r = Reader { data: bytes, at: 0, wide: false };
    if r.bytes(9)? != b"gimp xcf " {
        return Err("Not an XCF file".into());
    }
    let tag = r.bytes(4)?;
    let version = if tag == b"file" { 0 } else { std::str::from_utf8(&tag[1..]).ok().and_then(|s| s.parse::<u32>().ok()).ok_or("Invalid XCF version")? };
    if version > 13 {
        return Err(format!("XCF v{version} uses newer GIMP features. Export OpenRaster, or save an 8-bit compatibility copy."));
    }
    r.bytes(1)?;
    r.wide = version >= 11;
    let (w, h, base) = (r.u32()?, r.u32()?, r.u32()?);
    size(w, h)?;
    let precision = if version >= 4 { r.u32()? } else { 150 };
    if base > 1 || !matches!(precision, 0 | 150) {
        return Err("XCF import currently needs 8-bit non-linear RGB or grayscale. Convert precision in GIMP or export OpenRaster.".into());
    }
    let props = r.props()?;
    let compression = props.get(&17).and_then(|v| v.first()).copied().unwrap_or(1);
    let mut doc = Document::empty(name, w, h);
    let mut nodes = BTreeMap::new();
    let mut total = 0usize;
    for index in 0..4096u32 {
        let ptr = r.pointer()?;
        if ptr == 0 {
            break;
        }
        if index == 4095 {
            return Err("Too many XCF layers".into());
        }
        let mut l = r.at(ptr);
        let (lw, lh, kind) = (l.u32()?, l.u32()?, l.u32()?);
        total += size(lw, lh)?;
        if total > 64_000_000 {
            return Err("XCF layers exceed the memory limit".into());
        }
        let label = l.string()?;
        let props = l.props()?;
        let hierarchy = l.pointer()?;
        let mask_ptr = l.pointer()?;
        let mut x = 0;
        let mut y = 0;
        if let Some(data) = props.get(&15)
            && data.len() == 8 {
                x = i32::from_be_bytes(data[..4].try_into().unwrap());
                y = i32::from_be_bytes(data[4..].try_into().unwrap());
            }
        let content = if props.contains_key(&29) {
            Content::Group { children: vec![], expanded: number(&props, 31, 1) != 0 }
        } else {
            let channels = match kind {
                0 => 3,
                1 => 4,
                2 => 1,
                3 => 2,
                _ => return Err("Indexed XCF layers aren't supported".into()),
            };
            let pixels = plane(&r, hierarchy, lw, lh, channels, compression)?;
            let rgba: Vec<u8> = pixels
                .chunks_exact(channels)
                .flat_map(|p| match kind {
                    0 => [p[0], p[1], p[2], 255],
                    1 => [p[0], p[1], p[2], p[3]],
                    2 => [p[0], p[0], p[0], 255],
                    _ => [p[0], p[0], p[0], p[1]],
                })
                .collect();
            Content::Raster { x, y, pixels: Raster::from_rgba(lw, lh, &rgba) }
        };
        let mut layer = Layer::new(doc.new_id(), label, content);
        layer.visible = number(&props, 8, 1) != 0;
        layer.opacity = if props.contains_key(&33) { f32::from_bits(number(&props, 33, 1f32.to_bits())).clamp(0., 1.) } else { number(&props, 6, 255).min(255) as f32 / 255. };
        layer.blend = mode(number(&props, 7, 0))?;
        if mask_ptr != 0 {
            let mut m = r.at(mask_ptr);
            let (mw, mh) = (m.u32()?, m.u32()?);
            m.string()?;
            let _ = m.props()?;
            let data = plane(&r, m.pointer()?, mw, mh, 1, compression)?;
            let mut mask = Mask::filled(w, h, 0);
            mask.write_rect(Rect::new(x, y, mw, mh), &data);
            layer.mask = Some(LayerMask { mask, enabled: number(&props, 11, 1) != 0 });
        }
        let path = props
            .get(&30)
            .map(|v| v.chunks_exact(4).map(|v| u32::from_be_bytes(v.try_into().unwrap())).collect::<Vec<_>>())
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| vec![index]);
        if path.len() > 32 || nodes.insert(path, layer).is_some() {
            return Err("Invalid XCF layer tree".into());
        }
    }
    fn assemble(nodes: &BTreeMap<Vec<u32>, Layer>, parent: &[u32]) -> Vec<Layer> {
        nodes
            .iter()
            .filter(|(key, _)| key.len() == parent.len() + 1 && key.starts_with(parent))
            .map(|(key, layer)| {
                let mut l = layer.clone();
                if let Content::Group { children, .. } = &mut l.content {
                    *children = assemble(nodes, key);
                }
                l
            })
            .collect()
    }
    doc.pages[0].layers = assemble(&nodes, &[]);
    doc.active = doc.pages[0].layers.first().map(|l| l.id.clone());
    Ok(doc)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn u32b(v: &mut Vec<u8>, x: u32) {
        v.extend(x.to_be_bytes());
    }
    fn fixture(compression: u8) -> Vec<u8> {
        let mut b = b"gimp xcf file\0".to_vec();
        for v in [2, 1, 0, 17, 1] {
            u32b(&mut b, v);
        }
        b.push(compression);
        for v in [0, 0] {
            u32b(&mut b, v);
        }
        let table = b.len();
        b.extend([0; 12]);
        let layer = b.len();
        for v in [2, 1, 1, 4] {
            u32b(&mut b, v);
        }
        b.extend(b"Red\0");
        for v in [0, 0] {
            u32b(&mut b, v);
        }
        let hp = b.len();
        b.extend([0; 8]);
        let hierarchy = b.len();
        for v in [2, 1, 4] {
            u32b(&mut b, v);
        }
        let lp = b.len();
        b.extend([0; 8]);
        let level = b.len();
        for v in [2, 1] {
            u32b(&mut b, v);
        }
        let tp = b.len();
        b.extend([0; 8]);
        let tile = b.len();
        let raw = [255, 0, 0, 255, 255, 0, 0, 255];
        match compression {
            0 => b.extend(raw),
            1 => b.extend([1, 255, 1, 0, 1, 0, 1, 255]),
            _ => {
                use std::io::Write;
                let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                z.write_all(&raw).unwrap();
                b.extend(z.finish().unwrap());
            }
        }
        for (at, value) in [(table, layer), (hp, hierarchy), (lp, level), (tp, tile)] {
            b[at..at + 4].copy_from_slice(&(value as u32).to_be_bytes());
        }
        b
    }
    #[test]
    fn decodes_raw_rle_and_zlib_layers() {
        for c in 0..=2 {
            let d = read(&fixture(c), "GIMP").unwrap();
            assert_eq!(d.pages[0].layers[0].raster().unwrap().2.to_vec(), [255, 0, 0, 255].repeat(2));
        }
    }
    #[test]
    fn refuses_truncated_and_oversized_files() {
        let b = fixture(1);
        for end in 0..b.len() {
            assert!(read(&b[..end], "bad").is_err());
        }
        let mut b = fixture(1);
        b[14..18].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(read(&b, "bad").is_err());
    }
}
