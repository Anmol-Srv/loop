//! Notifications: the bell in every page's toolbar, the panel it opens in the
//! top-right, and a popup when something new arrives.
//!
//! The server writes them (migration 20261007000027) when someone else acts
//! on your work: assigns you, moves or notes on your task, your agent asks,
//! plans or submits, or a task you were waiting on finishes. The app asks for
//! the newest fifty every 30 seconds — an ETag'd GET, so a quiet poll is a 304.

use std::cell::RefCell;

use egui::{RichText, Sense, Vec2};
use egui_phosphor::regular as icon;
use serde_json::{json, Value};

use super::task::{ago, day_label, exact};
use crate::desktop::design::{colour, radius, size, space, status_label, text, theme, widgets as w};
use crate::desktop::App;

const KEY: &str = "notifications";
const READ_KEY: &str = "notifications:read";
const PATH: &str = "/api/user/notifications";
/// Seconds between polls.
const EVERY: f64 = 30.0;
const PANEL_W: f32 = 400.0;

#[derive(Default)]
struct State {
    /// The server these belong to: a workspace switch starts over.
    server: String,
    polled_at: Option<f64>,
    /// The reply last looked at, by generation.
    seen_gen: u64,
    /// The newest id already announced; `None` until the first reply, which
    /// sets it without popping anything up.
    newest: Option<i64>,
    open: bool,
    unread_only: bool,
    /// The task open last frame, so opening one marks its notifications read.
    opened: Option<String>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn items(app: &App) -> (Vec<Value>, i64) {
    let Some(d) = app.net.as_ref().and_then(|n| n.data(KEY)) else { return (Vec::new(), 0) };
    let items = d.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
    (items, d.get("unread").and_then(Value::as_i64).unwrap_or(0))
}

fn str_of<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str)
}

/// The sentence a notification reads as, and its mark.
fn words(n: &Value) -> (String, &'static str, egui::Color32) {
    let actor = str_of(n, "actor").unwrap_or("Someone");
    let detail = str_of(n, "detail").unwrap_or_default();
    match str_of(n, "kind").unwrap_or_default() {
        "assigned" => (format!("{actor} assigned you"), icon::USER_PLUS, colour::INFO()),
        "status" => (format!("{actor} moved it to {}", status_label(detail)), icon::ARROWS_LEFT_RIGHT, colour::TEXT_MUTED()),
        "note" => (format!("{actor} left a note"), icon::CHAT_CIRCLE_TEXT, colour::TEXT_MUTED()),
        "question" => (format!("{actor} has a question"), icon::QUESTION, colour::WARN()),
        "plan" => (format!("{actor} sent a plan to review"), icon::LIST_CHECKS, colour::AGENT()),
        "submitted" => (format!("{actor} submitted it for review"), icon::ARROW_FAT_UP, colour::AGENT()),
        "unblocked" => (format!("{detail} is done \u{2014} you\u{2019}re unblocked"), icon::LOCK_OPEN, colour::OK()),
        _ => (format!("{actor} updated it"), icon::BELL_SIMPLE, colour::TEXT_MUTED()),
    }
}

/// The second line: the note or plan's words, where there are any.
fn excerpt(n: &Value) -> Option<String> {
    match str_of(n, "kind")? {
        "note" | "question" | "plan" | "submitted" => {
            let flat = str_of(n, "detail")?.split_whitespace().collect::<Vec<_>>().join(" ");
            Some(flat).filter(|s| !s.is_empty())
        }
        _ => None,
    }
}

