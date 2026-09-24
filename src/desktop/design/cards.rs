//! The card vocabulary.
//!
//! The first version of this app rendered everything as a flat row in a flat
//! container — a spreadsheet, not a dashboard. These are the pieces that give
//! a unit its own internal hierarchy: a tinted state chip, a stat tile with a
//! real numeral, a task card whose title is the loudest thing in it.

use egui::{Align, Color32, Layout, Response, RichText, Sense, Ui, Vec2};

use super::{motion, theme};
use super::tokens::{colour, pad, radius, size, space, text};

/// What a chip means. Each variant is one hue with a matching tint, so colour
/// carries meaning rather than decorating.
#[derive(Clone, Copy, PartialEq)]
pub enum Tone {
    /// Outlined, no fill — a project name, a neutral qualifier.
    Neutral,
    /// Recessed — a timestamp, an inert fact.
    Quiet,
    Ok,
    Running,
    Blocked,
    Agent,
    Info,
}

impl Tone {
    fn colours(self) -> (Color32, Color32, Color32) {
        // (fill, stroke, text)
        match self {
            Tone::Neutral => (Color32::TRANSPARENT, colour::LINE, colour::TEXT_MUTED),
            Tone::Quiet => (colour::INSET, colour::LINE, colour::TEXT_MUTED),
            Tone::Ok => (colour::OK_BG, Color32::TRANSPARENT, colour::OK),
            Tone::Running => (colour::WARN_BG, Color32::TRANSPARENT, colour::WARN),
            Tone::Blocked => (colour::DANGER_BG, Color32::TRANSPARENT, colour::DANGER),
            Tone::Agent => (colour::AGENT_BG, Color32::TRANSPARENT, colour::AGENT),
            Tone::Info => (colour::INFO_BG, Color32::TRANSPARENT, colour::INFO),
        }
    }
}

/// The tone a discipline is drawn in, so one discipline is one colour
/// everywhere in the app.
pub fn discipline_tone(discipline: &str) -> Tone {
    match discipline {
        "design" => Tone::Agent,
        "frontend" => Tone::Info,
        "backend" => Tone::Ok,
        _ => Tone::Neutral,
    }
}

/// The tone a task status is drawn in.
///
/// Shipped is the only state that means the work is out in the world, so it
/// gets the one confident green; completed is finished-but-not-out and takes
/// the cooler blue. They used to be the same word and the same colour, which
/// is exactly the distinction the two tracks exist to draw.
pub fn status_tone(status: &str) -> Tone {
    match status {
        "shipped" => Tone::Ok,
        "completed" | "done" => Tone::Info,
        "handoff" => Tone::Agent,
        "in_progress" | "active" => Tone::Running,
        "blocked" => Tone::Blocked,
        _ => Tone::Neutral,
    }
}

/// A tinted chip. `dot` prepends a filled circle in the text colour, which is
/// how a status reads as a status rather than a label.
pub fn chip(ui: &mut Ui, label: &str, tone: Tone, dot: bool) -> Response {
    let (fill, stroke, fg) = tone.colours();
    let font = egui::FontId::proportional(text::CAPTION);
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font.clone(), fg);

    let dot_w = if dot { 10.0 } else { 0.0 };
    let pad_x = space::SM;
    let height = 18.0;
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(galley.size().x + dot_w + pad_x * 2.0, height),
        Sense::hover(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label));

    let p = ui.painter();
    if fill != Color32::TRANSPARENT {
        p.rect_filled(rect, radius::SM as f32, fill);
    }
    if stroke != Color32::TRANSPARENT {
        p.rect_stroke(
            rect,
            radius::SM as f32,
            egui::Stroke::new(1.0, stroke),
            egui::StrokeKind::Inside,
        );
    }

    let mut x = rect.left() + pad_x;
    if dot {
        p.circle_filled(egui::pos2(x + 2.5, rect.center().y), 2.5, fg);
        x += dot_w;
    }
    p.galley(
        egui::pos2(x, rect.center().y - galley.size().y / 2.0),
        galley,
        fg,
    );
    response
}

