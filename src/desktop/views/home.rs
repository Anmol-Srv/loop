//! Home: the dashboard.
//!
//! The previous version was a column of cards — the right unit for three
//! items and the wrong one for twenty, because you cannot count a column of
//! cards. This is the shape the owner asked for: four visualisations that read
//! at a glance, a filter bar, and a table.
//!
//! Everything arrives in a single `GET /api/user/home`, cached under the key
//! `"home"` — chrome.rs reads that same key for the sidebar badges, so the
//! name is load-bearing.
//!
//! The four cards sit on different data and are honest about it:
//!   * Status and Completed are counted over the tasks the payload actually
//!     carries (mine, plus the unclaimed preview).
//!   * By discipline is the server's per-project rollup, which is workspace-
//!     wide and exact.
//!   * Team load is `team[]`, real counts from one grouped query.
//!
//! Approvals are gone from this page: the inbox owns them, and the table's
//! "Mine" filter covers what used to be the claim list.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Datelike, Local, Utc};
use egui::{Align, Color32, Layout, RichText};
use egui_extras::{Column, TableBuilder};
use serde_json::Value;

use crate::desktop::design::{
    avatar, cards as c, colour, radius, shell, size, space, status_colour, status_label, text,
    theme, tokens, viz, widgets as w,
};
use crate::desktop::App;

const HOME: &str = "home";
const CLAIM: &str = "home:claim";

/// `App` owns no home state, and this view is the only thing that reads these
/// filters, so they live in egui's temp store rather than growing the struct.
const FILTERS: &str = "home:filters";
/// Which row the pointer was over last frame. A table row's own response only
/// exists after its first cell, which is too late to tint that cell.
const HOVER: &str = "home:hover";

/// Days in the completed chart. Seven, because the label says week.
const WEEK: usize = 7;

/// The status vocabulary, in the order it reads on the donut and in the
/// filter menu. `status_label` turns each one into words.
const STATUSES: [&str; 5] = ["done", "in_progress", "in_review", "blocked", "open"];

/// Table geometry. Fixed so the columns line up with the header and with each
/// other; the task column takes whatever is left.
const ROW_H: f32 = 38.0;
const COL_DOT: f32 = 22.0;
const COL_DISCIPLINE: f32 = 88.0;
const COL_STATUS: f32 = 104.0;
const COL_PROJECT: f32 = 150.0;
const COL_PHASE: f32 = 120.0;
const COL_OWNER: f32 = 70.0;
const COL_UPDATED: f32 = 78.0;

/// The four cards agree on a body height so the row reads as a row rather
/// than as four cards that happen to be adjacent.
const VIZ_BODY_H: f32 = 92.0;

/// What the table is filtered to. Every field is "no filter" when unset, so
/// `Default` is the unfiltered view.
#[derive(Clone, Default, PartialEq)]
pub struct State {
    /// Only tasks assigned to me.
    pub mine: bool,
    pub discipline: Option<String>,
    pub status: Option<String>,
    /// A project id, not a name.
    pub project: Option<String>,
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let can_write = app.can_write();
    let my_person_id = me_str(app, "personId");

    let filters_id = egui::Id::new(FILTERS);
    let mut state: State = ui.ctx().data_mut(|d| d.get_temp(filters_id)).unwrap_or_default();

    let net = app.net.as_mut().unwrap();

    // A finished claim has changed rows every other view has cached.
    if net.data(CLAIM).is_some() {
        net.invalidate(CLAIM);
        net.invalidate(HOME);
        net.invalidate_prefix("board:");
        net.invalidate_prefix("task:");
    }
    net.get_once(HOME, "/api/user/home");

    let loading = net.is_loading(HOME);
    let error = net.error(HOME).map(str::to_owned);
    let claim_error = net.error(CLAIM).map(str::to_owned);
    let busy = net.is_loading(CLAIM);
    let home = net.data(HOME).cloned().unwrap_or(Value::Null);

    let projects = list(&home, "projects");
    let team = list(&home, "team");

