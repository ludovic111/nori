//! Example nori plugin: Halftone. Turns tones into round dots on a turned grid, like a printed
//! newspaper photo. It reads a whole cell around each pixel, so it asks for a margin.

use nori_plugin::prelude::*;

pub struct Halftone;

impl Filter for Halftone {
    const INFO: Info = Info::filter("xyz.lsuite.nori.halftone", "Halftone", "lsuite", "stylize", "Turns tones into dots on a grid, like a printed newspaper photo.");

    fn params() -> Vec<Param> {
        vec![Param::integer("cell", "Cell size", 3.0, 64.0, 8.0, "px"), Param::number("angle", "Angle", 0.0, 90.0, 45.0, "°"), Param::switch("color", "Keep colour", false)]
    }

    fn new() -> Self {
        Halftone
    }

    fn margin(&self, values: &[f64]) -> u32 {
        values[0].ceil() as u32 * 2
    }

    fn process(&mut self, tile: &mut Tile, values: &[f64]) {
        let cell = values[0] as f32;
        let (s, c) = (values[1] as f32).to_radians().sin_cos();
        let keep = values[2] >= 0.5;
        let src: Vec<[f32; 4]> = tile.pixels.to_vec();
        let (w, h) = (tile.width as i32, tile.height as i32);
        let at = |x: i32, y: i32| src[(y.clamp(0, h - 1) * w + x.clamp(0, w - 1)) as usize];
        for y in 0..h {
            for x in 0..w {
                // Page position, turned onto the grid.
                let (px, py) = ((tile.x + x) as f32 + 0.5, (tile.y + y) as f32 + 0.5);
                let (u, v) = (px * c + py * s, -px * s + py * c);
                let (cu, cv) = ((u / cell).floor() * cell + cell / 2.0, (v / cell).floor() * cell + cell / 2.0);
                // The cell's centre back on the page, and its tone there.
                let (gx, gy) = (cu * c - cv * s, cu * s + cv * c);
                let sample = at(gx as i32 - tile.x, gy as i32 - tile.y);
                let (col, a) = unpremultiply(sample);
                let ink = 1.0 - luma(col);
                let r = (ink.max(0.0)).sqrt() * cell * 0.72;
                let d = ((u - cu).powi(2) + (v - cv).powi(2)).sqrt();
                let cover = (r - d + 0.5).clamp(0.0, 1.0);
                let dot = if keep { col } else { [0.0; 3] };
                let paper = [1.0; 3];
                let out = [0, 1, 2].map(|i| paper[i] + (dot[i] - paper[i]) * cover);
                tile.pixels[(y * w + x) as usize] = premultiply(out, a.max(at(x, y)[3]));
            }
        }
    }
}

export_plugins!(Halftone);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_stays_paper_and_black_fills_in() {
        let mut white = vec![[1.0f32; 4]; 64];
        Halftone.process(&mut Tile { pixels: &mut white, width: 8, height: 8, x: 0, y: 0 }, &[8.0, 0.0, 0.0]);
        assert!(white.iter().all(|p| p[0] > 0.99));
        let mut black = vec![[0.0, 0.0, 0.0, 1.0]; 64];
        Halftone.process(&mut Tile { pixels: &mut black, width: 8, height: 8, x: 0, y: 0 }, &[8.0, 0.0, 0.0]);
        let dark = black.iter().filter(|p| p[0] < 0.5).count();
        assert!(dark > 40, "{dark}");
    }
}
