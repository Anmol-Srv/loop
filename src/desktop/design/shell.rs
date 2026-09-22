//! The dashboard shell: a fixed sidebar, a content header, and a body.
//!
//! Views do not lay themselves out. They hand the shell a title, optional
//! actions, and a body closure — so every screen agrees on gutters, header
//! height and content width, and a new view is a `NavItem` plus a match arm
//! rather than a new layout.

use egui::{Align, Layout, Response, RichText, Ui};

use super::tokens::{colour, pad, radius, size, space, text};

/// Vertical room for the macOS traffic lights, which overlay the content when
/// the title bar is hidden.
const TRAFFIC_LIGHTS: f32 = size::ROW;

/// Where the wash's bounds are stashed, so a notch can sample the exact colour
/// behind it. The gradient is painted before the items, and the items need to
/// know it — one value, and it never leaves this module.
/// One entry in the sidebar.
///
/// `badge` is an attention count and is drawn in the accent; `count` is
/// ambient and is drawn faint. A pending-review count is a badge; the number
/// of projects is a count. Conflating them makes everything shout.
pub struct NavItem<'a> {
    pub icon: &'a str,
    pub label: &'a str,
    pub selected: bool,
    pub badge: usize,
    pub count: Option<String>,
    /// A coloured dot instead of an icon — used for project and agent rows.
    pub dot: Option<egui::Color32>,
}

impl<'a> NavItem<'a> {
    pub fn new(icon: &'a str, label: &'a str, selected: bool) -> Self {
        Self { icon, label, selected, badge: 0, count: None, dot: None }
    }
    pub fn badge(mut self, n: usize) -> Self {
        self.badge = n;
        self
    }
    pub fn count(mut self, s: impl Into<String>) -> Self {
        self.count = Some(s.into());
        self
    }
    pub fn dot(mut self, c: egui::Color32) -> Self {
        self.dot = Some(c);
        self
    }
}

/// A group of nav items under a small muted heading.
pub struct NavGroup<'a> {
    pub label: &'a str,
    pub items: Vec<NavItem<'a>>,
}

/// Draws the sidebar and returns the index of a clicked item, if any.
/// The sidebar: a brand row, a search affordance, grouped navigation, and the
/// signed-in person pinned at the foot.
///
/// Returns `(group, item)` of whatever was clicked. Grouping is the point: a
/// flat list of four items in 232px reads as an accident, and the group labels
/// are what let projects and agents live here without competing with the
/// primary destinations.
pub fn sidebar(
    ui: &mut Ui,
    brand: (&str, &str),
    groups: &[NavGroup<'_>],
    footer: impl FnOnce(&mut Ui),
) -> Option<(usize, usize)> {
    let mut clicked = None;
    let narrow = ui.max_rect().width() < size::SIDEBAR_COLLAPSE_AT;
    let width = if narrow { size::SIDEBAR_W_NARROW } else { size::SIDEBAR_W };

    egui::Panel::left("sidebar")
        .exact_size(width)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(colour::CHROME)
                .inner_margin(egui::Margin::symmetric(pad::SIDEBAR.0 as i8, pad::SIDEBAR.1 as i8)),
        )
        .show(ui, |ui| {
            // Room for the traffic lights, which float over this corner
            // because the window has no title bar.
            ui.add_space(TRAFFIC_LIGHTS);

            if !narrow {
                brand_row(ui, brand.0, brand.1);
                ui.add_space(space::MD);
                search_field(ui);
                ui.add_space(space::MD);
            }

            for (g, group) in groups.iter().enumerate() {
                if !narrow && !group.label.is_empty() {
                    ui.add_space(space::SM);
                    ui.label(
                        RichText::new(group.label)
                            .size(text::CAPTION)
                            .family(egui::FontFamily::Name(super::theme::BOLD.into()))
                            .color(colour::TEXT_FAINT),
                    );
                    ui.add_space(space::XXS);
                }
                for (i, item) in group.items.iter().enumerate() {
                    if nav_item(ui, item, narrow).clicked() {
                        clicked = Some((g, i));
                    }
                }
                ui.add_space(space::SM);
            }

            if !narrow {
                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    ui.add_space(space::SM);
                    footer(ui);
                    ui.add_space(space::MD);
                    let line = ui.available_rect_before_wrap();
                    ui.painter().hline(
                        line.x_range(),
                        ui.cursor().top(),
                        egui::Stroke::new(1.0, colour::LINE_SOFT),
                    );
                });
            }
        });

    clicked
}

