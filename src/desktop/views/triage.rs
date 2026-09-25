//! Triage: tasks an intake agent filed for someone, waiting on a yes or a no.
//!
//! Linear's Triage is the model — a compact row per item, the source it came
//! from underneath, and the decision inline. The pieces here are shared: My
//! Tasks draws the group, the task page draws the source card and the same
//! Accept / Dismiss, and every row's right-click menu offers them too (see
//! `menus::task_items`). The requests themselves go through `menus::Tasks`,
//! like every other task action, so the toast and the invalidation are one.

use egui::{RichText, Sense, Vec2};
use serde_json::Value;

use super::menus::{Pick, Viewer};
use super::projects::{PROJECTS_KEY, PROSE_W};
use crate::desktop::design::{
    cards as c, colour, glyph, motion, radius, shell, size, space, text, theme, viz, widgets as w,
};
use crate::desktop::net::Net;

pub(super) const NOT_YOURS: &str = "Only the person it was filed for, or an admin, can triage it.";

/// Two lines — the title, then where it came from — at a list row's pitch.
const ROW_H: f32 = 56.0;
/// The split button's caret half.
const CARET_W: f32 = 24.0;

/// The categories an intake agent files under, wire value first.
pub(super) const CATEGORIES: [(&str, &str); 5] =
    [("bug", "Bug"), ("feature", "Feature"), ("feedback", "Feedback"), ("question", "Question"), ("chore", "Chore")];

pub(super) fn category_label(value: &str) -> &str {
    CATEGORIES.iter().find(|(v, _)| *v == value).map_or(value, |(_, l)| l)
}

/// One tinted chip per category, from the chip tones: every one of them
/// clears 4.5:1 on its own fill.
pub(super) fn category_tone(value: &str) -> c::Tone {
    match value {
        "bug" => c::Tone::Blocked,
        "feature" => c::Tone::Agent,
        "feedback" => c::Tone::Info,
        _ => c::Tone::Quiet,
    }
}