    // The table is my tasks plus the unclaimed preview, deduplicated. That
    // union is the honest limit of this payload, not a design choice.
    let mut seen: HashSet<&str> = HashSet::new();
    let rows: Vec<&Value> = list(&home, "myTasks")
        .into_iter()
        .chain(list(&home["available"], "first"))
        .filter(|t| str_at(t, "id").is_some_and(|id| seen.insert(id)))
        .collect();

    // A blocker resolves to a title only when the blocking task happens to be
    // in this union. It usually is — you are blocked by work near your own.
    let known: HashMap<&str, &str> = rows
        .iter()
        .filter_map(|t| Some((str_at(t, "id")?, str_at(t, "title")?)))
        .collect();
    let owners: HashMap<&str, &str> = team
        .iter()
        .filter_map(|p| Some((str_at(p, "personId")?, str_at(p, "email")?)))
        .collect();

    let workspace_total: i64 = projects.iter().map(|p| num(p, "total")).sum();
    shell::page_title(
        ui,
        "Overview",
        &format!(
            "{} across {}",
            plural(workspace_total as usize, "task"),
            plural(projects.len(), "project")
        ),
        |_| {},
    );

    if let Some(err) = &claim_error {
        w::error(ui, &format!("That did not go through. {err}"));
        ui.add_space(space::MD);
    }

    if let Some(err) = &error {
        w::error(ui, err);
        return;
    }
    if loading && rows.is_empty() {
        w::loading(ui, "Loading");
        return;
    }

    // ---- the four figures
    ui.columns(4, |cols| {
        status_card(&mut cols[0], &rows);
        discipline_card(&mut cols[1], &projects);
        completed_card(&mut cols[2], &rows);
        team_card(&mut cols[3], &team);
    });
    ui.add_space(space::XL);

    // The count is drawn from last frame's filter state; a click repaints, so
    // the lag is never seen.
    let shown: Vec<&Value> = rows
        .iter()
        .copied()
        .filter(|t| keep(t, &state, &my_person_id))
        .collect();
    filter_bar(ui, &mut state, &rows, &projects, shown.len());
    ui.ctx().data_mut(|d| d.insert_temp(filters_id, state));

    let mut open_task: Option<String> = None;
    let mut claim: Option<String> = None;

    if shown.is_empty() {
        w::empty(
            ui,
            "Nothing matches those filters.",
            "Clear one of them, or claim something that has no owner yet.",
        );
    } else {
        table(ui, &shown, &known, &owners, can_write && !busy, &mut open_task, &mut claim);
    }
    ui.add_space(space::XXL);

    if let Some(id) = claim {
        net.post(CLAIM, &format!("/api/user/tasks/{id}/claim"), Value::Null);
    }
    if let Some(id) = open_task {
        app.task = Some(id);
    }
}

// ------------------------------------------------------------------- figures

/// Status as a ring. The centre carries the only number worth reading from
/// across the room: how much of this is finished.
fn status_card(ui: &mut egui::Ui, rows: &[&Value]) {
    let count = |s: &str| rows.iter().filter(|t| bucket(t) == s).count();
    let total = rows.len();
    let done = count("done");
    let pct = if total == 0 { 0 } else { done * 100 / total };

    viz::card(ui, "Status", &plural(total, "task"), |ui| {
        ui.set_min_height(VIZ_BODY_H);
        let slices: Vec<viz::Slice<'_>> = STATUSES
            .iter()
            .zip(LABELS)
            .map(|(status, label)| viz::Slice {
                label,
                count: count(status),
                colour: donut_tint(status),
            })
            .collect();
        viz::donut(ui, &slices, &format!("{pct}%"), "DONE");
    });
}

/// Sentence-cased status names, positionally matched to `STATUSES`. Built once
/// rather than capitalising in the render loop.
const LABELS: [&str; 5] = ["Done", "In progress", "In review", "Blocked", "Open"];

