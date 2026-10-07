//! Pixels, stored in tiles shared between copies.
//!
//! A [`Raster`] is RGBA (8 bits a channel, straight alpha) and a [`Mask`] is one grey channel.
//! Both are a grid of [`TILE`]×[`TILE`] tiles behind `Arc`s: cloning a raster copies a list of
//! pointers, and writing to a tile copies that tile only if someone else still holds it
//! (`Arc::make_mut`). This is what makes the undo history cheap: every step keeps a whole
//! [`Document`](crate::Document), and two steps share every tile the step between them didn't
//! touch. A tile that was never written is `None` and reads as the plane's `fill` value
//! (transparent for layers, white or black for masks).

use std::sync::Arc;

/// Side of a tile in pixels.
pub const TILE: u32 = 256;

/// One plane of tiles with `C` channels.
#[derive(Clone, Debug)]
pub struct Plane<const C: usize> {
    width: u32,
    height: u32,
    fill: [u8; C],
    tiles: Vec<Option<Arc<Vec<u8>>>>,
}

pub type Raster = Plane<4>;
pub type Mask = Plane<1>;

/// A rectangle in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }

    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }

    pub fn right(&self) -> i32 {
        self.x + self.w as i32
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h as i32
    }

    pub fn intersect(&self, o: &Rect) -> Rect {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = self.right().min(o.right());
        let y1 = self.bottom().min(o.bottom());
        if x1 <= x0 || y1 <= y0 { Rect::default() } else { Rect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32) }
    }

    pub fn union(&self, o: &Rect) -> Rect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        let x0 = self.x.min(o.x);
        let y0 = self.y.min(o.y);
        let x1 = self.right().max(o.right());
        let y1 = self.bottom().max(o.bottom());
        Rect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32)
    }

    pub fn translate(&self, dx: i32, dy: i32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }

    /// Grown by `n` pixels on every side.
    pub fn inflate(&self, n: i32) -> Rect {
        Rect::new(self.x - n, self.y - n, (self.w as i32 + 2 * n).max(0) as u32, (self.h as i32 + 2 * n).max(0) as u32)
    }
}

