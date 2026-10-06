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
/// `NeedsInput`, the rose dot at the corner.
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

impl Dot {
    /// Latitude in radians, `-PI/2` at the bottom pole to `PI/2` at the top,
    /// fixed to the point rather than the camera.
    pub fn lat(&self) -> f32 {
        (self.t * 2.0 - 1.0).clamp(-1.0, 1.0).asin()
    }
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

/// A dot's radius at this globe size and depth: a fraction of the gap
/// between neighbours on the lattice, not of the globe, so a big globe has
/// the same airy grain as a small one instead of dots packing into a ball.
/// At XS/SM the far side is still clamped to stay legible rather than fading
/// to sub-pixel static.
fn dot_radius(side: f32, depth: f32) -> f32 {
    let globe = side * 0.5 * INSET;
    let spacing = globe * (4.0 * std::f32::consts::PI / dot_count(side) as f32).sqrt();
    let r = spacing * 0.3 * depth_scale(depth);
    if side <= 20.0 {
        r.max(1.1)
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
/// near that longitude right now — a meridian scan.
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
    let dim = if highlight_lon.is_some() { PATTERN_DIM } else { 1.0 };
    paint_lit(p, center, radius, side, colours, spin, tilt, dim, |d| match highlight_lon {
        Some(lon) => (1.0 - angle_dist(d.lon, lon) / SWEEP_WIDTH).max(0.0),
        None => 0.0,
    });
}

/// How far an unlit dot steps back while a pattern plays, so the lit ones
/// carry it by contrast rather than by swelling into each other.
pub const PATTERN_DIM: f32 = 0.4;

/// The sphere with a per-dot `light` in `0..=1`: how much brighter a dot
/// draws on top of its depth shading, with `dim` the brightness of a dot the
/// pattern leaves dark (`1.0` for no pattern). Every state's pattern — a
/// meridian, a plait, a rolling wave — is one of these functions over the
/// dot's fixed latitude and longitude, so the pattern rides the sphere as it
/// turns instead of being painted over it.
#[allow(clippy::too_many_arguments)]
pub fn paint_lit(
    p: &Painter,
    center: Pos2,
    radius: f32,
    side: f32,
    colours: (Color32, Color32),
    spin: f32,
    tilt: f32,
    dim: f32,
    light: impl Fn(&Dot) -> f32,
) {
    let mut dots = project(dot_count(side), spin, tilt);
    // Painter's algorithm: farthest first, so the near hemisphere draws on top.
    dots.sort_by(|a, b| a.depth.total_cmp(&b.depth));
    for d in dots {
        let k = light(&d).clamp(0.0, 1.0);
        // Lit dots come up to full strength; only a touch larger, so bands
        // stay made of dots.
        let alpha = (depth_alpha(d.depth) * (dim + (1.0 - dim) * k) + k * 0.25).min(1.0);
        let r = dot_radius(side, d.depth) * (1.0 + k * 0.25);
        let colour = colours.0.lerp_to_gamma(colours.1, d.t).gamma_multiply(alpha);
        p.circle_filled(center + d.offset * radius, r, colour);
    }
}

/// `weaving` — planning: three strands plait around the sphere. Each strand
/// is a latitude that swings with longitude, a third of a turn out of phase
/// with the next, so where two cross they read as passing over and under as
/// the sphere turns. `phase` slides the strands along themselves.
pub fn plait(d: &Dot, phase: f32) -> f32 {
    const WIDTH: f32 = 0.2;
    (0..3)
        .map(|k| {
            let path = 0.55 * (2.0 * d.lon + phase + k as f32 * TAU / 3.0).sin();
            (1.0 - (d.lat() - path).abs() / WIDTH).max(0.0)
        })
        .fold(0.0, f32::max)
}

/// `listening` — waiting on you: a wave rolls down through the latitude
/// rings, pole to pole, and starts again. `phase` is 0→1 through one roll.
pub fn listen(d: &Dot, phase: f32) -> f32 {
    // Two crests on the sphere at once, soft-edged: a pulse, not a stripe.
    let wave = (d.t * 2.0 * TAU + phase * TAU).sin();
    (wave * 0.5 + 0.5).powi(3)
}

/// A faint edge and a very soft inner tint — enough to read as a sphere's
/// silhouette, none of the bloom PRODUCT.md's anti-references rule out.
pub fn hairline(p: &Painter, center: Pos2, radius: f32, tint: Color32) {
    p.circle_filled(center, radius, tint.gamma_multiply(0.05));
    p.circle_stroke(center, radius, Stroke::new(1.0, tint.gamma_multiply(0.12)));
}

/// A particle on one of `working`'s orbits, placed in 3D so it can pass
/// behind the globe: `(screen offset in radii, depth)`.
fn orbit_point(k: usize, t: f32, tilt: f32) -> (Vec2, f32) {
    // Three planes, each tipped a different way, a little wider than the
    // sphere so the particles clear it on the near pass.
    let (incline, node, rr) = [(0.9_f32, 0.0_f32, 1.22_f32), (0.9, TAU / 3.0, 1.16), (0.35, TAU * 2.0 / 3.0, 1.28)][k % 3];
    let a = t * TAU;
    let (x, z) = (a.cos() * rr, a.sin() * rr);
    // Incline the ring about x, then turn it to its node, then the view tilt.
    let (si, ci) = incline.sin_cos();
    let (y1, z1) = (-z * si, z * ci);
    let (sn, cn) = node.sin_cos();
    let (x2, z2) = (x * cn + z1 * sn, z1 * cn - x * sn);
    let (st, ct) = tilt.sin_cos();
    let (y3, z3) = (y1 * ct - z2 * st, y1 * st + z2 * ct);
    (vec2(x2, y3), z3)
}

/// `working` — particles on tilted orbits, each with a short fading trail.
/// `front` picks which half to draw: call once with `false` before the sphere
/// and once with `true` after it, and the particles pass behind the globe.
pub fn orbits(p: &Painter, center: Pos2, radius: f32, side: f32, colour: Color32, t: f32, tilt: f32, front: bool) {
    // Two orbits at the small sizes, where a third is only clutter.
    let count = if side < 24.0 { 2 } else { 3 };
    let trail = if side < 24.0 { 2 } else { 4 };
    for k in 0..count {
        // Different speeds per orbit, so the particles never line up.
        let speed = [1.0, 0.8, 1.25][k];
        for j in 0..trail {
            let tt = (t * speed + k as f32 / count as f32 - j as f32 * 0.035).rem_euclid(1.0);
            let (off, depth) = orbit_point(k, tt, tilt);
            if (depth >= 0.0) != front {
                continue;
            }
            let fade = 1.0 - j as f32 / trail as f32;
            let r = (side * 0.05).max(1.1) * (0.55 + 0.45 * fade) * (0.8 + 0.2 * depth_scale(depth));
            let a = depth_alpha(depth) * fade * if front { 1.0 } else { 0.5 };
            p.circle_filled(center + off * radius, r, colour.gamma_multiply(a));
        }
    }
}

/// `working`'s particles, kept for callers that draw them in one pass: every
/// particle in front of the sphere.
pub fn orbit(p: &Painter, center: Pos2, radius: f32, side: f32, colour: Color32, t: f32) {
    orbits(p, center, radius, side, colour, t, 0.42, true);
}

/// Which lattice dots make this cycle's constellation: six, zig-zagging
/// across the face that looked at the viewer when the cycle began (`front`
/// is that longitude). Chosen once per cycle from the lattice itself, so the
/// figure rides the sphere as it turns instead of re-picking every frame.
pub fn stars(dots: &[Dot], front: f32) -> Vec<usize> {
    let mut picked: Vec<usize> = Vec::with_capacity(6);
    for k in 0..6 {
        let lon = front + (k as f32 / 5.0 - 0.5) * 1.5;
        // Centred on the latitude the tilted view faces (screen y grows
        // downward, so the face is at a positive latitude), zig-zagging a
        // little either side of it.
        let lat = if k % 2 == 0 { 0.6 } else { 0.05 } + 0.1 * (k as f32 * 2.3).sin();
        let best = (0..dots.len())
            .filter(|i| !picked.contains(i))
            .min_by(|&a, &b| {
                let da = angle_dist(dots[a].lon, lon).powi(2) + (dots[a].lat() - lat).powi(2);
                let db = angle_dist(dots[b].lon, lon).powi(2) + (dots[b].lat() - lat).powi(2);
                da.total_cmp(&db)
            });
        if let Some(i) = best {
            picked.push(i);
        }
    }
    picked
}

/// `connecting` — first contact: a constellation wires itself across the
/// near face. `t` is 0→1 through one build; edges appear one after another,
/// a packet runs down the newest, then the figure fades and the next cycle
/// starts from a fresh set of stars. `dots` in lattice order (as `project`
/// returns them), `stars` from `stars`.
#[allow(clippy::too_many_arguments)]
pub fn constellation(
    p: &Painter,
    center: Pos2,
    radius: f32,
    side: f32,
    colour: Color32,
    dots: &[Dot],
    stars: &[usize],
    t: f32,
) {
    if stars.len() < 2 {
        return;
    }
    // Lifted toward the ink, so the figure reads over the dots in either
    // palette: lighter on the dark canvas, deeper on the light one.
    let colour = colour.lerp_to_gamma(super::tokens::colour::TEXT(), 0.35);
    let edges = stars.len() - 1;
    // Build over the first 80% of the cycle, then fade out.
    let build = (t / 0.8).min(1.0) * edges as f32;
    let fade = if t > 0.8 { 1.0 - (t - 0.8) / 0.2 } else { 1.0 };
    let width = (side / 30.0).clamp(0.9, 2.0);
    let at = |i: usize| center + dots[stars[i]].offset * radius;
    // A star turning away fades with it rather than drawing through the globe.
    let seen = |i: usize| (dots[stars[i]].depth * 2.0 + 0.6).clamp(0.0, 1.0);
    for i in 0..edges {
        let grown = (build - i as f32).clamp(0.0, 1.0);
        if grown <= 0.0 {
            break;
        }
        let (a, b) = (at(i), at(i + 1));
        let end = a + (b - a) * grown;
        let vis = seen(i).min(seen(i + 1)) * fade;
        p.line_segment([a, end], Stroke::new(width, colour.gamma_multiply(0.7 * vis)));
        // The packet rides the edge still being drawn.
        if grown < 1.0 {
            p.circle_filled(end, (side * 0.045).max(1.2), colour.gamma_multiply(vis));
        }
    }
    for i in 0..stars.len().min(build.ceil() as usize + 1) {
        p.circle_filled(at(i), (side * 0.05).max(1.3), colour.gamma_multiply(seen(i) * fade));
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

    #[test]
    fn patterns_stay_in_range_and_light_some_dots() {
        let dots = project(140, 0.3, 0.42);
        for (name, f) in [("plait", plait as fn(&Dot, f32) -> f32), ("listen", listen)] {
            let lit: Vec<f32> = dots.iter().map(|d| f(d, 0.4)).collect();
            assert!(lit.iter().all(|k| (0.0..=1.0).contains(k)), "{name} left 0..=1");
            let on = lit.iter().filter(|k| **k > 0.5).count();
            // A pattern, not a flood and not nothing.
            assert!(on > 5 && on < dots.len() * 3 / 4, "{name} lit {on} of {}", dots.len());
        }
    }

    #[test]
    fn orbit_particles_pass_behind_the_sphere() {
        let depths: Vec<f32> = (0..40).map(|i| orbit_point(0, i as f32 / 40.0, 0.42).1).collect();
        assert!(depths.iter().any(|d| *d < 0.0) && depths.iter().any(|d| *d > 0.0));
    }
}