/// The projects a triaged task can be accepted into, from the list the
/// Projects page keeps: (id, name), live ones only.
pub(super) fn projects(net: &Net) -> Vec<(String, String)> {
    net.data(PROJECTS_KEY)
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter(|p| !super::board::archived(p))
                .filter_map(|p| Some((str_of(p, "id")?.to_owned(), str_of(p, "name")?.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

/// Ask for that list, on a page that has triage on it.
pub(super) fn want_projects(net: &mut Net) {
    net.get_once(PROJECTS_KEY, "/api/user/projects");
}

/// Whether the viewer may decide on `t`.
pub(super) fn decides(t: &Value, viewer: &Viewer) -> bool {
    viewer.admin || (!viewer.me.is_empty() && str_of(t, "assigneePersonId") == Some(viewer.me.as_str()))
}

// ------------------------------------------------------------------ the source

fn source_of(t: &Value) -> Option<&Value> {
    t.get("source").filter(|s| s.is_object())
}

/// A DM: the request said so, or the channel is a Slack DM id.
fn private(src: &Value) -> bool {
    src.get("private").and_then(Value::as_bool).unwrap_or(false)
        || str_of(src, "channel").is_some_and(|c| c.starts_with('D'))
}

/// "#issues-and-feedback", "Direct message", or nothing for a bare channel id
/// that has no name to show.
fn channel(src: &Value) -> Option<String> {
    if private(src) {
        return Some("Direct message".to_owned());
    }
    let raw = str_of(src, "channelName").or_else(|| str_of(src, "channel"))?.trim();
    if raw.is_empty() {
        return None;
    }
    if raw.starts_with('#') {
        return Some(raw.to_owned());
    }
    // "C04AB12CD": an id, not a name. Better nothing than a code.
    let id_like = raw.len() >= 9 && raw.chars().all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit());
    (!id_like).then(|| format!("#{raw}"))
}

fn kind_word(kind: &str) -> String {
    match kind {
        "slack" => "Slack".to_owned(),
        "github" => "GitHub".to_owned(),
        other => {
            let mut ch = other.chars();
            ch.next().map(|f| f.to_uppercase().chain(ch).collect()).unwrap_or_else(|| "Intake".to_owned())
        }
    }
}

/// "Slack · #issues-and-feedback · Priya · 12m". The author is absent for a
/// teammate reading someone else's DM, and so is left out.
pub(super) fn source_words(src: &Value, short_age: bool) -> String {
    let mut parts = vec![kind_word(str_of(src, "kind").unwrap_or("slack"))];
    parts.extend(channel(src));
    if let Some(a) = str_of(src, "author").map(str::trim).filter(|a| !a.is_empty()) {
        parts.push(a.to_owned());
    }
    if let Some(at) = str_of(src, "receivedAt") {
        let age = if short_age { super::mytasks::age(at) } else { super::task::ago(at) };
        if !age.is_empty() {
            parts.push(age);
        }
    }
    parts.join(" \u{00B7} ")
}

/// The source's mark, at `side`.
fn mark(ui: &mut egui::Ui, src: &Value, side: f32, ink: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
    glyph::source(ui.painter(), rect.center(), side, str_of(src, "kind").unwrap_or("slack"), ink);
}

/// The source, as a card under the description: where it came from, the
/// message as a quote, and why the agent filed it.
///
/// A teammate reading a task filed from someone's DM gets the fact that it
/// came from one and nothing of what was said — the server withholds the
/// text and the author; this only declines to invent them.
pub(super) fn source_card(ui: &mut egui::Ui, t: &Value) {
    let Some(src) = source_of(t) else { return };
    let text_ = str_of(src, "text").map(str::trim).filter(|s| !s.is_empty());
    let readable = text_.is_some();
    let owner = str_of(t, "assigneeName").and_then(|n| n.split_whitespace().next()).unwrap_or("its owner");

    shell::section(ui, "Source");
    ui.scope(|ui| {
        ui.set_max_width(PROSE_W.min(ui.available_width()));
        w::card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = space::SM;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::XS;
                mark(ui, src, text::BODY, colour::TEXT_MUTED);
                let url = str_of(src, "url").filter(|u| u.starts_with("http"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(url) = url.filter(|_| readable) {
                        let label = match str_of(src, "kind") {
                            Some("slack") | None => "Open in Slack".to_owned(),
                            Some(k) => format!("Open in {}", kind_word(k)),
                        };
                        if w::link(ui, &label).on_hover_text(url).clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                        }
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(
                            egui::Label::new(RichText::new(source_words(src, false)).size(text::SMALL).color(colour::TEXT_2))
                                .truncate(),
                        );
                    });
                });
            });

            match text_ {
                Some(said) => quote(ui, said),
                None if private(src) => {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::XS;
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(text::SMALL + 1.0), Sense::hover());
                        glyph::lock(ui.painter(), r.center(), r.width(), colour::TEXT_MUTED);
                        w::muted(ui, &format!("From a direct message \u{2014} only {owner} can read it."));
                    });
                }
                None => {}
            }

            filed_by(ui, src);
        });
    });
}

/// The original words, as a quote: a rule down the left and the text beside
/// it. Not a bubble — this is a record of what was said, not a conversation.
fn quote(ui: &mut egui::Ui, said: &str) {
    let out = egui::Frame::new()
        .inner_margin(egui::Margin { left: space::MD as i8, right: 0, top: space::XXS as i8, bottom: space::XXS as i8 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(said).size(text::BODY).color(colour::TEXT_2));
        });
    let r = out.response.rect;
    ui.painter().rect_filled(
        egui::Rect::from_min_size(r.min, Vec2::new(space::XXS, r.height())),
        radius::SM as f32,
        colour::LINE_STRONG,
    );
}

/// "Filed by Slack Agent — looks like a bug: … · 0.86".
fn filed_by(ui: &mut egui::Ui, src: &Value) {
    let agent = str_of(src, "agentName")
        .or_else(|| src.get("agent").and_then(|a| str_of(a, "name")))
        .unwrap_or("an intake agent");
    let mut line = format!("Filed by {agent}");
    if let Some(reason) = str_of(src, "reason").map(str::trim).filter(|r| !r.is_empty()) {
        line += &format!(" \u{2014} {reason}");
    }
    let confidence = src.get("confidence").and_then(Value::as_f64);
    if let Some(c) = confidence {
        line += &format!(" \u{00B7} {c:.2}");
    }
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = space::XS;
        w::agent_mark(ui, text::BODY);
        let r = ui.label(RichText::new(line).size(text::SMALL).color(colour::TEXT_MUTED));
        if confidence.is_some() {
            r.on_hover_text("The last figure is how sure the agent was, from 0 to 1.");
        }
    });
}

// ------------------------------------------------------------------ category