/// A stat tile: a quiet label, a delta chip, a large numeral, and whatever
/// evidence belongs beside it.
pub fn stat(
    ui: &mut Ui,
    label: &str,
    value: &str,
    delta: Option<(&str, Tone)>,
    footnote: &str,
    beside: impl FnOnce(&mut Ui),
) {
    surface(ui, false, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(label)
                    .size(text::SMALL)
                    .color(colour::TEXT_MUTED),
            );
            if let Some((d, tone)) = delta {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    chip(ui, d, tone, false);
                });
            }
        });
        ui.add_space(space::MD);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(value)
                    .size(text::HERO)
                    .family(egui::FontFamily::Name(theme::BOLD.into()))
                    .color(colour::TEXT),
            );
            ui.with_layout(Layout::right_to_left(Align::Max), beside);
        });
        if !footnote.is_empty() {
            ui.add_space(space::SM);
            ui.label(
                RichText::new(footnote)
                    .size(text::CAPTION)
                    .color(colour::TEXT_MUTED),
            );
        }
    });
}

/// A bar sparkline. Enough to show a shape; anything more wants a chart.
pub fn sparkline(ui: &mut Ui, values: &[f32], tint: Color32) {
    if values.is_empty() {
        return;
    }
    let height = 30.0;
    let bar = 5.0;
    let gap = 3.0;
    let width = values.len() as f32 * (bar + gap) - gap;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let peak = values.iter().cloned().fold(1.0_f32, f32::max);

    let p = ui.painter();
    for (i, v) in values.iter().enumerate() {
        let h = (v / peak * height).max(2.0);
        let x = rect.left() + i as f32 * (bar + gap);
        p.rect_filled(
            egui::Rect::from_min_size(egui::pos2(x, rect.bottom() - h), Vec2::new(bar, h)),
            2.0,
            tint,
        );
    }
}

/// A segmented load bar: filled cells for open work, then any in review or
/// blocking, then empty. Reads as a count, not a percentage, because a count
/// is what we actually know.
pub fn segments(ui: &mut Ui, open: usize, review: usize, blocking: usize, total: usize) {
    let cells = total.max(6);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 6.0), Sense::hover());
    let gap = 3.0;
    let cw = (rect.width() - gap * (cells - 1) as f32) / cells as f32;
    let p = ui.painter();
    for i in 0..cells {
        let fill = if i < open {
            colour::ACCENT
        } else if i < open + review {
            colour::WARN
        } else if i < open + review + blocking {
            colour::DANGER
        } else {
            colour::LINE
        };
        let x = rect.left() + i as f32 * (cw + gap);
        p.rect_filled(
            egui::Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(cw, rect.height())),
            2.0,
            fill,
        );
    }
}

/// A plain card surface. Hover is a lift in fill plus a brighter border —
/// no shadow anywhere in this app.
pub fn surface<R>(ui: &mut Ui, hovered: bool, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    egui::Frame::new()
        .fill(if hovered { colour::SURFACE_HOVER } else { colour::SURFACE })
        .stroke(egui::Stroke::new(
            1.0,
            if hovered { colour::LINE_STRONG } else { colour::LINE },
        ))
        .corner_radius(radius::LG)
        .inner_margin(egui::Margin::symmetric(pad::CARD.0 as i8, pad::CARD.1 as i8))
        .show(ui, add)
}

/// The task card: chips on top, the title as the loudest element, context and
/// people underneath. This replaced the flat row, and it is the single biggest
/// reason the board stopped reading as a spreadsheet.
pub struct TaskCard<'a> {
    pub title: &'a str,
    /// Leading chips: age, discipline, status, project.
    pub chips: &'a [(String, Tone, bool)],
    /// Muted line under the title: phase, or what it waits on.
    pub context: &'a str,
    /// Right-hand chips, e.g. the agent holding it.
    pub trailing_chips: &'a [(String, Tone)],
    /// Avatar seeds for whoever owns it.
    pub people: &'a [String],
}