/// The product mark and name.
/// The mark, as an alpha mask.
///
/// The source art is a white glyph on a black disc. Stored as luminance-only
/// so the black never renders — a bitmap with its own background would sit on
/// the sidebar as a slightly-wrong-coloured square — and so the mark can be
/// tinted like any other ink.
const MARK: &[u8] = include_bytes!("../../../assets/mark.png");
const MARK_SIZE: f32 = 26.0;

/// Decode once, then hand out the same handle every frame.
///
/// The lookup and the load are deliberately separate statements: `data_mut`
/// holds the context's write lock for the whole closure, and `load_texture`
/// reaches for that same lock, so doing the load inside the closure
/// deadlocks the first frame and the window never appears. It did.
fn mark_texture(ctx: &egui::Context) -> egui::TextureHandle {
    let id = egui::Id::new("brand:mark");
    if let Some(handle) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return handle;
    }

    let decoded = image::load_from_memory(MARK).expect("the mark is baked in").to_luma_alpha8();
    let (w, h) = decoded.dimensions();
    // Every pixel is white and the alpha carries the shape, so one tint at
    // draw time colours the whole mark.
    let pixels: Vec<egui::Color32> =
        decoded.pixels().map(|p| egui::Color32::from_white_alpha(p.0[1])).collect();
    let handle = ctx.load_texture(
        "brand:mark",
        egui::ColorImage {
            size: [w as usize, h as usize],
            pixels,
            source_size: egui::vec2(w as f32, h as f32),
        },
        egui::TextureOptions::LINEAR,
    );
    ctx.data_mut(|d| d.insert_temp(id, handle.clone()));
    handle
}

fn brand_row(ui: &mut Ui, name: &str, tagline: &str) {
    ui.horizontal(|ui| {
        let texture = mark_texture(ui.ctx());
        ui.add(
            egui::Image::new(&texture)
                .fit_to_exact_size(egui::Vec2::splat(MARK_SIZE))
                .tint(colour::TEXT),
        );
        ui.add_space(space::SM);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(
                RichText::new(name)
                    .size(text::BODY)
                    .family(egui::FontFamily::Name(super::theme::SEMIBOLD.into()))
                    .color(colour::TEXT),
            );
            ui.label(
                RichText::new(tagline)
                    .size(text::CAPTION)
                    .color(colour::TEXT_FAINT),
            );
        });
    });
}

/// A search affordance. Not wired yet — it is drawn because its absence is the
/// loudest thing missing from a sidebar of this shape, and because the
/// keyboard shortcut needs somewhere to advertise itself.
fn search_field(ui: &mut Ui) {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), size::CONTROL + 2.0),
        egui::Sense::click(),
    );
    let p = ui.painter();
    p.rect_filled(rect, radius::SM as f32, colour::INSET);
    p.rect_stroke(
        rect,
        radius::SM as f32,
        egui::Stroke::new(1.0, if response.hovered() { colour::LINE_STRONG } else { colour::LINE }),
        egui::StrokeKind::Inside,
    );
    p.text(
        egui::pos2(rect.left() + space::SM, rect.center().y),
        egui::Align2::LEFT_CENTER,
        egui_phosphor::thin::MAGNIFYING_GLASS,
        egui::FontId::proportional(text::BODY),
        colour::TEXT_FAINT,
    );
    p.text(
        egui::pos2(rect.left() + space::SM + size::ICON_COL, rect.center().y),
        egui::Align2::LEFT_CENTER,
        "Search",
        egui::FontId::proportional(text::SMALL),
        colour::TEXT_FAINT,
    );
    // The shortcut chip, so the affordance teaches itself.
    for (i, key) in ["K", "\u{2318}"].iter().enumerate() {
        let w = 18.0;
        let chip = egui::Rect::from_center_size(
            egui::pos2(rect.right() - space::SM - w / 2.0 - i as f32 * (w + 2.0), rect.center().y),
            egui::vec2(w, 16.0),
        );
        p.rect_filled(chip, radius::SM as f32 - 2.0, colour::SURFACE_HOVER);
        p.text(
            chip.center(),
            egui::Align2::CENTER_CENTER,
            key,
            egui::FontId::proportional(text::CAPTION),
            colour::TEXT_MUTED,
        );
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
}