/// The category chip, and — for someone who may edit it — a menu that sets
/// it. Returns the category picked this frame. Without a category, an
/// editor gets a quiet "Category" chip to set one; a reader gets nothing.
pub(super) fn category_chip(ui: &mut egui::Ui, current: Option<&str>, editable: bool) -> Option<String> {
    let chip = match current {
        Some(v) => c::chip(ui, category_label(v), category_tone(v), false),
        None if editable => c::chip(ui, "Category", c::Tone::Neutral, false),
        None => return None,
    };
    if !editable {
        return None;
    }
    let id = chip.id.with("pick");
    let r = ui.interact(chip.rect, id, Sense::click());
    r.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::ComboBox,
            true,
            format!("Category: {}", current.map_or("none", category_label)),
        )
    });
    let r = motion::operable_sm(ui, r);
    if r.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        ui.painter().rect_stroke(chip.rect, radius::SM as f32, egui::Stroke::new(1.0, colour::LINE_STRONG), egui::StrokeKind::Inside);
    }
    let mut picked = None;
    viz::click_menu(&r, |ui| {
        for (value, label) in CATEGORIES {
            if viz::menu_choice(ui, label, current == Some(value)) && current != Some(value) {
                picked = Some(value.to_owned());
            }
        }
    });
    picked
}

// ------------------------------------------------------------------ decisions

/// Accept (with the project it goes to behind a caret) and Dismiss, laid out
/// right to left: call from a right-to-left layout. `deciding` shows the
/// request in flight instead of the buttons.
pub(super) fn actions(ui: &mut egui::Ui, t: &Value, viewer: &Viewer, deciding: bool) -> Option<Pick> {
    if deciding {
        ui.add(egui::Spinner::new().size(text::BODY));
        return None;
    }
    let mut pick = None;
    if w::ghost(ui, "Dismiss").on_hover_text("Drop it, with an optional reason").clicked() {
        pick = Some(Pick::Dismiss);
    }
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let caret = caret_button(ui);
        let here = str_of(t, "projectId");
        let kept = str_of(t, "projectName").filter(|n| !n.is_empty()).unwrap_or("its intake project");
        viz::click_menu(&caret, |ui| {
            ui.horizontal(|ui| {
                ui.set_height(size::CONTROL - space::XS);
                ui.add_space(space::SM);
                w::caption(ui, "Accept into");
            });
            if viz::menu_item(ui, &format!("Keep in {kept}"), false, None) {
                pick = Some(Pick::Accept(None));
            }
            let others: Vec<&(String, String)> =
                viewer.projects.iter().filter(|(id, _)| Some(id.as_str()) != here).collect();
            if !others.is_empty() {
                viz::menu_rule(ui);
            }
            for (id, name) in others {
                if viz::menu_item(ui, name, false, None) {
                    pick = Some(Pick::Accept(Some((id.clone(), name.clone()))));
                }
            }
        });
        ui.add_space(1.0);
        if w::secondary(ui, "Accept", true).on_hover_text(format!("Make it an open task in {kept}")).clicked() {
            pick = Some(Pick::Accept(None));
        }
    });
    pick
}

/// The caret half of Accept: opens "Accept into".
fn caret_button(ui: &mut egui::Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(CARET_W, size::CONTROL), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Accept into a project"));
    let response = motion::operable_sm(ui, response);
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
    let hot = response.hovered() || response.has_focus() || open;
    let p = ui.painter();
    p.rect_filled(rect, radius::SM as f32, if hot { colour::GLASS_HOVER } else { colour::GLASS });
    p.rect_stroke(
        rect,
        radius::SM as f32,
        egui::Stroke::new(1.0, if hot { colour::EDGE_HI_HOVER } else { colour::EDGE_MID }),
        egui::StrokeKind::Inside,
    );
    glyph::caret(p, rect.center(), text::BODY, 1.0, if hot { colour::TEXT } else { colour::TEXT_MUTED });
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if open {
        response
    } else {
        response.on_hover_text("Accept into another project")
    }
}

// ------------------------------------------------------------------ the group

/// What the group asked for: a task to open, or a decision.
pub(super) enum Out {
    Open(String),
    Pick(Value, Pick),
}

