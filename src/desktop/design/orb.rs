//! The agent's globe: a dotted sphere painted in 2D, in the spirit of
//! libraries.dev's "thinking orbs" — recreated by painting in egui rather than
//! importing a canvas library we can't use here.
//!
//! Dots sit on a Fibonacci lattice (the golden-angle spiral: the one even
//! spread with no visible pole or seam), rotate, and project straight onto the
//! disc — `x, y` become the screen offset, `z` becomes depth. Depth alone
//! drives size and brightness, so a far dot is smaller and dimmer with no real
//! perspective divide to compute. The geometry (`project`) takes no painter
//! and is what the test below checks; painting is a thin loop over its output.
//!
//! No glow, no bloom: PRODUCT.md's anti-references rule out the neon-orb look
//! this is otherwise easy to reach for. `hairline` is a 1px edge and a 5% fill,
//! nothing brighter.

use std::f32::consts::TAU;

use egui::{vec2, Color32, Painter, Pos2, Stroke, Vec2};

/// Dots sit inside the box, leaving room for the hairline and, on
/// `NeedsInput`, the amber dot at the corner.
pub const INSET: f32 = 0.86;

/// A lattice point after rotation: its screen offset from the globe's centre
/// (unit disc, `[-1, 1]`), its depth (`-1` far, `1` near), its position along
/// the gradient (latitude, fixed to the point rather than the camera), and its
/// longitude before rotation — the meridian sweep lights up whichever dots
/// currently sit near a fixed longitude, so turning the sphere sweeps it.
#[derive(Clone, Copy, Debug)]
pub struct Dot {
    pub offset: Vec2,
    pub depth: f32,
    pub t: f32,
    pub lon: f32,
}

struct Point {
    x: f32,
    y: f32,
    z: f32,
    lon: f32,
}

/// `count` points spread evenly over the unit sphere.
fn lattice(count: usize) -> Vec<Point> {
    let golden_angle = std::f32::consts::PI * (3.0 - 5.0_f32.sqrt());
    let n = count.max(1) as f32;
    (0..count)
        .map(|i| {
            let y = 1.0 - 2.0 * (i as f32 + 0.5) / n;
            let r = (1.0 - y * y).max(0.0).sqrt();
            let lon = (golden_angle * i as f32).rem_euclid(TAU);
            Point { x: r * lon.cos(), y, z: r * lon.sin(), lon }
        })
        .collect()
}

/// The lattice rotated by `spin` (around the vertical axis — the sphere's own
/// turning) and tilted by `tilt` (around the horizontal — a fixed viewing
/// angle so the globe reads as a sphere rather than a flat disc), then
/// projected onto the screen. Pure: no painter, so this is what the test
/// below exercises directly.
pub fn project(count: usize, spin: f32, tilt: f32) -> Vec<Dot> {
    let (sy, cy) = spin.sin_cos();
    let (sx, cx) = tilt.sin_cos();
    lattice(count)
        .into_iter()
        .map(|p| {
            let x1 = p.x * cy + p.z * sy;
            let z1 = p.z * cy - p.x * sy;
            let y2 = p.y * cx - z1 * sx;
            let z2 = p.y * sx + z1 * cx;
            Dot { offset: vec2(x1, y2), depth: z2, t: (p.y + 1.0) * 0.5, lon: p.lon }
        })
        .collect()
}

/// How many dots a globe this size carries. Tuned anchors: ~24 at 16px, ~60 at
/// 28px, ~140 at 56px — fewer, larger dots at the small sizes so they stay
/// crisp rather than turning to static.
pub fn dot_count(side: f32) -> usize {
    let n = if side <= 16.0 {
        24.0
    } else if side <= 28.0 {
        24.0 + (side - 16.0) / (28.0 - 16.0) * (60.0 - 24.0)
    } else if side <= 56.0 {
        60.0 + (side - 28.0) / (56.0 - 28.0) * (140.0 - 60.0)
    } else {
        (140.0 + (side - 56.0) * 1.5).min(200.0)
    };
    n.round() as usize
}

/// Depth eases size and brightness together rather than linearly, so the near
/// hemisphere doesn't dominate.
fn depth_scale(depth: f32) -> f32 {
    0.7 + 0.3 * (depth * 0.5 + 0.5)
}

fn depth_alpha(depth: f32) -> f32 {
    (0.3 + 0.7 * (depth * 0.5 + 0.5)).clamp(0.0, 1.0)
}

