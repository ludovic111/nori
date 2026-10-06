//! The document: one or more pages (artboards), each a stack of layers (top first), master
//! pages, paragraph and character styles, the active page and layer, and the selection.
//!
//! Everything is measured in document pixels at `dpi` (72 for screen work, 300 for print): a
//! raster layer's pixel is one document pixel; vectors and text use the same units as `f32`
//! and stay sharp at any output resolution. PDF export turns pixels into points with `dpi`.

use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::layer::{Content, Layer, TextAlign};
use crate::raster::{Mask, Raster, Rect};

/// The `.nori` format version written in `document.json`.
pub const FORMAT: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Margins {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Guide {
    /// A vertical guide at x = `at`, else horizontal at y = `at`.
    pub vertical: bool,
    pub at: f32,
}

/// A page (in a layout) or an artboard (in a design): its own size, layers and layout grid.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// Where it sits on the pasteboard (artboards side by side), in document pixels.
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    /// Top layer first.
    pub layers: Vec<Layer>,
    /// The master page drawn under this page's layers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub master: Option<String>,
    #[serde(default)]
    pub margins: Margins,
    #[serde(default = "one_column")]
    pub columns: u32,
    #[serde(default)]
    pub gutter: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guides: Vec<Guide>,
    /// Printed past the trim on every side, for PDF export.
    #[serde(default)]
    pub bleed: f32,
}

fn one_column() -> u32 {
    1
}

impl Page {
    pub fn new(id: impl Into<String>, name: impl Into<String>, width: u32, height: u32) -> Self {
        Self { id: id.into(), name: name.into(), width, height, x: 0.0, y: 0.0, layers: vec![], master: None, margins: Margins::default(), columns: 1, gutter: 0.0, guides: vec![], bleed: 0.0 }
    }

    pub fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.width, self.height)
    }

    /// Every layer on the page, depth first, top first.
    pub fn all(&self) -> Vec<&Layer> {
        self.layers.iter().flat_map(Layer::walk).collect()
    }

    /// The column boxes inside the margins: `[x, y, w, h]` each.
    pub fn column_boxes(&self) -> Vec<[f32; 4]> {
        let m = self.margins;
        let n = self.columns.max(1) as f32;
        let inner_w = self.width as f32 - m.left - m.right;
        let w = ((inner_w - self.gutter * (n - 1.0)) / n).max(1.0);
        (0..self.columns.max(1)).map(|i| [m.left + i as f32 * (w + self.gutter), m.top, w, (self.height as f32 - m.top - m.bottom).max(1.0)]).collect()
    }
}

/// A paragraph style: how a whole text layer is set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ParagraphStyle {
    pub name: String,
    pub font: String,
    pub size: f32,
    pub weight: u16,
    pub italic: bool,
    pub color: Color,
    pub align: TextAlign,
    pub line_height: f32,
    pub letter_spacing: f32,
    pub paragraph_spacing: f32,
}

impl Default for ParagraphStyle {
    fn default() -> Self {
        Self { name: "Body".into(), font: "Manrope".into(), size: 32.0, weight: 400, italic: false, color: Color::BLACK, align: TextAlign::Left, line_height: 1.35, letter_spacing: 0.0, paragraph_spacing: 12.0 }
    }
}

/// A character style: what a run of words changes (only the fields it sets).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct CharacterStyle {
    pub name: String,
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Styles {
    pub paragraph: Vec<ParagraphStyle>,
    pub character: Vec<CharacterStyle>,
}

/// The units the window shows lengths in (the document is always in pixels).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Units {
    #[default]
    Px,
    Pt,
    Mm,
    In,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub format: u32,
    pub name: String,
    /// Pixels per inch (72 for screen, 300 for print): print sizes and PDF points come from it.
    #[serde(default = "default_dpi")]
    pub dpi: f32,
    pub pages: Vec<Page>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub masters: Vec<Page>,
    /// The page tools and commands act on by default.
    pub active_page: String,
    /// The layer tools and commands act on by default.
    #[serde(default)]
    pub active: Option<String>,
    /// What is selected on the active page, as a mask over it (255 fully selected); none
    /// selects everything.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "selection_size")]
    pub selection: Option<Mask>,
    #[serde(default)]
    pub styles: Styles,
    #[serde(default)]
    pub units: Units,
    /// The next number used for an id (`L<n>` layers, `P<n>` pages).
    #[serde(default = "one")]
    pub next_id: u32,
}