fn nav_item(ui: &mut Ui, item: &NavItem<'_>, narrow: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), size::NAV_ROW),
        egui::Sense::click(),
    );
    let response = super::motion::operable(ui, response, radius::SM as f32);

    let r = radius::SM as f32;
    // The selected pill eases in, so switching tabs reads as one surface
    // moving rather than two surfaces blinking.
    let sel = super::motion::to(ui, response.id.with("sel"), item.selected, super::motion::BASE);
    let hov = super::motion::to(
        ui,
        response.id.with("hov"),
        response.hovered() || response.has_focus(),
        super::motion::FAST,
    );
    let p = ui.painter();
    if sel > 0.001 {
        p.rect_filled(rect, r, colour::SURFACE_ACTIVE.gamma_multiply(sel));
    } else if hov > 0.001 {
        p.rect_filled(rect, r, colour::SURFACE.gamma_multiply(hov));
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let fg = if item.selected { colour::TEXT } else { colour::TEXT_MUTED };
    let p = ui.painter();

    if narrow {
        if let Some(c) = item.dot {
            p.circle_filled(rect.center(), 3.5, c);
        } else {
            p.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                item.icon,
                egui::FontId::proportional(text::HEADING),
                fg,
            );
        }
        return response.on_hover_text(item.label);
    }

    let x = rect.left() + space::SM;
    if let Some(c) = item.dot {
        p.circle_filled(egui::pos2(x + 6.0, rect.center().y), 3.5, c);
    } else {
        p.text(
            egui::pos2(x, rect.center().y),
            egui::Align2::LEFT_CENTER,
            item.icon,
            egui::FontId::proportional(text::HEADING),
            fg,
        );
    }
    // A badge demands attention and takes the accent; a count is ambient and
    // stays faint. Both right-aligned, never both present. Measured before the
    // label is painted, so a long name truncates into what is left of the rail
    // instead of running under them.
    let trailing = if item.badge > 0 {
        Some((
            p.layout_no_wrap(
                item.badge.to_string(),
                egui::FontId::proportional(text::CAPTION),
                colour::ON_ACCENT,
            ),
            true,
        ))
    } else {
        item.count.as_ref().map(|count| {
            (
                p.layout_no_wrap(
                    count.clone(),
                    egui::FontId::proportional(text::CAPTION),
                    colour::TEXT_FAINT,
                ),
                false,
            )
        })
    };
    let trailing_w = trailing
        .as_ref()
        .map(|(g, badge)| g.size().x + if *badge { space::MD } else { 0.0 } + space::SM)
        .unwrap_or(0.0);

    let label_x = x + size::ICON_COL;
    let label = super::widgets::truncated(
        ui,
        item.label,
        egui::FontId::proportional(text::BODY),
        fg,
        (rect.right() - space::SM - trailing_w - label_x).max(0.0),
    );
    p.galley(egui::pos2(label_x, rect.center().y - label.size().y / 2.0), label, fg);

    match trailing {
        Some((galley, true)) => {
            let w = galley.size().x + space::MD;
            let badge = egui::Rect::from_center_size(
                egui::pos2(rect.right() - space::SM - w / 2.0, rect.center().y),
                egui::vec2(w, size::BADGE_H),
            );
            p.rect_filled(badge, radius::PILL as f32, colour::DANGER);
            p.galley(badge.center() - galley.size() / 2.0, galley, colour::TEXT);
        }
        Some((galley, false)) => {
            let at = egui::pos2(
                rect.right() - space::SM - galley.size().x,
                rect.center().y - galley.size().y / 2.0,
            );
            p.galley(at, galley, colour::TEXT_FAINT);
        }
        None => {}
    }

    response
}

