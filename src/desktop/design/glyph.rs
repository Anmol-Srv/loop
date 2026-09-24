//! Painted glyphs.
//!
//! Nunito has no tick, cross, lock, caret or evidence marks, and the icon
//! font's thin cut is a hairline at these sizes. Each one here is a few
//! strokes on a painter, centred on a point and sized by one number, so a
//! glyph sits on the same centre line as the text beside it wherever it is.

use egui::{pos2, vec2, Color32, Painter, Pos2, Rect, Stroke};

fn stroke(size: f32, ink: Color32) -> Stroke {
    Stroke::new((size / 8.0).clamp(1.2, 2.0), ink)
}

/// A tick.
pub fn tick(p: &Painter, c: Pos2, size: f32, ink: Color32) {
    let s = size / 2.0;
    let elbow = pos2(c.x - s * 0.18, c.y + s * 0.5);
    p.line_segment([pos2(c.x - s * 0.75, c.y + s * 0.02), elbow], stroke(size, ink));
    p.line_segment([elbow, pos2(c.x + s * 0.8, c.y - s * 0.62)], stroke(size, ink));
}

/// A cross.
pub fn cross(p: &Painter, c: Pos2, size: f32, ink: Color32) {
    let s = size * 0.32;
    p.line_segment([c + vec2(-s, -s), c + vec2(s, s)], stroke(size, ink));
    p.line_segment([c + vec2(-s, s), c + vec2(s, -s)], stroke(size, ink));
}

/// A short rule: "not reported".
pub fn dash(p: &Painter, c: Pos2, size: f32, ink: Color32) {
    p.line_segment([c + vec2(-size * 0.3, 0.0), c + vec2(size * 0.3, 0.0)], stroke(size, ink));
}

/// A padlock: a shackle over a body.
pub fn lock(p: &Painter, c: Pos2, size: f32, ink: Color32) {
    let body = Rect::from_center_size(c + vec2(0.0, size * 0.16), vec2(size * 0.7, size * 0.5));
    p.rect_filled(body, size * 0.1, ink);
    let r = size * 0.22;
    let top = body.top();
    let st = Stroke::new((size / 9.0).max(1.1), ink);
    let arc: Vec<Pos2> = (0..=8)
        .map(|i| {
            let a = std::f32::consts::PI * (1.0 + i as f32 / 8.0);
            pos2(c.x + r * a.cos(), top - size * 0.08 + r * a.sin())
        })
        .collect();
    p.line_segment([pos2(c.x - r, top), pos2(c.x - r, top - size * 0.08)], st);
    p.line_segment([pos2(c.x + r, top), pos2(c.x + r, top - size * 0.08)], st);
    p.add(egui::Shape::line(arc, st));
}

/// A disclosure caret: pointing right when closed, down when open. `open` is
/// 0→1 so the turn can be eased.
pub fn caret(p: &Painter, c: Pos2, size: f32, open: f32, ink: Color32) {
    let a = open.clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2;
    let (sin, cos) = a.sin_cos();
    let rot = |v: egui::Vec2| c + vec2(v.x * cos - v.y * sin, v.x * sin + v.y * cos);
    let s = size * 0.3;
    p.add(egui::Shape::convex_polygon(
        vec![rot(vec2(-s * 0.6, -s)), rot(vec2(s * 0.8, 0.0)), rot(vec2(-s * 0.6, s))],
        ink,
        Stroke::NONE,
    ));
}

/// An arrow up: something handed in.
pub fn arrow_up(p: &Painter, c: Pos2, size: f32, ink: Color32) {
    let s = size * 0.32;
    let st = stroke(size, ink);
    p.line_segment([c + vec2(0.0, s), c + vec2(0.0, -s)], st);
    p.line_segment([c + vec2(-s * 0.8, -s * 0.2), c + vec2(0.0, -s)], st);
    p.line_segment([c + vec2(s * 0.8, -s * 0.2), c + vec2(0.0, -s)], st);
}

/// A hooked arrow back: sent back with changes.
pub fn back(p: &Painter, c: Pos2, size: f32, ink: Color32) {
    let s = size * 0.32;
    let st = stroke(size, ink);
    let pts: Vec<Pos2> = (0..=8)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 8.0;
            c + vec2(s * 0.3 + s * 0.7 * a.cos(), s * 0.7 * a.sin() - s * 0.1)
        })
        .collect();
    let (start, end) = (pts[0], pts[8]);
    p.add(egui::Shape::line(pts, st));
    // The top of the hook runs back left; the bottom ends in the head.
    p.line_segment([start, start + vec2(-s * 0.6, 0.0)], st);
    p.line_segment([end, end + vec2(-s * 1.1, 0.0)], st);
    p.line_segment([end + vec2(-s * 1.1, 0.0), end + vec2(-s * 0.6, -s * 0.45)], st);
    p.line_segment([end + vec2(-s * 1.1, 0.0), end + vec2(-s * 0.6, s * 0.45)], st);
}

/// Evidence marks, one per artifact kind.
pub fn evidence(p: &Painter, c: Pos2, size: f32, kind: &str, ink: Color32) {
    let st = Stroke::new((size / 10.0).max(1.1), ink);
    let r = size * 0.13;
    match kind {
        // A pull request: two commits on the left joined, a branch on the right.
        "pr" => {
            let (a, b) = (c + vec2(-size * 0.22, -size * 0.3), c + vec2(-size * 0.22, size * 0.3));
            let d = c + vec2(size * 0.24, size * 0.3);
            p.circle_stroke(a, r, st);
            p.circle_stroke(b, r, st);
            p.circle_stroke(d, r, st);
            p.line_segment([a + vec2(0.0, r), b - vec2(0.0, r)], st);
            p.line_segment([d - vec2(0.0, r), c + vec2(size * 0.24, -size * 0.18)], st);
            p.line_segment([c + vec2(size * 0.24, -size * 0.18), c + vec2(size * 0.06, -size * 0.3)], st);
        }
        // A commit: a ring on a line.
        "commit" => {
            p.circle_stroke(c, size * 0.18, st);
            p.line_segment([c + vec2(-size * 0.42, 0.0), c + vec2(-size * 0.18, 0.0)], st);
            p.line_segment([c + vec2(size * 0.18, 0.0), c + vec2(size * 0.42, 0.0)], st);
        }
        // Figma: the stacked frame, reduced to a square and a circle.
        "figma" => {
            let sq = size * 0.3;
            p.rect_stroke(Rect::from_center_size(c + vec2(-sq * 0.5, -sq * 0.5), vec2(sq, sq)), sq * 0.3, st, egui::StrokeKind::Middle);
            p.rect_stroke(Rect::from_center_size(c + vec2(-sq * 0.5, sq * 0.5), vec2(sq, sq)), sq * 0.3, st, egui::StrokeKind::Middle);
            p.circle_stroke(c + vec2(sq * 0.55, 0.0), sq * 0.45, st);
        }
        // A page with a turned corner.
        "doc" => {
            let rect = Rect::from_center_size(c, vec2(size * 0.52, size * 0.66));
            p.rect_stroke(rect, 1.5, st, egui::StrokeKind::Middle);
            for i in 0..2 {
                let y = rect.top() + rect.height() * (0.4 + i as f32 * 0.25);
                p.line_segment([pos2(rect.left() + 2.0, y), pos2(rect.right() - 2.0, y)], st);
            }
        }
        // A link: two rings, overlapped.
        _ => {
            p.circle_stroke(c + vec2(-size * 0.12, size * 0.08), size * 0.17, st);
            p.circle_stroke(c + vec2(size * 0.12, -size * 0.08), size * 0.17, st);
        }
    }
}