/// The Triage group at the top of My Tasks. Nothing at all for someone with
/// no intake agent and nothing filed; the empty state only for someone who
/// has one, so it teaches where these come from rather than advertising it.
pub(super) fn group(ui: &mut egui::Ui, rows: &[&Value], viewer: &Viewer, deciding: Option<&str>, has_intake: bool) -> Option<Out> {
    if rows.is_empty() && !has_intake {
        return None;
    }
    shell::section_count(ui, "Triage", rows.len());
    if rows.is_empty() {
        w::empty(ui, "Nothing to triage.", "Tasks your intake agents file land here.");
        return None;
    }
    let mut out = None;
    w::card_list(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 0.0;
        for (i, t) in rows.iter().enumerate() {
            if i > 0 {
                let y = ui.cursor().top();
                ui.painter().hline(ui.max_rect().x_range(), y, egui::Stroke::new(1.0, colour::LINE_SOFT));
            }
            let busy = deciding.is_some() && deciding == str_of(t, "id");
            if let Some(o) = row(ui, t, viewer, busy) {
                out = Some(o);
            }
        }
    });
    out
}

fn row(ui: &mut egui::Ui, t: &Value, viewer: &Viewer, busy: bool) -> Option<Out> {
    let title = str_of(t, "title").unwrap_or("Untitled");
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title));
    let response = motion::operable_sm(ui, response);
    let mut out = None;

    let mut pick = None;
    let menu_open = viz::context_menu(&response, |ui| {
        if let Some(p) = super::menus::task_items(ui, t, viewer, true) {
            pick = Some(p);
        }
    });
    let lit = response.hovered() || response.has_focus() || menu_open;
    let tint = motion::hover_fill(ui, response.id.with("hover"), lit, colour::TRANSPARENT, colour::SURFACE_HOVER);
    if tint != colour::TRANSPARENT {
        ui.painter().rect_filled(rect, radius::SM as f32, tint);
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let mut inner = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(space::MD, 0.0)))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    inner.spacing_mut().item_spacing.x = space::SM;
    if decides(t, viewer) {
        pick = pick.or(actions(&mut inner, t, viewer, busy));
    }
    // Two lines at fixed heights in what the actions left: nested layouts
    // each take a control's height and spill into the next row.
    let left = rect.left() + space::MD;
    let right = inner.min_rect().left().min(rect.right() - space::MD) - space::SM;
    let mut line = |top: f32, h: f32| {
        ui.new_child(
            egui::UiBuilder::new()
                .max_rect(egui::Rect::from_x_y_ranges(left..=right, top..=top + h))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        )
    };
    let mut first = line(rect.top() + space::SM, size::CONTROL - space::XS);
    first.spacing_mut().item_spacing.x = space::SM;
    if let Some(cat) = str_of(t, "category") {
        c::chip(&mut first, category_label(cat), category_tone(cat), false);
    }
    first.add(
        egui::Label::new(
            RichText::new(title)
                .size(text::BODY)
                .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                .color(colour::TEXT),
        )
        .truncate()
        .selectable(false),
    );
    if let Some(src) = source_of(t) {
        let mut second = line(rect.bottom() - space::SM - text::SMALL * 1.5, text::SMALL * 1.5);
        second.spacing_mut().item_spacing.x = space::XS;
        mark(&mut second, src, text::SMALL, colour::TEXT_MUTED);
        let mut words = source_words(src, true);
        if let Some(said) = str_of(src, "text").map(str::trim).filter(|s| !s.is_empty()) {
            words += &format!("  \u{201c}{}\u{201d}", said.lines().next().unwrap_or_default());
        }
        second.add(
            egui::Label::new(RichText::new(words).size(text::SMALL).color(colour::TEXT_MUTED))
                .truncate()
                .selectable(false),
        );
    }

    match pick {
        Some(Pick::Open) => out = str_of(t, "id").map(|id| Out::Open(id.to_owned())),
        Some(p) => out = Some(Out::Pick(t.clone(), p)),
        None if response.clicked() => out = str_of(t, "id").map(|id| Out::Open(id.to_owned())),
        None => {}
    }
    out
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn source_words_read_like_a_line() {
        let s = json!({"kind": "slack", "channel": "C04AB12CD", "channelName": "issues-and-feedback", "author": "Priya"});
        assert_eq!(source_words(&s, true), "Slack \u{00B7} #issues-and-feedback \u{00B7} Priya");
        // A bare id with no name says nothing rather than a code.
        assert_eq!(source_words(&json!({"kind": "slack", "channel": "C04AB12CD"}), true), "Slack");
        // A DM, as a teammate sees it: no author, and never the id.
        assert_eq!(source_words(&json!({"kind": "slack", "channel": "D0123ABCD"}), true), "Slack \u{00B7} Direct message");
        assert_eq!(source_words(&json!({"channel": "#general", "private": false}), true), "Slack \u{00B7} #general");
    }
}