/// Progress per discipline, from the server's per-project rollup — the one
/// number on this page that covers every task, not just the ones on screen.
fn discipline_card(ui: &mut egui::Ui, projects: &[&Value]) {
    // Sum the rollups across projects, keeping first-seen order so the list
    // does not reshuffle between refreshes.
    let mut order: Vec<String> = Vec::new();
    let mut tally: HashMap<String, (i64, i64)> = HashMap::new();
    let (mut all_done, mut all_total) = (0, 0);

    for p in projects {
        all_done += num(p, "done");
        all_total += num(p, "total");
        for d in list(p, "disciplines") {
            let Some(name) = str_at(d, "discipline") else { continue };
            let slot = tally.entry(name.to_owned()).or_insert_with(|| {
                order.push(name.to_owned());
                (0, 0)
            });
            slot.0 += num(d, "done");
            slot.1 += num(d, "total");
        }
    }

    // Whatever the disciplines do not account for is unlabelled work. It is
    // real, so it gets a row rather than quietly vanishing from the total.
    let labelled: i64 = order.iter().filter_map(|d| tally.get(d)).map(|(_, t)| t).sum();
    let labelled_done: i64 = order.iter().filter_map(|d| tally.get(d)).map(|(d, _)| d).sum();
    if all_total > labelled {
        order.push("none".to_owned());
        tally.insert("none".to_owned(), (all_done - labelled_done, all_total - labelled));
    }

    viz::card(ui, "By discipline", "done / total", |ui| {
        ui.set_min_height(VIZ_BODY_H);
        for name in &order {
            let (done, total) = tally.get(name).copied().unwrap_or((0, 0));
            let fill = if total == 0 { 0.0 } else { done as f32 / total as f32 };
            viz::bar_row(
                ui,
                name,
                fill,
                discipline_tint(name),
                &format!("{done}/{total}"),
                tokens::DISCIPLINE_W,
            );
        }
    });
}

/// Completions per day for the last week.
///
/// `updatedAt` is the closest thing the payload has to a completion date, so
/// the buckets are "last touched while done". The delta compares the same
/// measure over the previous week — same assumption, so the comparison holds
/// even where the absolute number is soft.
fn completed_card(ui: &mut egui::Ui, rows: &[&Value]) {
    let today = Local::now().date_naive();
    let mut buckets = vec![0.0_f32; WEEK];
    let mut previous = 0usize;

    for t in rows.iter().filter(|t| str_at(t, "status") == Some("done")) {
        let Some(raw) = str_at(t, "updatedAt") else { continue };
        let Ok(when) = DateTime::parse_from_rfc3339(raw) else { continue };
        let days = (today - when.with_timezone(&Local).date_naive()).num_days();
        if (0..WEEK as i64).contains(&days) {
            buckets[WEEK - 1 - days as usize] += 1.0;
        } else if (WEEK as i64..2 * WEEK as i64).contains(&days) {
            previous += 1;
        }
    }

    let total: f32 = buckets.iter().sum();
    let total = total as usize;
    let delta = match total as i64 - previous as i64 {
        _ if total == 0 && previous == 0 => String::new(),
        0 => "same as prev".to_owned(),
        d if d > 0 => format!("+{d} vs prev"),
        d => format!("{d} vs prev"),
    };

    // Day initials, oldest bucket first, so the last column is today.
    let names: Vec<String> = (0..WEEK)
        .map(|i| {
            let day = today - chrono::Duration::days((WEEK - 1 - i) as i64);
            initial(day.weekday()).to_owned()
        })
        .collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();

    viz::card(ui, "Completed", "last 7 days", |ui| {
        ui.set_min_height(VIZ_BODY_H);
        viz::headline(ui, &total.to_string(), &delta);
        ui.add_space(space::MD);
        viz::columns(ui, &buckets, &names, colour::ACCENT);
    });
}

fn initial(day: chrono::Weekday) -> &'static str {
    ["M", "T", "W", "T", "F", "S", "S"][day.num_days_from_monday() as usize]
}