/// Poll, and announce what is new. Once a frame, before the page draws.
pub fn tick(app: &mut App, ctx: &egui::Context) {
    let Some(net) = app.net.as_mut() else { return };
    let now = ctx.input(|i| i.time);
    let server = net.base_url.clone();
    let poll = STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.server != server {
            *s = State { server, ..State::default() };
        }
        let due = s.polled_at.is_none_or(|t| now - t >= EVERY);
        if due {
            s.polled_at = Some(now);
        }
        due
    });
    if poll && !net.is_loading(KEY) {
        net.get(KEY, PATH);
    }
    ctx.request_repaint_after(std::time::Duration::from_secs_f64(EVERY));

    // A task was just opened, from anywhere: what it was telling you is read.
    let opened = app.task.clone();
    if STATE.with(|s| std::mem::replace(&mut s.borrow_mut().opened, opened.clone())) != opened {
        if let Some(task) = opened {
            // ponytail: only the newest 50 the bell holds; older unread ones stay.
            let ids: Vec<i64> = items(app)
                .0
                .iter()
                .filter(|n| str_of(n, "taskId") == Some(task.as_str()))
                .filter(|n| n.get("unread").and_then(Value::as_bool).unwrap_or(false))
                .filter_map(|n| n.get("id").and_then(Value::as_i64))
                .collect();
            if !ids.is_empty() {
                let net = app.net.as_mut().expect("signed in");
                net.invalidate(READ_KEY);
                net.post(READ_KEY, "/api/user/notifications/read", json!({ "ids": ids }));
            }
        }
    }
    let net = app.net.as_mut().expect("signed in");

    // A mark-read landed: read the list again so the counts follow.
    if !net.is_loading(READ_KEY) && net.peek(READ_KEY).is_some() {
        net.invalidate(READ_KEY);
        net.get(KEY, PATH);
    }

    let generation = net.generation(KEY);
    let fresh = STATE.with(|s| {
        let mut s = s.borrow_mut();
        (s.seen_gen != generation).then(|| {
            s.seen_gen = generation;
        })
    });
    if fresh.is_none() {
        return;
    }
    let (list, _) = items(app);
    let top = list.iter().filter_map(|n| n.get("id").and_then(Value::as_i64)).max();
    let newest = STATE.with(|s| s.borrow().newest);
    let Some(seen) = newest else {
        STATE.with(|s| s.borrow_mut().newest = top.or(Some(0)));
        return;
    };
    let new: Vec<&Value> = list
        .iter()
        .filter(|n| n.get("id").and_then(Value::as_i64).is_some_and(|id| id > seen))
        .filter(|n| n.get("unread").and_then(Value::as_bool).unwrap_or(false))
        .collect();
    if let Some(top) = top {
        STATE.with(|s| s.borrow_mut().newest = Some(top.max(seen)));
    }
    if new.len() > 3 {
        w::popup(ctx, w::Popup::new(w::PopTone::Info, format!("{} new notifications", new.len())).detail("Open the bell to see them."));
        return;
    }
    for n in new.iter().rev() {
        let (title, _, _) = words(n);
        let task = str_of(n, "taskTitle").unwrap_or("A task");
        let detail = match excerpt(n) {
            Some(e) => format!("{task} \u{00B7} {e}"),
            None => task.to_owned(),
        };
        let tone = match str_of(n, "kind") {
            Some("question" | "plan" | "submitted") => w::PopTone::Agent,
            _ => w::PopTone::Info,
        };
        let mut p = w::Popup::new(tone, title).detail(detail);
        if let Some(id) = str_of(n, "taskId") {
            p = p.open_task("Open task", id);
        }
        w::popup(ctx, p);
    }
}

/// The bell, at the far right of the toolbar, with the unread count.
pub fn bell(app: &App, ui: &mut egui::Ui) {
    let (_, unread) = items(app);
    let open = STATE.with(|s| s.borrow().open);
    let side = size::CONTROL;
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), Sense::click());
    let label = if unread > 0 { format!("Notifications, {unread} unread") } else { "Notifications".to_owned() };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    let hot = response.hovered() || response.has_focus();
    let p = ui.painter();
    if hot || open {
        p.rect_filled(rect, radius::MD as f32, colour::SURFACE_HOVER());
    }
    p.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        if open { icon::BELL_RINGING } else { icon::BELL },
        egui::FontId::proportional(text::HEADING),
        if hot || open { colour::TEXT() } else { colour::TEXT_2() },
    );
    if unread > 0 {
        let n = if unread > 9 { "9+".to_owned() } else { unread.to_string() };
        let g = p.layout_no_wrap(n, egui::FontId::new(text::CAPTION * 0.85, egui::FontFamily::Name(theme::SEMIBOLD.into())), colour::ON_ACCENT());
        let w_ = (g.size().x + 8.0).max(16.0);
        let pill = egui::Rect::from_center_size(rect.right_top() + egui::vec2(-6.0, 7.0), egui::vec2(w_, 16.0));
        p.rect_filled(pill, 8.0, colour::ACCENT());
        p.galley(pill.center() - g.size() / 2.0, g, colour::ON_ACCENT());
    }
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if response.on_hover_text("Notifications").clicked() {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.open = !s.open;
        });
    }
}

