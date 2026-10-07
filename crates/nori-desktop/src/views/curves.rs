//! Interactive RGB/channel tone curves. Preview uses the renderer's exact interpolation.
use crate::store::{Store, StoreExt};
use crate::theme::{ActiveTheme, size as sz};
use crate::ui::Button;
use gpui::{Bounds, Context, Entity, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathBuilder, Pixels, Render, Window, canvas, div, point, prelude::*, px, size};
use nori_core::{Adjustment, Content};
use serde_json::json;

pub struct Curves {
    store: Entity<Store>,
    bounds: Bounds<Pixels>,
    layer: String,
    channel: usize,
    points: Vec<[f32; 2]>,
    dragging: Option<usize>,
    gesture: String,
}
impl Curves {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self { store: cx.store(), bounds: Bounds::default(), layer: String::new(), channel: 0, points: vec![[0., 0.], [255., 255.]], dragging: None, gesture: String::new() }
    }
    fn position(&self, p: gpui::Point<Pixels>) -> [f32; 2] {
        [
            ((f32::from(p.x - self.bounds.origin.x) / f32::from(self.bounds.size.width).max(1.)) * 255.).clamp(0., 255.),
            (255. - f32::from(p.y - self.bounds.origin.y) / f32::from(self.bounds.size.height).max(1.) * 255.).clamp(0., 255.),
        ]
    }
    fn commit(&self, cx: &mut Context<Self>) {
        let key = ["rgb", "red", "green", "blue"][self.channel];
        let params = json!({"layerId":self.layer,"settings":{key:self.points},"coalesce":self.gesture});
        self.store.update(cx, |s, cx| s.run("layer.setAdjustment", params, cx));
    }
    fn down(&mut self, e: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        let p = self.position(e.position);
        self.gesture = format!("curve-{}", uuid::Uuid::new_v4());
        let nearest = self.points.iter().enumerate().find(|(_, q)| (p[0] - q[0]).hypot(p[1] - q[1]) < 14.).map(|(i, _)| i);
        if e.click_count >= 2 {
            if let Some(i) = nearest
                && i > 0 && i + 1 < self.points.len() {
                    self.points.remove(i);
                    self.commit(cx);
                }
            self.dragging = None;
            cx.notify();
            return;
        }
        let i = match nearest {
            Some(i) => i,
            None => {
                let i = self.points.partition_point(|q| q[0] < p[0]);
                if i == 0 || i == self.points.len() || self.points.len() >= 32 {
                    return;
                }
                self.points.insert(i, p);
                i
            }
        };
        self.dragging = Some(i);
        self.commit(cx);
        cx.notify();
    }
    fn moved(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(i) = self.dragging else { return };
        let mut p = self.position(e.position);
        p[0] = if i == 0 {
            0.
        } else if i + 1 == self.points.len() {
            255.
        } else {
            p[0].clamp(self.points[i - 1][0] + 0.1, self.points[i + 1][0] - 0.1)
        };
        self.points[i] = p;
        self.commit(cx);
        cx.notify();
    }
    fn up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.dragging = None;
        cx.notify();
    }
}
impl Render for Curves {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.dragging.is_none() {
            let s = self.store.read(cx);
            if let Some(doc) = &s.doc
                && let Some(id) = doc.active_id()
                    && let Some(l) = doc.layer(&id)
                        && let Content::Adjustment { adjustment: Adjustment::Curves { rgb, red, green, blue } } = &l.content {
                            self.layer = id;
                            self.points = [rgb, red, green, blue][self.channel].clone();
                        }
        }
        let t = cx.theme().clone();
        let entity = cx.entity();
        let bounds_entity = entity.clone();
        let points = self.points.clone();
        let line = t.line;
        let accent = [t.accent, gpui::rgb(0xe16f75).into(), gpui::rgb(0x7cba85).into(), gpui::rgb(0x78a6dd).into()][self.channel];
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(div().flex().gap(px(4.)).children(["RGB", "Red", "Green", "Blue"].into_iter().enumerate().map(|(i, label)| {
                Button::new(("curve-channel", i), label).small().on_click(cx.listener(move |this, _, _, cx| {
                    this.channel = i;
                    this.dragging = None;
                    cx.notify();
                }))
            })))
            .child(
                div()
                    .id("curve-graph")
                    .relative()
                    .h(px(180.))
                    .w_full()
                    .bg(t.bg)
                    .cursor_crosshair()
                    .border_1()
                    .border_color(t.line)
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::down))
                    .on_mouse_move(cx.listener(Self::moved))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::up))
                    .when(self.dragging.is_some(), |d| d.child(crate::ui::drag::track(entity, |c, e, w, cx| c.moved(e, w, cx), |c, e, w, cx| c.up(e, w, cx))))
                    .child(
                        canvas(
                            move |bounds, _, cx| {
                                bounds_entity.update(cx, |this, _| this.bounds = bounds);
                            },
                            move |bounds, (), window, _| {
                                let map = |p: [f32; 2]| point(bounds.origin.x + bounds.size.width * (p[0] / 255.), bounds.origin.y + bounds.size.height * (1. - p[1] / 255.));
                                for i in 1..4 {
                                    let q = i as f32 * 255. / 4.;
                                    let mut grid = PathBuilder::stroke(px(1.));
                                    grid.move_to(map([q, 0.]));
                                    grid.line_to(map([q, 255.]));
                                    grid.move_to(map([0., q]));
                                    grid.line_to(map([255., q]));
                                    if let Ok(p) = grid.build() {
                                        window.paint_path(p, line);
                                    }
                                }
                                let curve = nori_render::adjust::curve_fn(&points);
                                let mut path = PathBuilder::stroke(px(2.));
                                for x in 0..=255 {
                                    let p = map([x as f32, curve(x as f32 / 255.) * 255.]);
                                    if x == 0 { path.move_to(p) } else { path.line_to(p) }
                                }
                                if let Ok(p) = path.build() {
                                    window.paint_path(p, accent);
                                }
                                for &p in &points {
                                    let p = map(p);
                                    window.paint_quad(gpui::fill(Bounds::new(point(p.x - px(3.), p.y - px(3.)), size(px(6.), px(6.))), accent));
                                }
                            },
                        )
                        .absolute()
                        .inset_0(),
                    ),
            )
            .child(div().text_size(px(sz::XS)).text_color(t.text_3).child("Click to add · drag to adjust · double-click a point to remove"))
    }
}
