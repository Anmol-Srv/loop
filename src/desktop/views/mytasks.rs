//! My Tasks: everything assigned to the signed-in person, one table per
//! project.
//!
//! This was a column of cards, and cards are the wrong unit for "what am I
//! holding": you cannot compare a column of cards, and eleven of them is a
//! scroll. It is the same table vocabulary as Home and Projects — column
//! consts, a header band, hover from a temp store, keyboard-reachable rows —
//! broken into one table per project, because the project is the thing you
//! context-switch between and a single flat list buries it in a column.
//!
//! Finished work sinks to the bottom of its group and is hidden until asked
//! for. This screen answers "what now"; yesterday's done pile is noise in that
//! answer, but it is the first thing you want when someone asks what you did.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use egui::{Align, Layout, RichText};
use egui_extras::{Column, TableBuilder};
use serde_json::Value;

use crate::desktop::design::{
    cards as c, colour, motion, radius, shell, size, space, status_label, text, theme, viz,
    widgets as w,
};
use crate::desktop::App;

/// The personal list. Prefixed `mytasks` so any view that moves a task can drop
/// it with one `invalidate_prefix`.
const MINE: &str = "mytasks:mine";
/// Whether finished rows are shown. Which project the pointer is over gets its
/// own id per group, derived from this one.
const FINISHED: &str = "mytasks:finished";
const HOVER: &str = "mytasks:hover";

// ---- table geometry. Fixed so every group's columns line up with every
// ---- other's; the description takes whatever is left.
const ROW_H: f32 = 38.0;
/// The title is what is being scanned for, so it gets the widest fixed column;
/// past this it truncates rather than pushing the row around.
const COL_TASK: f32 = 300.0;
/// The description's floor. Below this a one-liner is cut to nothing useful.
const COL_DESCRIPTION: f32 = 180.0;
/// "P0" in a chip.
const COL_PRIORITY: f32 = 52.0;
const COL_STATUS: f32 = 104.0;
const COL_UPDATED: f32 = 78.0;

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    // ponytail: one flag, so it lives in egui's temp store rather than growing
    // `App`. Move it onto `App` the day another view needs to read it.
    let finished_id = egui::Id::new(FINISHED);
    let mut show_finished: bool = ui.ctx().data(|d| d.get_temp(finished_id)).unwrap_or(false);

    let net = app.net.as_mut().unwrap();
    net.get_once(MINE, "/api/user/tasks/mine");

    let loading = net.is_loading(MINE);
    let error = net.error(MINE).map(str::to_owned);
    let tasks = net.data(MINE).cloned().unwrap_or(Value::Null);
    let rows: Vec<&Value> = tasks.as_array().map(|a| a.iter().collect()).unwrap_or_default();

    // The subtitle counts the work that is still live, and the projects that
    // work is in — not every project that has ever held a row of mine.
    let open: Vec<&&Value> = rows.iter().filter(|t| !finished(t)).collect();
    let mut open_projects: Vec<&str> = open.iter().map(|t| project_of(t)).collect();
    open_projects.sort_unstable();
    open_projects.dedup();

    shell::page_title(
        ui,
        "My Tasks",
        &format!(
            "{} open across {}",
            open.len(),
            plural(open_projects.len(), "project")
        ),
        |_| {},
    );

    if let Some(err) = &error {
        w::error(ui, &format!("Could not load your work. {err}"));
        return;
    }
    if rows.is_empty() {
        if loading {
            w::loading(ui, "your work");
        } else {
            w::empty(
                ui,
                "Nothing assigned to you.",
                "When someone hands you a task it shows up here, grouped by project.",
            );
        }
        return;
    }

    if viz::filter(ui, "Show finished", show_finished, false).clicked() {
        show_finished = !show_finished;
    }
    ui.ctx().data_mut(|d| d.insert_temp(finished_id, show_finished));

    // Grouped by project name, alphabetically — `BTreeMap` is the sort. Inside
    // a group the finished work sinks: a stable sort on one bit keeps the
    // server's order within each half.
    let mut groups: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
    for t in &rows {
        if !show_finished && finished(t) {
            continue;
        }
        groups.entry(project_of(t)).or_default().push(t);
    }
    for rows in groups.values_mut() {
        rows.sort_by_key(|t| finished(t));
    }

    if groups.is_empty() {
        w::empty(
            ui,
            "Nothing open.",
            "Everything assigned to you is finished \u{2014} show finished work to see it.",
        );
        return;
    }

    let mut open_task: Option<String> = None;
    for (project, rows) in &groups {
        shell::section_count(ui, project, rows.len());
        table(ui, project, rows, &mut open_task);
    }
    ui.add_space(space::XXL);

    if let Some(id) = open_task {
        app.task = Some(id);
    }
}

// --------------------------------------------------------------------- table