/// Who is carrying what. These are real counts from the server's grouped
/// query, including `blocking` — the number that says who to go and unblock.
fn team_card(ui: &mut egui::Ui, team: &[&Value]) {
    let peak = team.iter().map(|p| num(p, "open")).max().unwrap_or(0);

    viz::card(ui, "Team load", "open", |ui| {
        ui.set_min_height(VIZ_BODY_H);
        for p in team {
            let open = num(p, "open");
            let blocking = num(p, "blocking");
            let fill = if peak == 0 { 0.0 } else { open as f32 / peak as f32 };
            viz::bar_row(
                ui,
                first_name(str_at(p, "name").unwrap_or_default()),
                fill,
                if blocking > 0 { colour::DANGER } else { colour::ACCENT },
                &open.to_string(),
                tokens::DISCIPLINE_W,
            );
        }

        let note: Vec<String> = team
            .iter()
            .filter(|p| num(p, "blocking") > 0)
            .map(|p| {
                format!(
                    "{} is blocking {}",
                    first_name(str_at(p, "name").unwrap_or_default()),
                    plural(num(p, "blocking") as usize, "task")
                )
            })
            .collect();
        if !note.is_empty() {
            w::caption(ui, &note.join(", "));
        }
    });
}

// ---------------------------------------------------------------- filter bar

fn filter_bar(
    ui: &mut egui::Ui,
    state: &mut State,
    rows: &[&Value],
    projects: &[&Value],
    shown: usize,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::SM;

        if viz::filter(ui, "Mine", state.mine).clicked() {
            state.mine = !state.mine;
        }

        // Only disciplines that actually appear: a menu of empty filters is a
        // menu of ways to get an empty table.
        let mut disciplines: Vec<&str> = rows
            .iter()
            .filter_map(|t| str_at(t, "discipline"))
            .filter(|d| !d.is_empty())
            .collect();
        disciplines.sort_unstable();
        disciplines.dedup();
        menu(
            ui,
            state.discipline.clone().unwrap_or_else(|| "All disciplines".to_owned()),
            state.discipline.is_some(),
            "All disciplines",
            disciplines.iter().map(|d| ((*d).to_owned(), (*d).to_owned())).collect(),
            &mut state.discipline,
        );

        let status_label_now = state
            .status
            .as_deref()
            .map(|s| sentence(status_label(s)))
            .unwrap_or_else(|| "Any status".to_owned());
        menu(
            ui,
            status_label_now,
            state.status.is_some(),
            "Any status",
            STATUSES.iter().map(|s| ((*s).to_owned(), sentence(status_label(s)))).collect(),
            &mut state.status,
        );

        let names: Vec<(String, String)> = projects
            .iter()
            .filter_map(|p| Some((str_at(p, "id")?.to_owned(), str_at(p, "name")?.to_owned())))
            .collect();
        let project_now = state
            .project
            .as_deref()
            .and_then(|id| names.iter().find(|(p, _)| p == id))
            .map(|(_, n)| n.clone())
            .unwrap_or_else(|| "All projects".to_owned());
        menu(
            ui,
            project_now,
            state.project.is_some(),
            "All projects",
            names,
            &mut state.project,
        );

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            w::caption(ui, &format!("{shown} of {}", rows.len()));
        });
    });
    ui.add_space(space::MD);
}