/// The panel, when the bell has it open. After the page, so it sits on top.
pub fn panel(app: &mut App, ctx: &egui::Context) {
    let (open, unread_only) = STATE.with(|s| {
        let s = s.borrow();
        (s.open, s.unread_only)
    });
    let t = ctx.animate_bool_with_time(egui::Id::new("notifications:panel"), open, 0.18);
    if t <= 0.001 {
        return;
    }
    let (list, unread) = items(app);
    let loading = app.net.as_ref().is_some_and(|n| n.data(KEY).is_none() && n.is_loading(KEY));
    let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let mut open_task: Option<(String, Option<i64>)> = None;
    let mut mark_all = false;
    let mut flip_filter: Option<bool> = None;

    let height = (ctx.content_rect().height() - size::TOOLBAR - space::LG * 2.0).min(620.0);
    let area = egui::Area::new(egui::Id::new("notifications:area"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-space::LG + (1.0 - t) * 24.0, size::TOOLBAR + space::XS))
        .show(ctx, |ui| {
            ui.set_opacity(t);
            egui::Frame::new()
                .fill(colour::SURFACE())
                .stroke(egui::Stroke::new(1.0, colour::LINE_STRONG()))
                .corner_radius(radius::LG)
                .shadow(egui::epaint::Shadow { offset: [0, 10], blur: 32, spread: 0, color: egui::Color32::from_black_alpha(110) })
                .inner_margin(egui::Margin::same(0))
                .show(ui, |ui| {
                    ui.set_width(PANEL_W);
                    ui.set_max_height(height);
                    // ---- header
                    egui::Frame::new().inner_margin(egui::Margin::symmetric(space::MD as i8, space::SM as i8)).show(ui, |ui| {
                        ui.set_width(PANEL_W - space::MD * 2.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = space::XS;
                            ui.label(RichText::new("Notifications").size(text::BODY).family(egui::FontFamily::Name(theme::SEMIBOLD.into())).color(colour::TEXT()));
                            if unread > 0 {
                                ui.label(RichText::new(format!("{unread} new")).size(text::CAPTION).color(colour::ACCENT()));
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.spacing_mut().item_spacing.x = space::SM;
                                let x = ui.add(egui::Button::new(RichText::new(icon::X).size(text::SMALL).color(colour::TEXT_MUTED())).frame(false));
                                x.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Close notifications"));
                                close |= x.clicked();
                                if unread > 0 && w::link(ui, "Mark all read").clicked() {
                                    mark_all = true;
                                }
                            });
                        });
                        ui.add_space(space::XS);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = space::XS;
                            for (label, only) in [("All", false), ("Unread", true)] {
                                let on = unread_only == only;
                                let r = ui.add(
                                    egui::Button::new(RichText::new(label).size(text::CAPTION).color(if on { colour::TEXT() } else { colour::TEXT_MUTED() }))
                                        .fill(if on { colour::SURFACE_ACTIVE() } else { colour::TRANSPARENT() })
                                        .stroke(egui::Stroke::NONE)
                                        .corner_radius(radius::SM),
                                );
                                if r.clicked() {
                                    flip_filter = Some(only);
                                }
                            }
                        });
                    });
                    let (rule, _) = ui.allocate_exact_size(egui::vec2(PANEL_W, 1.0), Sense::hover());
                    ui.painter().hline(rule.x_range(), rule.center().y, egui::Stroke::new(1.0, colour::LINE()));

                    // ---- the list
                    let shown: Vec<&Value> = list
                        .iter()
                        .filter(|n| !unread_only || n.get("unread").and_then(Value::as_bool).unwrap_or(false))
                        .collect();
                    egui::ScrollArea::vertical().max_height(height - 90.0).auto_shrink([false, true]).show(ui, |ui| {
                        ui.set_width(PANEL_W);
                        if loading {
                            ui.add_space(space::LG);
                            ui.vertical_centered(|ui| w::muted(ui, "Loading\u{2026}"));
                            ui.add_space(space::LG);
                            return;
                        }
                        if shown.is_empty() {
                            ui.add_space(space::XL);
                            ui.vertical_centered(|ui| {
                                ui.label(RichText::new(icon::BELL_SIMPLE_SLASH).size(text::HEADING * 1.4).color(colour::TEXT_FAINT()));
                                ui.add_space(space::XS);
                                w::muted(ui, if unread_only { "You\u{2019}re all caught up." } else { "Nothing yet \u{2014} assignments, notes and your agents\u{2019} questions land here." });
                            });
                            ui.add_space(space::XL);
                            return;
                        }
                        let mut day = String::new();
                        for n in shown {
                            let at = str_of(n, "createdAt").unwrap_or_default();
                            let label = match day_label(at).as_str() {
                                "Today" => "Today".to_owned(),
                                _ => "Earlier".to_owned(),
                            };
                            if label != day {
                                ui.add_space(space::SM);
                                ui.horizontal(|ui| {
                                    ui.add_space(space::MD);
                                    w::caption(ui, &label);
                                });
                                ui.add_space(space::XXS);
                                day = label;
                            }
                            if row(ui, n) {
                                let id = n.get("id").and_then(Value::as_i64);
                                if let Some(task) = str_of(n, "taskId") {
                                    open_task = Some((task.to_owned(), id));
                                }
                            }
                        }
                        ui.add_space(space::SM);
                    });
                });
        });

    // A click outside closes it, but not the click that opened it.
    if open && ctx.input(|i| i.pointer.any_pressed()) {
        if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
            let on_bell_row = pos.y < size::TOOLBAR;
            if !area.response.rect.contains(pos) && !on_bell_row {
                close = true;
            }
        }
    }

    let net = app.net.as_mut().expect("signed in");
    if let Some(only) = flip_filter {
        STATE.with(|s| s.borrow_mut().unread_only = only);
    }
    if mark_all {
        net.invalidate(READ_KEY);
        net.post(READ_KEY, "/api/user/notifications/read", json!({}));
    }
    // Opening it marks the task's notifications read (see `tick`).
    if let Some((task, _)) = open_task {
        app.task = Some(task);
        close = true;
    }
    if close {
        STATE.with(|s| s.borrow_mut().open = false);
    }
}