fn default_dpi() -> f32 {
    72.0
}

fn one() -> u32 {
    1
}

/// Where a page is: among the pages or the masters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PageRef {
    Page(usize),
    Master(usize),
}

/// Where a layer is: its page, and the indices from the page's top level down.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loc {
    pub page: PageRef,
    pub path: Vec<usize>,
}

impl Document {
    /// One page with one layer: `background` as pixels, or a transparent layer when `None`.
    pub fn new(name: impl Into<String>, width: u32, height: u32, background: Option<Color>) -> Self {
        let mut d = Self::empty(name, width, height);
        let (pixels, layer_name) = match background {
            Some(c) => (Raster::solid(width, height, c.to_u8()), "Background"),
            None => (Raster::transparent(width, height), "Layer 1"),
        };
        let id = d.new_id();
        d.pages[0].layers.push(Layer::new(id.clone(), layer_name, Content::Raster { x: 0, y: 0, pixels }));
        d.active = Some(id);
        d
    }

    /// One empty page (no layers).
    pub fn empty(name: impl Into<String>, width: u32, height: u32) -> Self {
        let mut d = Self {
            format: FORMAT,
            name: name.into(),
            dpi: 72.0,
            pages: vec![],
            masters: vec![],
            active_page: String::new(),
            active: None,
            selection: None,
            styles: Styles::default(),
            units: Units::Px,
            next_id: 1,
        };
        let pid = d.new_page_id();
        d.pages.push(Page::new(pid.clone(), "Page 1", width, height));
        d.active_page = pid;
        d
    }

    /// A document holding one picture (straight RGBA rows) as its background layer.
    pub fn from_rgba(name: impl Into<String>, width: u32, height: u32, rgba: &[u8]) -> Self {
        let mut d = Self::empty(name, width, height);
        let id = d.new_id();
        d.pages[0].layers.push(Layer::new(id.clone(), "Background", Content::Raster { x: 0, y: 0, pixels: Raster::from_rgba(width, height, rgba) }));
        d.active = Some(id);
        d
    }

    // ---- pages ------------------------------------------------------------------------------

    pub fn page_index(&self, id: &str) -> Option<usize> {
        self.pages.iter().position(|p| p.id == id)
    }

    pub fn master_index(&self, id: &str) -> Option<usize> {
        self.masters.iter().position(|p| p.id == id)
    }

    /// The active page's index among the pages (the first page if the active one is gone or
    /// is a master).
    pub fn active_index(&self) -> usize {
        self.page_index(&self.active_page).unwrap_or(0)
    }

    /// The active page: a page, or a master page being edited.
    pub fn active_ref(&self) -> PageRef {
        match (self.page_index(&self.active_page), self.master_index(&self.active_page)) {
            (Some(i), _) => PageRef::Page(i),
            (None, Some(i)) => PageRef::Master(i),
            _ => PageRef::Page(0),
        }
    }

    /// The active page (or master being edited).
    pub fn page(&self) -> &Page {
        match self.active_ref() {
            PageRef::Page(i) => &self.pages[i],
            PageRef::Master(i) => &self.masters[i],
        }
    }

    pub fn page_mut(&mut self) -> &mut Page {
        match self.active_ref() {
            PageRef::Page(i) => &mut self.pages[i],
            PageRef::Master(i) => &mut self.masters[i],
        }
    }

    pub fn page_at(&self, r: PageRef) -> Option<&Page> {
        match r {
            PageRef::Page(i) => self.pages.get(i),
            PageRef::Master(i) => self.masters.get(i),
        }
    }

    pub fn page_at_mut(&mut self, r: PageRef) -> Option<&mut Page> {
        match r {
            PageRef::Page(i) => self.pages.get_mut(i),
            PageRef::Master(i) => self.masters.get_mut(i),
        }
    }

    /// A page or master by id, or by name, or by number ("2" is the second page).
    pub fn resolve_page(&self, key: &str) -> Result<PageRef, String> {
        let key = key.trim();
        if let Some(i) = self.page_index(key) {
            return Ok(PageRef::Page(i));
        }
        if let Some(i) = self.master_index(key) {
            return Ok(PageRef::Master(i));
        }
        if let Ok(n) = key.parse::<usize>()
            && n >= 1
            && n <= self.pages.len()
        {
            return Ok(PageRef::Page(n - 1));
        }
        if let Some(i) = self.pages.iter().position(|p| p.name.eq_ignore_ascii_case(key)) {
            return Ok(PageRef::Page(i));
        }
        if let Some(i) = self.masters.iter().position(|p| p.name.eq_ignore_ascii_case(key)) {
            return Ok(PageRef::Master(i));
        }
        Err(format!("No page `{key}`. page.list shows them (ids, names, or numbers from 1)."))
    }