impl<const C: usize> Plane<C> {
    /// A plane where every pixel is `fill`.
    pub fn new(width: u32, height: u32, fill: [u8; C]) -> Self {
        let (tx, ty) = (width.div_ceil(TILE), height.div_ceil(TILE));
        Self { width, height, fill, tiles: vec![None; (tx * ty) as usize] }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn fill(&self) -> [u8; C] {
        self.fill
    }

    pub fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.width, self.height)
    }

    /// Tiles across and down.
    pub fn grid(&self) -> (u32, u32) {
        (self.width.div_ceil(TILE), self.height.div_ceil(TILE))
    }

    /// The pixels a tile covers (the last row and column of tiles are cut at the edge).
    pub fn tile_rect(&self, tx: u32, ty: u32) -> Rect {
        let x = tx * TILE;
        let y = ty * TILE;
        Rect::new(x as i32, y as i32, TILE.min(self.width - x), TILE.min(self.height - y))
    }

    fn index(&self, tx: u32, ty: u32) -> usize {
        (ty * self.grid().0 + tx) as usize
    }

    /// A tile's pixels (`TILE * TILE * C` bytes, rows of `TILE`), or `None` when it is all `fill`.
    pub fn tile(&self, tx: u32, ty: u32) -> Option<&[u8]> {
        self.tiles.get(self.index(tx, ty)).and_then(|t| t.as_deref().map(Vec::as_slice))
    }

    /// The tile's shared pointer (to tell whether two copies still share it).
    pub fn tile_arc(&self, tx: u32, ty: u32) -> Option<&Arc<Vec<u8>>> {
        self.tiles.get(self.index(tx, ty)).and_then(Option::as_ref)
    }

    /// Whether this plane and `other` hold the very same tile at (tx, ty) (both empty counts).
    pub fn same_tile(&self, other: &Self, tx: u32, ty: u32) -> bool {
        if self.width != other.width || self.height != other.height || self.fill != other.fill {
            return false;
        }
        match (self.tile_arc(tx, ty), other.tile_arc(tx, ty)) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// A tile to write to: made (filled) if empty, copied first if shared.
    pub fn tile_mut(&mut self, tx: u32, ty: u32) -> &mut [u8] {
        let fill = self.fill;
        let i = self.index(tx, ty);
        let slot = &mut self.tiles[i];
        let arc = slot.get_or_insert_with(|| {
            let mut v = vec![0u8; (TILE * TILE) as usize * C];
            if fill.iter().any(|b| *b != 0) {
                for px in v.chunks_exact_mut(C) {
                    px.copy_from_slice(&fill);
                }
            }
            Arc::new(v)
        });
        Arc::make_mut(arc).as_mut_slice()
    }

    /// Empties a tile (it reads as `fill` again).
    pub fn clear_tile(&mut self, tx: u32, ty: u32) {
        let i = self.index(tx, ty);
        self.tiles[i] = None;
    }

    /// Every tile becomes `fill`.
    pub fn clear(&mut self) {
        self.tiles.iter_mut().for_each(|t| *t = None);
    }

    /// One pixel; outside the plane, `fill`.
    pub fn get(&self, x: i32, y: i32) -> [u8; C] {
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
            return self.fill;
        }
        let (x, y) = (x as u32, y as u32);
        match self.tile(x / TILE, y / TILE) {
            Some(t) => {
                let o = (((y % TILE) * TILE + x % TILE) as usize) * C;
                let mut p = [0u8; C];
                p.copy_from_slice(&t[o..o + C]);
                p
            }
            None => self.fill,
        }
    }

    /// Sets one pixel (ignored outside the plane).
    pub fn set(&mut self, x: i32, y: i32, v: [u8; C]) {
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
            return;
        }
        let (x, y) = (x as u32, y as u32);
        let t = self.tile_mut(x / TILE, y / TILE);
        let o = (((y % TILE) * TILE + x % TILE) as usize) * C;
        t[o..o + C].copy_from_slice(&v);
    }

    /// The pixels of `r` (clipped to nothing: outside reads as `fill`), rows of `r.w`.
    pub fn read_rect(&self, r: Rect) -> Vec<u8> {
        let mut out = vec![0u8; (r.w * r.h) as usize * C];
        for px in out.chunks_exact_mut(C) {
            px.copy_from_slice(&self.fill);
        }
        let inside = r.intersect(&self.bounds());
        if inside.is_empty() {
            return out;
        }
        let (tx0, ty0) = (inside.x as u32 / TILE, inside.y as u32 / TILE);
        let (tx1, ty1) = ((inside.right() as u32 - 1) / TILE, (inside.bottom() as u32 - 1) / TILE);
        for ty in ty0..=ty1 {
            for tx in tx0..=tx1 {
                let Some(tile) = self.tile(tx, ty) else { continue };
                let part = self.tile_rect(tx, ty).intersect(&inside);
                for y in part.y..part.bottom() {
                    let src = (((y as u32 % TILE) * TILE + part.x as u32 % TILE) as usize) * C;
                    let dst = (((y - r.y) as u32 * r.w + (part.x - r.x) as u32) as usize) * C;
                    let n = part.w as usize * C;
                    out[dst..dst + n].copy_from_slice(&tile[src..src + n]);
                }
            }
        }
        out
    }

    /// Writes `data` (rows of `r.w`) into `r`; what falls outside the plane is dropped.
    pub fn write_rect(&mut self, r: Rect, data: &[u8]) {
        assert_eq!(data.len(), (r.w * r.h) as usize * C, "write_rect: data doesn't match the rectangle");
        let inside = r.intersect(&self.bounds());
        if inside.is_empty() {
            return;
        }
        let (tx0, ty0) = (inside.x as u32 / TILE, inside.y as u32 / TILE);
        let (tx1, ty1) = ((inside.right() as u32 - 1) / TILE, (inside.bottom() as u32 - 1) / TILE);
        for ty in ty0..=ty1 {
            for tx in tx0..=tx1 {
                let part = self.tile_rect(tx, ty).intersect(&inside);
                let tile = self.tile_mut(tx, ty);
                for y in part.y..part.bottom() {
                    let dst = (((y as u32 % TILE) * TILE + part.x as u32 % TILE) as usize) * C;
                    let src = (((y - r.y) as u32 * r.w + (part.x - r.x) as u32) as usize) * C;
                    let n = part.w as usize * C;
                    tile[dst..dst + n].copy_from_slice(&data[src..src + n]);
                }
            }
        }
    }

    /// The whole plane as one buffer (rows of `width`).
    pub fn to_vec(&self) -> Vec<u8> {
        self.read_rect(self.bounds())
    }

    /// A plane from one buffer (rows of `width`); tiles that are all `fill` stay empty.
    pub fn from_vec(width: u32, height: u32, fill: [u8; C], data: &[u8]) -> Self {
        let mut p = Self::new(width, height, fill);
        p.write_rect(Rect::new(0, 0, width, height), data);
        p.compact();
        p
    }

    /// Drops tiles that hold nothing but `fill` (after an erase, a clear).
    pub fn compact(&mut self) {
        let fill = self.fill;
        for t in self.tiles.iter_mut() {
            if t.as_ref().is_some_and(|v| v.chunks_exact(C).all(|p| p == fill)) {
                *t = None;
            }
        }
    }

    /// How many tiles hold pixels.
    pub fn used_tiles(&self) -> usize {
        self.tiles.iter().filter(|t| t.is_some()).count()
    }

    /// The smallest rectangle holding every pixel that isn't `fill` (in whole tiles, then
    /// tightened to pixels), or `None` if the plane is empty.
    pub fn content_bounds(&self) -> Option<Rect> {
        let (gx, gy) = self.grid();
        let mut r = Rect::default();
        for ty in 0..gy {
            for tx in 0..gx {
                let Some(tile) = self.tile(tx, ty) else { continue };
                let tr = self.tile_rect(tx, ty);
                let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
                for y in 0..tr.h {
                    for x in 0..tr.w {
                        let o = ((y * TILE + x) as usize) * C;
                        if tile[o..o + C] != self.fill {
                            x0 = x0.min(x);
                            y0 = y0.min(y);
                            x1 = x1.max(x + 1);
                            y1 = y1.max(y + 1);
                        }
                    }
                }
                if x1 > 0 {
                    r = r.union(&Rect::new(tr.x + x0 as i32, tr.y + y0 as i32, x1 - x0, y1 - y0));
                }
            }
        }
        (!r.is_empty()).then_some(r)
    }

    /// Same size and pixels (reads every pixel when tiles differ).
    pub fn same_pixels(&self, other: &Self) -> bool {
        if self.width != other.width || self.height != other.height {
            return false;
        }
        let (gx, gy) = self.grid();
        (0..gy).all(|ty| (0..gx).all(|tx| self.same_tile(other, tx, ty) || self.read_rect(self.tile_rect(tx, ty)) == other.read_rect(other.tile_rect(tx, ty))))
    }

    /// Bytes held by tiles (shared tiles counted in full).
    pub fn bytes(&self) -> usize {
        self.used_tiles() * (TILE * TILE) as usize * C
    }

    /// Calls `f` for every tile that holds pixels, in parallel-friendly order (row by row).
    pub fn tiles_with_pixels(&self) -> Vec<(u32, u32)> {
        let (gx, gy) = self.grid();
        (0..gy).flat_map(|ty| (0..gx).map(move |tx| (tx, ty))).filter(|(tx, ty)| self.tile(*tx, *ty).is_some()).collect()
    }

    /// Takes a tile's buffer out to work on it (made if empty, copied if shared); put it back
    /// with [`Plane::put_tile`]. Lets callers work on many tiles in parallel.
    pub fn take_tile(&mut self, tx: u32, ty: u32) -> Vec<u8> {
        self.tile_mut(tx, ty);
        let i = self.index(tx, ty);
        let arc = self.tiles[i].take().expect("tile was just made");
        Arc::try_unwrap(arc).unwrap_or_else(|a| (*a).clone())
    }

    pub fn put_tile(&mut self, tx: u32, ty: u32, data: Vec<u8>) {
        debug_assert_eq!(data.len(), (TILE * TILE) as usize * C);
        let i = self.index(tx, ty);
        self.tiles[i] = Some(Arc::new(data));
    }

    /// The tile list, for parallel writers ([`Plane::set_tiles`] puts it back).
    pub fn into_tiles(self) -> (u32, u32, [u8; C], Vec<Option<Arc<Vec<u8>>>>) {
        (self.width, self.height, self.fill, self.tiles)
    }

    pub fn from_tiles(width: u32, height: u32, fill: [u8; C], tiles: Vec<Option<Arc<Vec<u8>>>>) -> Self {
        assert_eq!(tiles.len(), (width.div_ceil(TILE) * height.div_ceil(TILE)) as usize);
        Self { width, height, fill, tiles }
    }
}