pub fn task_card(ui: &mut Ui, card: &TaskCard<'_>) -> Response {
    let id = ui.next_auto_id();
    let hovered = ui.ctx().data(|d| d.get_temp::<bool>(id).unwrap_or(false));

    let out = surface(ui, hovered, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = space::SM - 2.0;
            for (label, tone, dot) in card.chips {
                chip(ui, label, *tone, *dot);
            }
        });
        ui.add_space(space::SM);
        ui.label(
            RichText::new(card.title)
                .size(text::CARD)
                .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                .color(colour::TEXT),
        );
        ui.add_space(space::SM);
        ui.horizontal(|ui| {
            ui.set_min_height(size::AVATAR_SM);
            if !card.context.is_empty() {
                ui.label(
                    RichText::new(card.context)
                        .size(text::SMALL)
                        .color(colour::TEXT_MUTED),
                );
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                for seed in card.people.iter().rev() {
                    super::avatar::small(ui, seed, size::AVATAR_SM);
                }
                for (label, tone) in card.trailing_chips {
                    chip(ui, label, *tone, false);
                }
            });
        });
    });

    let response = motion::operable(ui, out.response.interact(Sense::click()), radius::LG as f32);
    ui.ctx().data_mut(|d| d.insert_temp(id, response.hovered()));
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    ui.add_space(space::SM);
    response
}

/// A compact single-line card, for claimable work where the title and one
/// action are the whole content.
pub fn slim_card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> Response {
    let out = egui::Frame::new()
        .fill(colour::SURFACE)
        .stroke(egui::Stroke::new(1.0, colour::LINE))
        .corner_radius(radius::MD)
        .inner_margin(egui::Margin::symmetric(pad::CARD.0 as i8, space::SM as i8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(size::CONTROL - space::XS);
                add(ui);
            });
        });
    ui.add_space(space::XS);
    out.response
}

/// A page's tab strip: a segmented group, not underlines.
pub fn tabs(ui: &mut Ui, labels: &[&str], selected: usize) -> Option<usize> {
    let mut clicked = None;
    egui::Frame::new()
        .fill(colour::SURFACE)
        .stroke(egui::Stroke::new(1.0, colour::LINE))
        .corner_radius(radius::SM)
        .inner_margin(egui::Margin::same(3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::XXS;
                for (i, label) in labels.iter().enumerate() {
                    let on = i == selected;
                    let galley = ui.painter().layout_no_wrap(
                        (*label).to_owned(),
                        egui::FontId::proportional(text::SMALL),
                        if on { colour::TEXT } else { colour::TEXT_MUTED },
                    );
                    let (rect, response) = ui.allocate_exact_size(
                        Vec2::new(galley.size().x + space::MD * 2.0, 24.0),
                        Sense::click(),
                    );
                    let response = motion::operable(ui, response, radius::SM as f32);
                    if on {
                        ui.painter()
                            .rect_filled(rect, radius::SM as f32, colour::SURFACE_ACTIVE);
                    } else {
                        let tint = motion::hover_fill(
                            ui,
                            response.id.with("hover"),
                            response.hovered() || response.has_focus(),
                            Color32::TRANSPARENT,
                            colour::SURFACE_HOVER,
                        );
                        ui.painter().rect_filled(rect, radius::SM as f32, tint);
                        if response.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                    }
                    ui.painter().galley(
                        rect.center() - galley.size() / 2.0,
                        galley,
                        colour::TEXT,
                    );
                    if response.clicked() {
                        clicked = Some(i);
                    }
                }
            });
        });
    ui.add_space(space::LG);
    clicked
}

/// A person's load: name, disciplines, an open count and a segmented bar.
pub fn capacity(
    ui: &mut Ui,
    seed: &str,
    name: &str,
    disciplines: &str,
    open: usize,
    review: usize,
    blocking: usize,
) {
    surface(ui, false, |ui| {
        ui.horizontal(|ui| {
            super::avatar::small(ui, seed, size::AVATAR_MD);
            ui.add_space(space::XS);
            ui.label(
                RichText::new(name)
                    .size(text::BODY)
                    .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                    .color(colour::TEXT),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(disciplines)
                        .size(text::CAPTION)
                        .color(colour::TEXT_FAINT),
                );
            });
        });
        ui.add_space(space::SM);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(open.to_string())
                    .size(text::DISPLAY)
                    .family(egui::FontFamily::Name(theme::BOLD.into()))
                    .color(colour::TEXT),
            );
            ui.add_space(space::XS);
            let note = if open == 0 {
                "free to take work".to_string()
            } else if blocking > 0 {
                format!("open · blocking {blocking}")
            } else if review > 0 {
                format!("open · {review} in review")
            } else {
                "open".to_string()
            };
            ui.label(
                RichText::new(note)
                    .size(text::SMALL)
                    .color(colour::TEXT_MUTED),
            );
        });
        ui.add_space(space::SM);
        segments(ui, open, review, blocking, 6);
    });
}