    /// The active page's size.
    pub fn width(&self) -> u32 {
        self.page().width
    }

    pub fn height(&self) -> u32 {
        self.page().height
    }

    /// The active page's bounds.
    pub fn bounds(&self) -> Rect {
        self.page().bounds()
    }

    /// A fresh layer id.
    pub fn new_id(&mut self) -> String {
        loop {
            let id = format!("L{}", self.next_id);
            self.next_id += 1;
            if self.path_of(&id).is_none() {
                return id;
            }
        }
    }

    /// A fresh page id.
    pub fn new_page_id(&mut self) -> String {
        loop {
            let id = format!("P{}", self.next_id);
            self.next_id += 1;
            if self.page_index(&id).is_none() && self.master_index(&id).is_none() {
                return id;
            }
        }
    }

    /// "Layer 3": the first "<base> n" no layer is called.
    pub fn new_name(&self, base: &str) -> String {
        let names: Vec<&str> = self.all().iter().map(|l| l.name.as_str()).collect();
        (1..).map(|n| format!("{base} {n}")).find(|n| !names.contains(&n.as_str())).unwrap_or_else(|| base.to_string())
    }

    // ---- layers -----------------------------------------------------------------------------

    /// Every layer on every page and master, depth first, top first.
    pub fn all(&self) -> Vec<&Layer> {
        self.pages.iter().chain(self.masters.iter()).flat_map(|p| p.all()).collect()
    }

    /// Where a layer is, by id.
    pub fn path_of(&self, id: &str) -> Option<Loc> {
        fn find(list: &[Layer], id: &str, prefix: &mut Vec<usize>) -> Option<Vec<usize>> {
            for (i, l) in list.iter().enumerate() {
                prefix.push(i);
                if l.id == id {
                    return Some(prefix.clone());
                }
                if let Some(p) = find(l.children(), id, prefix) {
                    return Some(p);
                }
                prefix.pop();
            }
            None
        }
        for (i, p) in self.pages.iter().enumerate() {
            if let Some(path) = find(&p.layers, id, &mut vec![]) {
                return Some(Loc { page: PageRef::Page(i), path });
            }
        }
        for (i, p) in self.masters.iter().enumerate() {
            if let Some(path) = find(&p.layers, id, &mut vec![]) {
                return Some(Loc { page: PageRef::Master(i), path });
            }
        }
        None
    }

    pub fn at(&self, loc: &Loc) -> Option<&Layer> {
        let (first, rest) = loc.path.split_first()?;
        let mut l = self.page_at(loc.page)?.layers.get(*first)?;
        for i in rest {
            l = l.children().get(*i)?;
        }
        Some(l)
    }

    pub fn at_mut(&mut self, loc: &Loc) -> Option<&mut Layer> {
        let (first, rest) = loc.path.split_first()?;
        let mut l = self.page_at_mut(loc.page)?.layers.get_mut(*first)?;
        for i in rest {
            l = l.children_mut()?.get_mut(*i)?;
        }
        Some(l)
    }

    /// The list a location's last index points into (the page's top level, or a group's).
    pub fn siblings_mut(&mut self, loc: &Loc) -> Option<&mut Vec<Layer>> {
        match loc.path.split_last() {
            Some((_, [])) => Some(&mut self.page_at_mut(loc.page)?.layers),
            Some((_, parent)) => self.at_mut(&Loc { page: loc.page, path: parent.to_vec() })?.children_mut(),
            None => None,
        }
    }

    pub fn layer(&self, id: &str) -> Option<&Layer> {
        self.at(&self.path_of(id)?)
    }

    pub fn layer_mut(&mut self, id: &str) -> Option<&mut Layer> {
        let p = self.path_of(id)?;
        self.at_mut(&p)
    }

    /// The page a layer is on.
    pub fn page_of(&self, id: &str) -> Option<&Page> {
        self.page_at(self.path_of(id)?.page)
    }