/// One filter control and the popup it opens. `None` is the leading "any"
/// entry, so clearing a filter is the same gesture as setting one.
fn menu(
    ui: &mut egui::Ui,
    label: String,
    active: bool,
    any: &str,
    options: Vec<(String, String)>,
    slot: &mut Option<String>,
) {
    let response = viz::filter(ui, &label, active);
    egui::Popup::menu(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .show(|ui| {
            let entry = RichText::new(any).size(text::BODY);
            if ui.selectable_label(slot.is_none(), entry).clicked() {
                *slot = None;
            }
            for (value, name) in &options {
                let entry = RichText::new(name).size(text::BODY);
                if ui.selectable_label(slot.as_ref() == Some(value), entry).clicked() {
                    *slot = Some(value.clone());
                }
            }
        });
}

fn keep(t: &Value, state: &State, my_person_id: &str) -> bool {
    if state.mine
        && !(str_at(t, "assigneeKind") == Some("person")
            && str_at(t, "assigneePersonId") == Some(my_person_id)
            && !my_person_id.is_empty())
    {
        return false;
    }
    if let Some(d) = &state.discipline {
        if str_at(t, "discipline") != Some(d.as_str()) {
            return false;
        }
    }
    if let Some(s) = &state.status {
        if bucket(t) != s.as_str() {
            return false;
        }
    }
    if let Some(p) = &state.project {
        if str_at(t, "projectId") != Some(p.as_str()) {
            return false;
        }
    }
    true
}

// --------------------------------------------------------------------- table

#[allow(clippy::too_many_arguments)]
fn table(
    ui: &mut egui::Ui,
    rows: &[&Value],
    known: &HashMap<&str, &str>,
    owners: &HashMap<&str, &str>,
    can_claim: bool,
    open_task: &mut Option<String>,
    claim: &mut Option<String>,
) {
    let hover_id = egui::Id::new(HOVER);
    let was: Option<usize> = ui.ctx().data(|d| d.get_temp(hover_id)).flatten();
    let mut now: Option<usize> = None;

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
                .id_salt("home:table")
                .vscroll(false)
                .sense(egui::Sense::click())
                .cell_layout(Layout::left_to_right(Align::Center))
                .column(Column::exact(COL_DOT))
                .column(Column::remainder().at_least(COL_PROJECT).clip(true))
                .column(Column::exact(COL_DISCIPLINE))
                .column(Column::exact(COL_STATUS))
                .column(Column::exact(COL_PROJECT).clip(true))
                .column(Column::exact(COL_PHASE).clip(true))
                .column(Column::exact(COL_OWNER))
                .column(Column::exact(COL_UPDATED))
                .header(size::CONTROL, |mut row| {
                    for name in
                        ["", "Task", "Discipline", "Status", "Project", "Phase", "Owner", "Updated"]
                    {
                        row.col(|ui| {
                            ui.label(
                                RichText::new(name)
                                    .size(text::CAPTION)
                                    .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                                    .color(colour::TEXT_FAINT),
                            );
                        });
                    }
                })
                .body(|mut body| {
                    for (i, t) in rows.iter().enumerate() {
                        body.row(ROW_H, |mut row| {
                            row.set_hovered(was == Some(i));
                            row.set_overline(i > 0);
                            task_row(&mut row, t, known, owners, can_claim && was == Some(i), claim);
                            let response = row.response();
                            if response.hovered() {
                                now = Some(i);
                                response.ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            if response.clicked() {
                                *open_task = str_at(t, "id").map(str::to_owned);
                            }
                        });
                    }
                });

            let band_rect = egui::Rect::from_min_max(
                egui::pos2(ui.max_rect().left() - space::MD, top),
                egui::pos2(ui.max_rect().right() + space::MD, top + size::CONTROL),
            );
            ui.painter().set(
                band,
                egui::Shape::rect_filled(
                    band_rect,
                    egui::CornerRadius {
                        nw: radius::LG,
                        ne: radius::LG,
                        sw: 0,
                        se: 0,
                    },
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

fn task_row(
    row: &mut egui_extras::TableRow<'_, '_>,
    t: &Value,
    known: &HashMap<&str, &str>,
    owners: &HashMap<&str, &str>,
    offer_claim: bool,
    claim: &mut Option<String>,
) {
    let status = bucket(t);
    let discipline = str_at(t, "discipline").unwrap_or_default();

    row.col(|ui| w::dot(ui, status_colour(status)));

    row.col(|ui| {
        ui.label(
            RichText::new(str_at(t, "title").unwrap_or_default())
                .size(text::BODY)
                .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                .color(colour::TEXT),
        );
        // The blocker rides behind the title rather than in its own column:
        // it is a footnote on the task, not a property every row has.
        if status == "blocked" {
            ui.add_space(space::XS);
            w::caption(ui, &format!("\u{21b3} waiting on {}", blocker(t, known)));
        }
    });

    row.col(|ui| {
        if !discipline.is_empty() {
            c::chip(ui, discipline, c::discipline_tone(discipline), false);
        }
    });
    row.col(|ui| {
        c::chip(ui, &sentence(status_label(status)), c::status_tone(status), status != "blocked");
    });
    row.col(|ui| w::caption(ui, str_at(t, "projectName").unwrap_or_default()));
    row.col(|ui| w::caption(ui, str_at(t, "phaseName").unwrap_or_default()));

    row.col(|ui| match owner(t, owners) {
        Some(seed) => {
            avatar::small(ui, &seed, size::AVATAR_SM);
        }
        None if offer_claim => {
            if w::ghost(ui, "Claim").clicked() {
                *claim = str_at(t, "id").map(str::to_owned);
            }
        }
        None => w::caption(ui, "\u{2014}"),
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

/// The avatar seed for whoever holds the task: a teammate's email, or the
/// agent's own name. `None` means nobody has it.
fn owner(t: &Value, owners: &HashMap<&str, &str>) -> Option<String> {
    match str_at(t, "assigneeKind") {
        Some("person") => owners
            .get(str_at(t, "assigneePersonId")?)
            .map(|email| (*email).to_owned()),
        Some("agent") => Some(str_at(t, "claimedBy").unwrap_or("agent").to_owned()),
        _ => None,
    }
}

/// The blocker's title when it is in this payload, otherwise an honest count.
fn blocker(t: &Value, known: &HashMap<&str, &str>) -> String {
    let named = t
        .get("blockedBy")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .find_map(|id| known.get(id).copied());
    match named {
        Some(title) => title.to_owned(),
        None => plural(outstanding(t) as usize, "other task"),
    }
}

// -------------------------------------------------------------------- pieces

/// The state a row is filed under. Unfinished blockers beat the status field:
/// a task marked `in_progress` that is waiting on someone else is not in
/// progress, and filing it as though it were hides the thing to go and unblock.
fn bucket(t: &Value) -> &'static str {
    if outstanding(t) > 0 {
        return "blocked";
    }
    let status = str_at(t, "status").unwrap_or("open");
    STATUSES.iter().copied().find(|s| *s == status).unwrap_or("open")
}

fn outstanding(t: &Value) -> i64 {
    (num(t, "blockersTotal") - num(t, "blockersDone")).max(0)
}

/// The donut's hue per status. Deliberately not `status_colour`: a ring needs
/// its unfinished remainder to read as the track, so "open" is a line colour.
fn donut_tint(status: &str) -> Color32 {
    match status {
        "done" => colour::OK,
        "in_progress" => colour::ACCENT,
        "in_review" => colour::WARN,
        "blocked" => colour::DANGER,
        _ => colour::LINE_STRONG,
    }
}

/// A discipline's bar colour, matching the chip it wears everywhere else.
fn discipline_tint(discipline: &str) -> Color32 {
    match c::discipline_tone(discipline) {
        c::Tone::Agent => colour::AGENT,
        c::Tone::Info => colour::INFO,
        c::Tone::Ok => colour::OK,
        _ => colour::LINE_STRONG,
    }
}

/// "In progress" from "in progress". The vocabulary still comes from
/// `status_label`; this only decides where the sentence starts.
fn sentence(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

fn first_name(name: &str) -> &str {
    name.split_whitespace().next().unwrap_or(name)
}

/// "2d", "4h". A table column has room for two characters, and the exact
/// minute is never the thing being decided.
fn age(raw: &str) -> String {
    let Ok(then) = DateTime::parse_from_rfc3339(raw) else {
        return String::new();
    };
    let seconds = (Utc::now() - then.with_timezone(&Utc)).num_seconds().max(0);
    match seconds {
        s if s < 3600 => format!("{}m", (s / 60).max(1)),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

fn me_str(app: &App, key: &str) -> String {
    app.net
        .as_ref()
        .and_then(|n| n.data("__me"))
        .and_then(|m| m.get(key))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn list<'a>(value: &'a Value, key: &str) -> Vec<&'a Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().collect())
        .unwrap_or_default()
}

fn str_at<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn num(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or(0)
}
