//! Example nori plugin: Duotone. Maps dark tones to one colour and light tones to another
//! (the two-ink look of screen prints and posters).

use nori_plugin::prelude::*;

pub struct Duotone;

const PALETTES: [(&str, &str, &str); 4] = [("Ink", "#111111", "#f2efe6"), ("Sunset", "#2b1055", "#ff9a62"), ("Sea", "#00223e", "#7fffd4"), ("Risograph", "#1d3fbb", "#ff48b0")];

impl Filter for Duotone {
    const INFO: Info = Info::filter("xyz.lsuite.nori.duotone", "Duotone", "lsuite", "color", "Maps dark tones to one colour and light tones to another.");

    fn params() -> Vec<Param> {
        vec![Param::choice("palette", "Palette", &["ink", "sunset", "sea", "risograph"], 0), Param::number("contrast", "Contrast", -100.0, 100.0, 0.0, ""), Param::number("mix", "Mix", 0.0, 100.0, 100.0, "%")]
    }

    fn new() -> Self {
        Duotone
    }

    fn process(&mut self, tile: &mut Tile, values: &[f64]) {
        let (_, dark, light) = PALETTES[(values[0] as usize).min(PALETTES.len() - 1)];
        let (dark, light) = (hex(dark), hex(light));
        let contrast = 1.0 + (values[1] / 100.0) as f32;
        let mix = (values[2] / 100.0) as f32;
        for p in tile.pixels.iter_mut() {
            let (c, a) = unpremultiply(*p);
            if a <= 0.0 {
                continue;
            }
            let l = ((luma(c) - 0.5) * contrast + 0.5).clamp(0.0, 1.0);
            let d = [0, 1, 2].map(|i| dark[i] + (light[i] - dark[i]) * l);
            *p = premultiply([0, 1, 2].map(|i| c[i] + (d[i] - c[i]) * mix), a);
        }
    }
}

export_plugins!(Duotone);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn black_and_white_become_the_two_inks() {
        let mut px = vec![[0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0]];
        let mut t = Tile { pixels: &mut px, width: 2, height: 1, x: 0, y: 0 };
        Duotone.process(&mut t, &[1.0, 0.0, 100.0]);
        let (dark, light) = (hex("#2b1055"), hex("#ff9a62"));
        assert!((px[0][0] - dark[0]).abs() < 1e-4 && (px[1][2] - light[2]).abs() < 1e-4);
    }
}