    /// A layer by id, or by name when one layer has it (ids win; names ignore case). Names are
    /// looked up on the active page first.
    pub fn resolve(&self, key: &str) -> Result<String, String> {
        let key = key.trim();
        if self.path_of(key).is_some() {
            return Ok(key.to_string());
        }
        let here: Vec<&Layer> = self.page().all().into_iter().filter(|l| l.name.eq_ignore_ascii_case(key)).collect();
        if here.len() == 1 {
            return Ok(here[0].id.clone());
        }
        let all = self.all();
        let named: Vec<&&Layer> = all.iter().filter(|l| l.name.eq_ignore_ascii_case(key)).collect();
        match named.len() {
            1 => Ok(named[0].id.clone()),
            0 => {
                let mut names: Vec<&str> = all.iter().map(|l| l.id.as_str()).collect();
                names.extend(all.iter().map(|l| l.name.as_str()));
                let hint = crate::closest(key, &names).map(|c| format!(" Did you mean `{c}`?")).unwrap_or_default();
                Err(format!("No layer `{key}`.{hint} layer.list shows them."))
            }
            n => Err(format!("{n} layers are called `{key}`: use its id ({}).", named.iter().map(|l| l.id.as_str()).collect::<Vec<_>>().join(", "))),
        }
    }

    /// The active layer's id, if it is still on the active page.
    pub fn active_id(&self) -> Option<String> {
        let here = self.active_ref();
        self.active.clone().filter(|id| self.path_of(id).is_some_and(|l| l.page == here))
    }

    /// Inserts `layer` above `above` (in its group, on its page), or on top of the active page.
    pub fn insert_above(&mut self, layer: Layer, above: Option<&str>) {
        if let Some(loc) = above.and_then(|id| self.path_of(id)) {
            let i = *loc.path.last().unwrap_or(&0);
            if let Some(list) = self.siblings_mut(&loc) {
                list.insert(i, layer);
                return;
            }
        }
        self.page_mut().layers.insert(0, layer);
    }

    /// Takes a layer out of the tree (threads through it are mended).
    pub fn remove(&mut self, id: &str) -> Option<Layer> {
        let loc = self.path_of(id)?;
        let i = *loc.path.last()?;
        let l = self.siblings_mut(&loc)?.remove(i);
        // A text frame in a thread: the frame before it now leads to the one after.
        let after = match &l.content {
            Content::Text { text } => text.next.clone(),
            _ => None,
        };
        let ids: Vec<String> = self.all().iter().map(|x| x.id.clone()).collect();
        for other in ids {
            if let Some(Layer { content: Content::Text { text }, .. }) = self.layer_mut(&other)
                && text.next.as_deref() == Some(id)
            {
                text.next = after.clone();
            }
        }
        if self.active.as_deref() == Some(id) || self.active.as_ref().is_some_and(|a| self.path_of(a).is_none()) {
            self.active = self.page().layers.first().map(|l| l.id.clone());
        }
        Some(l)
    }

    /// The text frames of the thread `id` is in, first frame first.
    pub fn thread_of(&self, id: &str) -> Vec<String> {
        let prev_of = |target: &str| -> Option<String> {
            self.all().iter().find(|l| matches!(&l.content, Content::Text { text } if text.next.as_deref() == Some(target))).map(|l| l.id.clone())
        };
        let mut head = id.to_string();
        let mut guard = 0;
        while let Some(p) = prev_of(&head) {
            head = p;
            guard += 1;
            if guard > 10_000 {
                break;
            }
        }
        let mut out = vec![head.clone()];
        let mut cur = head;
        while let Some(Layer { content: Content::Text { text }, .. }) = self.layer(&cur) {
            match &text.next {
                Some(n) if !out.contains(n) && self.layer(n).is_some() => {
                    out.push(n.clone());
                    cur = n.clone();
                }
                _ => break,
            }
        }
        out
    }

    /// The selection, or the whole active page.
    pub fn selection_or_all(&self) -> Mask {
        self.selection.clone().unwrap_or_else(|| Mask::filled(self.width(), self.height(), 255))
    }

    /// The selection's bounds, or the active page.
    pub fn selection_bounds(&self) -> Rect {
        match &self.selection {
            Some(m) if m.fill()[0] == 0 => m.content_bounds().unwrap_or_default(),
            _ => self.bounds(),
        }
    }