fn table(ui: &mut egui::Ui, salt: &str, rows: &[&Value], open_task: &mut Option<String>) {
    let hover_id = egui::Id::new(HOVER).with(salt);
    let was: Option<usize> = ui.ctx().data(|d| d.get_temp(hover_id)).flatten();
    let mut now: Option<usize> = None;
    let mut responses: Vec<egui::Response> = Vec::with_capacity(rows.len());

    egui::Frame::new()
        .fill(colour::SURFACE)
        .stroke(egui::Stroke::new(1.0, colour::LINE))
        .corner_radius(radius::LG)
        .inner_margin(egui::Margin::symmetric(space::MD as i8, 0))
        .show(ui, |ui| {
            // Reserved now, painted once the header's extent is known: the
            // band has to sit under the header text, not over it.
            let band = ui.painter().add(egui::Shape::Noop);
            let top = ui.cursor().top();
            ui.spacing_mut().item_spacing = egui::Vec2::new(space::MD, 0.0);

            TableBuilder::new(ui)
                .id_salt(salt)
                .vscroll(false)
                .sense(egui::Sense::click())
                .cell_layout(Layout::left_to_right(Align::Center))
                .column(Column::exact(COL_TASK).clip(true))
                .column(Column::remainder().at_least(COL_DESCRIPTION).clip(true))
                .column(Column::exact(COL_PRIORITY))
                .column(Column::exact(COL_STATUS))
                .column(Column::exact(COL_UPDATED))
                .header(size::CONTROL, |mut row| {
                    for name in ["Task", "Description", "Priority", "Status", "Updated"] {
                        row.col(|ui| {
                            ui.label(
                                RichText::new(name)
                                    .size(text::CAPTION)
                                    .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                                    .color(colour::TEXT_MUTED),
                            );
                        });
                    }
                })
                .body(|mut body| {
                    for (i, t) in rows.iter().enumerate() {
                        body.row(ROW_H, |mut row| {
                            row.set_hovered(was == Some(i));
                            row.set_overline(i > 0);
                            task_row(&mut row, t);
                            responses.push(row.response());
                        });
                    }
                });

            // A row is hand-painted, so Tab does not reach it on its own.
            // Handled after the table rather than inside the body closure,
            // which holds the only `Ui` the focus ring can be drawn on.
            for (i, response) in responses.into_iter().enumerate() {
                let response = motion::operable_sm(ui, response);
                if response.hovered() {
                    now = Some(i);
                    response.ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if response.clicked() {
                    *open_task = str_at(rows[i], "id").map(str::to_owned);
                }
            }

            let band_rect = egui::Rect::from_min_max(
                egui::pos2(ui.max_rect().left() - space::MD, top),
                egui::pos2(ui.max_rect().right() + space::MD, top + size::CONTROL),
            );
            ui.painter().set(
                band,
                egui::Shape::rect_filled(
                    band_rect,
                    egui::CornerRadius { nw: radius::LG, ne: radius::LG, sw: 0, se: 0 },
                    colour::CHROME,
                ),
            );
            ui.painter().hline(
                band_rect.x_range(),
                band_rect.bottom(),
                egui::Stroke::new(1.0, colour::LINE),
            );
        });

    ui.ctx().data_mut(|d| d.insert_temp(hover_id, now));
}

fn task_row(row: &mut egui_extras::TableRow<'_, '_>, t: &Value) {
    let status = str_at(t, "status").unwrap_or("open");
    let done = finished(t);

    row.col(|ui| {
        ui.label(
            RichText::new(str_at(t, "title").unwrap_or_default())
                .size(text::BODY)
                .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                // Finished work stays legible and stops competing: the rows
                // above it are the ones with something to decide.
                .color(if done { colour::TEXT_MUTED } else { colour::TEXT }),
        );
        // Blocked rides behind the title rather than replacing the status: the
        // status is still true, the blocker is the reason it is not moving.
        if blocked(t) {
            ui.add_space(space::XS);
            c::chip(ui, "blocked", c::Tone::Blocked, false);
        }
    });

    row.col(|ui| {
        let body = str_at(t, "body").unwrap_or_default().trim();
        let (copy, ink) = match body.lines().next() {
            Some(first) if !first.is_empty() => (first, colour::TEXT_MUTED),
            // One line. The column clips, and a wrapped cell would make one
            // row taller than the rest of the table.
            _ => ("\u{2014}", colour::TEXT_FAINT),
        };
        ui.label(RichText::new(copy).size(text::SMALL).color(ink));
    });

    row.col(|ui| {
        if let Some(p) = t.get("priority").and_then(Value::as_i64) {
            c::chip(ui, &format!("P{p}"), priority_tone(p), false);
        }
    });

    row.col(|ui| {
        c::chip(ui, &sentence(status_label(status)), c::status_tone(status), true);
    });

    row.col(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(age(str_at(t, "updatedAt").unwrap_or_default()))
                    .size(text::SMALL)
                    .color(colour::TEXT_MUTED),
            );
        });
    });
}

// -------------------------------------------------------------------- pieces

/// P0 shouts and P4 whispers, in the same chip vocabulary as status.
fn priority_tone(p: i64) -> c::Tone {
    match p {
        0 => c::Tone::Blocked,
        1 => c::Tone::Running,
        2 => c::Tone::Neutral,
        _ => c::Tone::Quiet,
    }
}

fn blocked(t: &Value) -> bool {
    num(t, "blockersDone") < num(t, "blockersTotal")
}

fn finished(t: &Value) -> bool {
    matches!(str_at(t, "status"), Some("done") | Some("dropped"))
}

/// The group a task is filed under. A task always has a project; the fallback
/// only exists so a malformed row still lands somewhere visible.
fn project_of<'a>(t: &'a Value) -> &'a str {
    match str_at(t, "projectName") {
        Some(name) if !name.is_empty() => name,
        _ => "No project",
    }
}

/// "2d", "4h". A table column has room for two characters, and the exact
/// minute is never the thing being decided.
fn age(raw: &str) -> String {
    let Ok(then) = DateTime::parse_from_rfc3339(raw) else {
        return String::new();
    };
    match (Utc::now() - then.with_timezone(&Utc)).num_seconds().max(0) {
        s if s < 3600 => format!("{}m", (s / 60).max(1)),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// "In review" from "in review". The vocabulary still comes from
/// `status_label`; this only decides where the sentence starts.
fn sentence(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn str_at<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn num(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or(0)
}