/// The content area. No header strip: gutters and scrolling applied once, then
/// the view owns everything inside. Each view carries its own heading and back
/// control, so a shared header would only repeat them.
pub fn content(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(colour::CANVAS)
                .inner_margin(egui::Margin::symmetric(space::XXL as i8, pad::PAGE.1 as i8)),
        )
        .show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                // Cap the measure and centre it: past ~1080px the cards just
                // stretch, and a task title 1400px wide is unreadable.
                let avail = ui.available_width();
                if avail > size::CONTENT_MAX {
                    let side = (avail - size::CONTENT_MAX) / 2.0;
                    ui.horizontal(|ui| {
                        ui.add_space(side);
                        ui.vertical(|ui| {
                            ui.set_max_width(size::CONTENT_MAX);
                            body(ui);
                        });
                    });
                } else {
                    body(ui);
                }
            });
        });
}

/// The top of a page: title, optional subtitle, optional trailing control.
///
/// Six panes each drew their own, landing on four different gaps under the
/// same rank of element and two different title weights. `shell::content`
/// deliberately owns no header strip — but that left the one element present
/// on every screen as the one element with no shared implementation.
pub fn page_title(
    ui: &mut Ui,
    title: &str,
    subtitle: &str,
    trailing: impl FnOnce(&mut Ui),
) {
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            trailing(ui);
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(title)
                            .size(text::TITLE)
                            .family(egui::FontFamily::Name(super::theme::SEMIBOLD.into()))
                            .color(colour::TEXT),
                    )
                    .truncate(),
                );
            });
        });
    });
    if !subtitle.is_empty() {
        ui.add_space(space::XXS);
        ui.label(
            RichText::new(subtitle)
                .size(text::SMALL)
                .color(colour::TEXT_MUTED),
        );
    }
    ui.add_space(space::LG);
}

/// A back control above a page title. One spelling, so the two panes that have
/// one stop disagreeing about the gap beneath it.
pub fn back(ui: &mut Ui, label: &str) -> egui::Response {
    let response = super::widgets::icon_button(
        ui,
        egui_phosphor::thin::ARROW_LEFT,
        label,
        super::widgets::Emphasis::Link,
        true,
    );
    ui.add_space(space::XS);
    response
}

/// A section heading with something trailing on the right — a count, a live
/// pill, an action. The task view rebuilt this inline because `section` takes
/// a label and nothing else.
pub fn section_with(ui: &mut Ui, label: &str, trailing: impl FnOnce(&mut Ui)) {
    ui.add_space(space::XL);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(label)
                .size(text::SMALL)
                .family(egui::FontFamily::Name(super::theme::MEDIUM.into()))
                .color(colour::TEXT_MUTED),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), trailing);
    });
    ui.add_space(space::MD);
}

/// A section heading with a count beside it, the count a shade fainter.
///
/// Two views folded the count into the label string because nothing offered
/// this; the result was a count in the same tone as its heading, which reads
/// as part of the title.
pub fn section_count(ui: &mut Ui, label: &str, count: usize) {
    section_count_with(ui, label, count, |_| {});
}