    /// Pixels of every raster layer and mask, in bytes (shared tiles counted in full).
    pub fn bytes(&self) -> usize {
        self.all()
            .iter()
            .map(|l| l.raster().map(|(_, _, p)| p.bytes()).unwrap_or(0) + l.mask.as_ref().map(|m| m.mask.bytes()).unwrap_or(0))
            .sum::<usize>()
            + self.selection.as_ref().map(Mask::bytes).unwrap_or(0)
    }

    pub fn paragraph_style(&self, name: &str) -> Option<&crate::document::ParagraphStyle> {
        self.styles.paragraph.iter().find(|s| s.name.eq_ignore_ascii_case(name))
    }

    pub fn character_style(&self, name: &str) -> Option<&CharacterStyle> {
        self.styles.character.iter().find(|s| s.name.eq_ignore_ascii_case(name))
    }
}

mod selection_size {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use crate::raster::Mask;

    #[derive(Serialize, Deserialize)]
    struct Size {
        width: u32,
        height: u32,
        #[serde(default)]
        fill: u8,
    }

    pub fn serialize<S: Serializer>(m: &Option<Mask>, s: S) -> Result<S::Ok, S::Error> {
        m.as_ref().map(|m| Size { width: m.width(), height: m.height(), fill: m.fill()[0] }).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Mask>, D::Error> {
        Ok(Option::<Size>::deserialize(d)?.map(|s| Mask::filled(s.width.clamp(1, crate::MAX_SIDE), s.height.clamp(1, crate::MAX_SIDE), s.fill)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::TextLayer;

    #[test]
    fn tree_lookups_and_names() {
        let mut d = Document::new("Test", 100, 80, Some(Color::WHITE));
        assert_eq!(d.pages.len(), 1);
        assert_eq!(d.page().layers.len(), 1);
        let first = d.active_id().unwrap();
        let id = d.new_id();
        let name = d.new_name("Layer");
        assert_eq!(name, "Layer 1");
        d.insert_above(Layer::new(id.clone(), name, Content::Raster { x: 0, y: 0, pixels: Raster::transparent(100, 80) }), Some(&first));
        assert_eq!(d.page().layers[0].id, id);
        let gid = d.new_id();
        let child = d.remove(&id).unwrap();
        d.insert_above(Layer::new(gid.clone(), "Group 1", Content::Group { children: vec![child], expanded: true }), None);
        assert_eq!(d.path_of(&id).unwrap().path, vec![0, 0]);
        assert_eq!(d.resolve("layer 1").unwrap(), id);
        assert_eq!(d.resolve("background").unwrap(), first);
        assert!(d.resolve("Backgrund").unwrap_err().contains("Did you mean `Background`"));
        assert_eq!(d.all().len(), 3);
        d.remove(&first);
        assert_eq!(d.active_id().as_deref(), Some(gid.as_str()));
    }

    #[test]
    fn pages_threads_and_columns() {
        let mut d = Document::empty("Booklet", 600, 800);
        let p2 = d.new_page_id();
        d.pages.push(Page::new(p2.clone(), "Page 2", 600, 800));
        assert_eq!(d.resolve_page("2").unwrap(), PageRef::Page(1));
        assert_eq!(d.resolve_page(&p2).unwrap(), PageRef::Page(1));
        let a = d.new_id();
        let b = d.new_id();
        let c = d.new_id();
        let frame = |next: Option<&str>| Content::Text { text: TextLayer { frame: Some([200.0, 300.0]), next: next.map(str::to_string), ..Default::default() } };
        d.pages[0].layers.push(Layer::new(a.clone(), "A", frame(Some(&b))));
        d.pages[1].layers.push(Layer::new(b.clone(), "B", frame(Some(&c))));
        d.pages[1].layers.push(Layer::new(c.clone(), "C", frame(None)));
        assert_eq!(d.thread_of(&c), vec![a.clone(), b.clone(), c.clone()]);
        d.remove(&b);
        assert_eq!(d.thread_of(&a), vec![a.clone(), c.clone()]);
        assert_eq!(d.page_of(&c).unwrap().id, p2);
        let mut p = Page::new("x", "x", 1000, 500);
        p.margins = Margins { top: 50.0, right: 50.0, bottom: 50.0, left: 50.0 };
        p.columns = 3;
        p.gutter = 20.0;
        let cols = p.column_boxes();
        assert_eq!(cols.len(), 3);
        assert!((cols[0][2] - 286.666).abs() < 0.01);
        assert!((cols[2][0] + cols[2][2] - 950.0).abs() < 0.01);
    }
}