/// A dot's radius at this globe size and depth. At XS/SM the far side is
/// still clamped to stay legible rather than fading to sub-pixel static.
fn dot_radius(side: f32, depth: f32) -> f32 {
    let r = side * 0.06 * depth_scale(depth);
    if side <= 20.0 {
        r.max(1.2)
    } else {
        r.max(0.9)
    }
}

fn angle_dist(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(TAU);
    d.min(TAU - d)
}

/// How wide, in longitude, the meridian sweep's lit band is.
const SWEEP_WIDTH: f32 = 0.9;

/// The dotted sphere itself: `colours` blend across latitude (the primary
/// tint at one pole, the secondary at the other), depth sets size and
/// brightness, and `highlight_lon`, when given, lights whichever dots are
/// near that longitude right now — the `searching`/`waiting` scan.
pub fn paint(
    p: &Painter,
    center: Pos2,
    radius: f32,
    side: f32,
    colours: (Color32, Color32),
    spin: f32,
    tilt: f32,
    highlight_lon: Option<f32>,
) {
    let mut dots = project(dot_count(side), spin, tilt);
    // Painter's algorithm: farthest first, so the near hemisphere draws on top.
    dots.sort_by(|a, b| a.depth.total_cmp(&b.depth));
    for d in dots {
        let mut alpha = depth_alpha(d.depth);
        let mut r = dot_radius(side, d.depth);
        if let Some(lon) = highlight_lon {
            let dist = angle_dist(d.lon, lon);
            if dist < SWEEP_WIDTH {
                let k = 1.0 - dist / SWEEP_WIDTH;
                alpha = (alpha + k * 0.6).min(1.0);
                r *= 1.0 + k * 0.6;
            }
        }
        let colour = colours.0.lerp_to_gamma(colours.1, d.t).gamma_multiply(alpha);
        p.circle_filled(center + d.offset * radius, r, colour);
    }
}

/// A faint edge and a very soft inner tint — enough to read as a sphere's
/// silhouette, none of the bloom PRODUCT.md's anti-references rule out.
pub fn hairline(p: &Painter, center: Pos2, radius: f32, tint: Color32) {
    p.circle_filled(center, radius, tint.gamma_multiply(0.05));
    p.circle_stroke(center, radius, Stroke::new(1.0, tint.gamma_multiply(0.12)));
}

/// `working`'s particles: a couple of points tracing tilted orbits just past
/// the sphere's edge — the one bit of the globe that leaves its surface, so
/// "something is moving around this" reads instantly.
pub fn orbit(p: &Painter, center: Pos2, radius: f32, side: f32, colour: Color32, t: f32) {
    // A tilted ellipse just past the sphere's own edge — wide enough on one
    // axis to clear it, close enough on the other to graze it at the pass, so
    // the particle reads as orbiting the globe rather than floating loose
    // beside it. Kept close to `side` so it stays inside the avatar's own
    // footprint instead of bleeding into whatever sits next to it in a row.
    let (rx, ry) = (radius * 1.25, radius * 1.05);
    for i in 0..2 {
        let phase = (t + i as f32 * 0.5).rem_euclid(1.0) * TAU;
        let (s, c) = phase.sin_cos();
        // Which half of the tilted loop reads as nearer, for a light touch of
        // depth — the pass close to the sphere already sells the orbit.
        let depth = s;
        let pos = center + vec2(c * rx, s * ry);
        let r = (side * 0.05).max(1.2) * (0.8 + 0.2 * depth_scale(depth));
        p.circle_filled(pos, r, colour.gamma_multiply(0.7 + 0.3 * depth_alpha(depth)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_keeps_count_and_orders_depth_by_size() {
        let dots = project(48, 0.7, 0.4);
        assert_eq!(dots.len(), 48);
        for d in &dots {
            assert!((-1.01..=1.01).contains(&d.depth), "depth out of range: {}", d.depth);
            assert!((0.0..=1.0).contains(&d.t), "gradient position out of range: {}", d.t);
            assert!(d.offset.length() <= 1.01, "offset left the unit disc: {:?}", d.offset);
        }
        let nearest = dots.iter().copied().max_by(|a, b| a.depth.total_cmp(&b.depth)).unwrap();
        let farthest = dots.iter().copied().min_by(|a, b| a.depth.total_cmp(&b.depth)).unwrap();
        assert!(
            dot_radius(56.0, nearest.depth) > dot_radius(56.0, farthest.depth),
            "the nearest dot should paint larger than the farthest"
        );
    }
}