/// A section heading, its count, and something on the right.
///
/// The trailing slot is why this went unused: two views needed a "View all"
/// or a matched-disciplines note beside the heading, `section_count` offered
/// nowhere to put it, so both hand-rolled the whole thing and lost the fainter
/// count in the process.
pub fn section_count_with(
    ui: &mut Ui,
    label: &str,
    count: usize,
    trailing: impl FnOnce(&mut Ui),
) {
    ui.add_space(space::XL);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(label)
                .size(text::SMALL)
                .family(egui::FontFamily::Name(super::theme::MEDIUM.into()))
                .color(colour::TEXT_MUTED),
        );
        ui.label(
            RichText::new(count.to_string())
                .size(text::SMALL)
                .color(colour::TEXT_FAINT),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), trailing);
    });
    ui.add_space(space::MD);
}

/// A section heading inside a page body.
/// A hairline between two stacked sections.
///
/// A labelled heading alone was not enough separation on the task page: four
/// sections down one column at the same weight read as one long column with
/// words in it. The rule is what says "this is a different thing".
pub fn divider(ui: &mut Ui) {
    ui.add_space(space::XL);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 1.0),
        egui::Sense::hover(),
    );
    ui.painter().hline(rect.x_range(), rect.center().y, egui::Stroke::new(1.0, colour::LINE));
}

/// A page split into a content column and a properties rail.
///
/// The rail is Linear's shape and it earns its place: status, priority and
/// owner are facts you glance at, not prose you read, so they do not belong
/// in the reading column. Below `RAIL_AT` the window is too narrow to carry
/// both and the rail stacks under the content instead.
pub fn with_rail(ui: &mut Ui, content: impl FnOnce(&mut Ui), rail: impl FnOnce(&mut Ui)) {
    if ui.available_width() < RAIL_AT {
        content(ui);
        ui.add_space(space::XL);
        rail_surface(ui, rail);
        return;
    }

    let full = ui.available_width();
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = space::XXL;
        ui.allocate_ui_with_layout(
            egui::vec2(full - size::RAIL_W - space::XXL, 0.0),
            Layout::top_down(Align::Min),
            |ui| {
                ui.set_width(ui.available_width());
                content(ui);
            },
        );
        ui.allocate_ui_with_layout(
            egui::vec2(size::RAIL_W, 0.0),
            Layout::top_down(Align::Min),
            |ui| {
                ui.set_width(size::RAIL_W);
                rail_surface(ui, rail);
            },
        );
    });
}

/// Content width at which the properties rail sits beside the content rather
/// than under it. Below this the reading column would be narrower than the
/// rail, which is the wrong way round.
const RAIL_AT: f32 = 760.0;

fn rail_surface(ui: &mut Ui, rail: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(colour::SURFACE)
        .stroke(egui::Stroke::new(1.0, colour::LINE))
        .corner_radius(radius::LG)
        .inner_margin(egui::Margin::same(space::LG as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            rail(ui);
        });
}

/// One row of the properties rail: a muted label, then the value.
///
/// The label column is fixed so every value in the rail starts at the same x
/// — a ragged left edge down a list of facts is the thing that makes a rail
/// look like a pile.
pub fn property(ui: &mut Ui, label: &str, value: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(size::CONTROL);
        ui.spacing_mut().item_spacing.x = space::SM;
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(PROPERTY_LABEL_W, size::CONTROL),
            egui::Sense::hover(),
        );
        ui.painter().text(
            egui::pos2(rect.left(), rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(text::SMALL),
            colour::TEXT_MUTED,
        );
        value(ui);
    });
    ui.add_space(space::XS);
}

/// Wide enough for "Department", which is the longest label the rail carries.
const PROPERTY_LABEL_W: f32 = 82.0;

pub fn section(ui: &mut Ui, label: &str) {
    ui.add_space(space::XL);
    ui.label(
        RichText::new(label)
            .size(text::SMALL)
            .family(egui::FontFamily::Name(super::theme::MEDIUM.into()))
            .color(colour::TEXT_MUTED),
    );
    ui.add_space(space::MD);
}