/// One notification: its mark, what happened, on which task, when. The whole
/// row opens the task. Returns whether it was clicked.
fn row(ui: &mut egui::Ui, n: &Value) -> bool {
    let unread = n.get("unread").and_then(Value::as_bool).unwrap_or(false);
    let (title, glyph, ink) = words(n);
    let task = str_of(n, "taskTitle").unwrap_or("A task");
    let project = str_of(n, "projectName");
    let excerpt = excerpt(n);
    let at = str_of(n, "createdAt").unwrap_or_default();
    let height = if excerpt.is_some() { 68.0 } else { 52.0 };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(PANEL_W, height), Sense::click());
    let hot = response.hovered() || response.has_focus();
    let p = ui.painter();
    if hot {
        p.rect_filled(rect.shrink2(egui::vec2(space::XS, 0.0)), radius::MD as f32, colour::SURFACE_HOVER());
    }
    // The mark.
    let mark = egui::pos2(rect.left() + space::MD + 14.0, rect.top() + space::SM + 14.0);
    p.circle_filled(mark, 14.0, ink.gamma_multiply(0.16));
    p.text(mark, egui::Align2::CENTER_CENTER, glyph, egui::FontId::proportional(text::SMALL), ink);
    // Unread: a dot at the right edge.
    let right = rect.right() - space::MD;
    if unread {
        p.circle_filled(egui::pos2(right - 3.0, rect.top() + space::SM + 8.0), 4.0, colour::ACCENT());
    }
    let x = mark.x + 14.0 + space::SM;
    let room = right - x - space::MD;
    let when = p.layout_no_wrap(ago(at), egui::FontId::proportional(text::CAPTION), colour::TEXT_FAINT());
    let head = w::truncated(
        ui,
        &title,
        egui::FontId::new(text::SMALL, egui::FontFamily::Name(if unread { theme::SEMIBOLD } else { theme::MEDIUM }.into())),
        colour::TEXT(),
        room - when.size().x - space::SM,
    );
    let mut y = rect.top() + space::SM;
    p.galley(egui::pos2(x, y), head.clone(), colour::TEXT());
    p.galley(egui::pos2(right - when.size().x - if unread { 12.0 } else { 0.0 }, y + 1.0), when, colour::TEXT_FAINT());
    y += head.size().y + 2.0;
    let place = match project {
        Some(pr) => format!("{task} \u{00B7} {pr}"),
        None => task.to_owned(),
    };
    let sub = w::truncated(ui, &place, egui::FontId::proportional(text::CAPTION), colour::TEXT_2(), room);
    p.galley(egui::pos2(x, y), sub.clone(), colour::TEXT_2());
    y += sub.size().y + 2.0;
    if let Some(e) = &excerpt {
        let g = w::truncated(ui, e, egui::FontId::proportional(text::CAPTION), colour::TEXT_MUTED(), room);
        p.galley(egui::pos2(x, y), g, colour::TEXT_MUTED());
    }
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let response = response.on_hover_text(exact(at));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("{title}: {task}")));
    response.clicked()
}