impl Raster {
    /// A transparent raster.
    pub fn transparent(width: u32, height: u32) -> Self {
        Self::new(width, height, [0; 4])
    }

    /// From straight-alpha RGBA rows.
    pub fn from_rgba(width: u32, height: u32, data: &[u8]) -> Self {
        Self::from_vec(width, height, [0; 4], data)
    }

    /// A raster of one colour.
    pub fn solid(width: u32, height: u32, rgba: [u8; 4]) -> Self {
        let mut r = Self::new(width, height, rgba);
        // Stored as tiles so the fill survives a round trip through PNG and edits treat it as
        // pixels; a solid plane with an opaque fill would never read as empty.
        let (gx, gy) = r.grid();
        for ty in 0..gy {
            for tx in 0..gx {
                r.tile_mut(tx, ty);
            }
        }
        r.fill = [0; 4];
        r
    }
}

impl Mask {
    /// A mask where every pixel is `v` (255 shows everything).
    pub fn filled(width: u32, height: u32, v: u8) -> Self {
        Self::new(width, height, [v])
    }

    /// The value at a pixel, 0..1.
    pub fn at(&self, x: i32, y: i32) -> f32 {
        self.get(x, y)[0] as f32 / 255.0
    }

    /// Every pixel inverted.
    pub fn inverted(&self) -> Self {
        let mut m = self.clone();
        m.fill = [255 - self.fill[0]];
        let (gx, gy) = m.grid();
        for ty in 0..gy {
            for tx in 0..gx {
                if self.tile(tx, ty).is_some() {
                    for v in m.tile_mut(tx, ty) {
                        *v = 255 - *v;
                    }
                }
            }
        }
        m
    }

    /// Whether every pixel is 0.
    pub fn is_empty(&self) -> bool {
        self.fill[0] == 0 && self.content_bounds().is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_round_trip_across_tiles() {
        let mut r = Raster::transparent(600, 300);
        let data: Vec<u8> = (0..(100 * 60 * 4)).map(|i| (i % 251) as u8).collect();
        let rect = Rect::new(220, 230, 100, 60);
        r.write_rect(rect, &data);
        assert_eq!(r.read_rect(rect), data);
        // Untouched tiles stay empty, touched ones exist.
        assert!(r.tile(2, 0).is_none());
        assert!(r.tile(0, 0).is_some() && r.tile(1, 1).is_some());
        // Outside reads as transparent.
        assert_eq!(r.get(-1, 0), [0; 4]);
        assert_eq!(r.get(599, 299), [0; 4]);
    }

    #[test]
    fn clones_share_tiles_until_written() {
        let mut a = Raster::solid(512, 512, [10, 20, 30, 255]);
        let b = a.clone();
        assert!(a.same_tile(&b, 0, 0));
        a.set(5, 5, [1, 2, 3, 4]);
        assert!(!a.same_tile(&b, 0, 0));
        assert!(a.same_tile(&b, 1, 1));
        assert_eq!(b.get(5, 5), [10, 20, 30, 255]);
        assert_eq!(a.get(5, 5), [1, 2, 3, 4]);
    }

    #[test]
    fn content_bounds_and_compact() {
        let mut r = Raster::transparent(1000, 700);
        assert_eq!(r.content_bounds(), None);
        r.set(300, 400, [255, 0, 0, 255]);
        r.set(310, 405, [255, 0, 0, 255]);
        assert_eq!(r.content_bounds(), Some(Rect::new(300, 400, 11, 6)));
        r.set(300, 400, [0; 4]);
        r.set(310, 405, [0; 4]);
        r.compact();
        assert_eq!(r.used_tiles(), 0);
    }

    #[test]
    fn masks_invert_with_their_fill() {
        let mut m = Mask::filled(300, 300, 0);
        m.set(1, 1, [200]);
        let inv = m.inverted();
        assert_eq!(inv.get(1, 1), [55]);
        assert_eq!(inv.get(299, 299), [255]);
        assert!(!m.is_empty());
        assert!(Mask::filled(10, 10, 0).is_empty());
    }

    #[test]
    fn rect_math() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(5, 5, 10, 10);
        assert_eq!(a.intersect(&b), Rect::new(5, 5, 5, 5));
        assert_eq!(a.union(&b), Rect::new(0, 0, 15, 15));
        assert!(a.intersect(&Rect::new(20, 20, 1, 1)).is_empty());
    }
}
