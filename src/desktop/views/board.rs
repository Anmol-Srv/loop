//! Board: the project list, and inside a project its flow, phases and tasks.
//!
//! Two modes, chosen by `app.project`. The project detail screen opens with
//! what the project *is* — name, a meta line of three facts, the description —
//! and only then how far along it is. The list screen answers "where is the
//! work"; someone who has clicked into one project is asking the slower
//! question, and prose at the top is the answer to it.
//!
//! Under that, the flow strip: done/total per discipline. It reports, it does
//! not gate — a discipline may run ahead of the one to its left, so there is
//! no arrow implying an order the data does not have.
//!
//! Below the strip everything is a card: a task is a `c::task_card`, and an
//! unclaimed task is a slim card with a Claim button — the same shape the
//! claim zone has on My tasks. The detail header itself is deliberately not a
//! card; a card inside a page is a box around the page's own subject.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::desktop::design::tokens::{discipline_colour, DISCIPLINE_W};
use crate::desktop::design::{
    avatar, cards as c, colour, shell, size, space, status_label, text, theme, viz,
    widgets as w,
};
use crate::desktop::App;

const STATUSES: [&str; 6] = ["open", "in_progress", "in_review", "blocked", "done", "dropped"];
const KINDS: [&str; 2] = ["human", "agent"];

/// The usual order of the flow. Anything the server reports that is not in
/// here keeps its own order, after these — a new discipline should appear
/// rather than vanish because this list has not caught up.
const FLOW_ORDER: [&str; 3] = ["design", "frontend", "backend"];

/// Where a claim's reply is collected.
const CLAIM_KEY: &str = "board:claim";
const PROJECTS_KEY: &str = "board:projects";
const PEOPLE_KEY: &str = "board:people";
/// Where a create's reply is collected.
const CREATE_KEY: &str = "board:create";
/// Content width at which the project grid goes from two across to three.
/// Below this a three-up card cannot fit a title and a status chip on one line.
const GRID_THREE_AT: f32 = 820.0;
/// The card body's height, inside the padding. Fixed, because every card
/// carries the same lines; a description that would need more is cut to
/// `DESCRIPTION_ROWS`, not given a taller card.
const PROJECT_CARD_H: f32 = 120.0;
const DESCRIPTION_ROWS: usize = 2;
/// How far each avatar in the roster sits over the one before it.
const AVATAR_OVERLAP: f32 = 6.0;
/// Past this the roster shows a "+N" instead of more faces.
const MAX_AVATARS: usize = 5;
/// Room a status chip needs at the end of the title row. The title truncates
/// before it, never under it.
const STATUS_CHIP_W: f32 = 96.0;
/// The prose measure for a project description: ~70 characters at `text::BODY`,
/// which is where a paragraph stops needing a finger to track the line.
const PROSE_W: f32 = 640.0;

/// What the create form holds. `None` on `State::creating` means the form is
/// closed, which is also how the Create project button knows not to redraw
/// itself over an open form.
#[derive(Default)]
pub struct Draft {
    pub title: String,
    pub description: String,
    /// Person ids, in the order they were picked. Empty is a real answer.
    pub members: Vec<String>,
}

#[derive(Default)]
pub struct State {
    /// `None` means "any"; both filters go to the server as query params.
    pub status: Option<&'static str>,
    pub assignee_kind: Option<&'static str>,
    /// A claim is out; its reply invalidates the board when it lands.
    pub claiming: bool,
    /// The open create form, if there is one.
    pub creating: Option<Draft>,
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    match app.project.clone() {
        None => projects(app, ui),
        Some(project_id) => project(app, ui, &project_id),
    }
}

// ---------------------------------------------------------------- project list

fn projects(app: &mut App, ui: &mut egui::Ui) {
    let can_write = app.can_write();
    let net = app.net.as_mut().unwrap();

    // A finished create has invalidated the list, the sidebar counts and every
    // rollup that counts projects.
    if net.data(CREATE_KEY).is_some() {
        net.invalidate(CREATE_KEY);
        net.invalidate(PROJECTS_KEY);
        net.invalidate("home");
        app.board.creating = None;
    }
    let net = app.net.as_mut().unwrap();

    net.get_once(PROJECTS_KEY, "/api/user/projects");
    // The assignee picker needs names, and the cards need them to resolve
    // `memberIds`. One fetch serves both.
    net.get_once(PEOPLE_KEY, "/api/user/people");

    let list = array(net.data(PROJECTS_KEY));
    let people = array(net.data(PEOPLE_KEY));
    let loading = net.is_loading(PROJECTS_KEY);
    let error = net.error(PROJECTS_KEY).map(str::to_string);
    let create_error = net.error(CREATE_KEY).map(str::to_string);
    let creating = net.is_loading(CREATE_KEY);

    // One `/flow` per project — cached, so once per project per session. A
    // project with no tasks still answers, with zeroes.
    let mut flows: HashMap<String, Value> = HashMap::new();
    for p in &list {
        let id = str_at(p, "id");
        if id.is_empty() {
            continue;
        }
        let key = format!("board:flow:{id}");
        net.get_once(&key, &format!("/api/user/projects/{id}/flow"));
        if let Some(flow) = net.data(&key) {
            flows.insert(id.to_string(), flow.clone());
        }
    }

    let names: HashMap<String, String> = people
        .iter()
        .map(|p| (str_at(p, "id").to_string(), str_at(p, "name").to_string()))
        .collect();

    let mut start_create = false;
    shell::page_title(
        ui,
        "Projects",
        &subtitle(list.len(), loading),
        |ui| {
            // The form is the button's own state: while it is open the button
            // would only re-open what is already open.
            if app.board.creating.is_none() && w::primary(ui, "Create project", can_write).clicked()
            {
                start_create = true;
            }
        },
    );
    if start_create {
        app.board.creating = Some(Draft::default());
    }

    if let Some(err) = &create_error {
        w::error(ui, err);
        ui.add_space(space::MD);
    }

    let mut submit: Option<Value> = None;
    if let Some(draft) = app.board.creating.as_mut() {
        submit = create_form(ui, draft, &people, creating);
        ui.add_space(space::LG);
    }

    if let Some(err) = error {
        w::error(ui, &err);
        return;
    }
    if list.is_empty() {
        if loading {
            w::loading(ui, "projects");
        } else if app.board.creating.is_none() {
            w::empty(
                ui,
                "No projects yet.",
                "Create one and it will appear here. Phases and tasks come after.",
            );
        }
    }

    // A grid, three across when there is room and two otherwise. Every card
    // is the same shape (title, one line of description, lead, progress), so
    // rows line up without measuring.
    let per_row = if ui.available_width() >= GRID_THREE_AT { 3 } else { 2 };
    let mut open: Option<String> = None;
    for chunk in list.chunks(per_row) {
        ui.columns(per_row, |cols| {
            for (col, p) in cols.iter_mut().zip(chunk) {
                if project_card(col, p, flows.get(str_at(p, "id")), &names) {
                    open = Some(str_at(p, "id").to_string());
                }
            }
        });
        ui.add_space(space::MD);
    }

    let net = app.net.as_mut().unwrap();
    if let Some(body) = submit {
        net.post(CREATE_KEY, "/api/user/projects", body);
    }
    if let Some(id) = open {
        app.project = Some(id);
    }
}

fn subtitle(n: usize, loading: bool) -> String {
    match (n, loading) {
        (0, true) => String::new(),
        (1, _) => "1 project".to_owned(),
        (n, _) => format!("{n} projects"),
    }
}

/// The create form, inline under the header rather than in a modal.
///
/// Returns the request body once, on the frame Create is clicked. A modal was
/// the obvious first thought and the wrong one: this is a three-field form on
/// a page with room for it, and covering the list to ask what to add to the
/// list is a worse trade than pushing it down.
fn create_form(
    ui: &mut egui::Ui,
    draft: &mut Draft,
    people: &[Value],
    busy: bool,
) -> Option<Value> {
    let mut submit = None;
    let mut cancel = false;

    c::surface(ui, false, |ui| {
        ui.set_width(ui.available_width());
        w::heading(ui, "New project");
        ui.add_space(space::MD);

        w::field(ui, "Title", &mut draft.title, false, "Checkout redesign");
        ui.add_space(space::MD);
        w::field_multiline(
            ui,
            "Description",
            &mut draft.description,
            3,
            "What this project is for, and what done looks like.",
        );
        ui.add_space(space::MD);

        w::caption(ui, "Assignees");
        ui.add_space(space::XXS);
        let options: Vec<(String, String)> = people
            .iter()
            .map(|p| (str_at(p, "id").to_string(), str_at(p, "name").to_string()))
            .collect();
        ui.horizontal(|ui| {
            viz::multi_select(ui, "Nobody yet", &options, &mut draft.members);
        });
        ui.add_space(space::LG);

        // A project with no title has nothing to be called and nothing to
        // derive a key from, so Create stays off until there is one.
        let ready = !draft.title.trim().is_empty() && !busy;
        ui.horizontal(|ui| {
            if w::primary(ui, if busy { "Creating…" } else { "Create" }, ready).clicked() {
                submit = Some(serde_json::json!({
                    "name": draft.title.trim(),
                    "description": draft.description.trim(),
                    "memberIds": draft.members,
                }));
            }
            ui.add_space(space::XS);
            if w::ghost(ui, "Cancel").clicked() {
                cancel = true;
            }
        });
    });

    if cancel {
        *draft = Draft::default();
        return None;
    }
    submit
}

/// One project. Returns true when it was clicked into.
///
/// Fixed-shape, so a row of these lines up: name and status, two lines of
/// description, then a footer of who is on it and how far along it is. The
/// budget is `PROJECT_CARD_H`; the footer is placed from the top of the card,
/// never from the bottom of the page — the first version asked egui how much
/// height was left and got the rest of the window.
fn project_card(
    ui: &mut egui::Ui,
    p: &Value,
    flow: Option<&Value>,
    names: &HashMap<String, String>,
) -> bool {
    let hover_id = ui.next_auto_id();
    let hovered = ui.ctx().data(|d| d.get_temp::<bool>(hover_id).unwrap_or(false));
    let (done, total) = flow.map(|f| (num_at(f, "done"), num_at(f, "total"))).unwrap_or((0, 0));
    let status = str_at(p, "status");

    let out = c::surface(ui, hovered, |ui| {
        ui.set_width(ui.available_width());
        let top = ui.max_rect().top();

        ui.horizontal(|ui| {
            w::row_title(ui, str_at(p, "name"), STATUS_CHIP_W);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                c::chip(ui, status_label(status), c::status_tone(status), true);
            });
        });
        ui.add_space(space::XS);

        // Two lines, then an ellipsis. One line lost the sentence; the whole
        // thing would make the card as tall as the description.
        let description = str_at(p, "description");
        let (copy, ink) = if description.is_empty() {
            ("No description", colour::TEXT_FAINT)
        } else {
            (description, colour::TEXT_MUTED)
        };
        let mut job = egui::text::LayoutJob::single_section(
            copy.to_owned(),
            egui::TextFormat {
                font_id: egui::FontId::proportional(text::SMALL),
                color: ink,
                ..Default::default()
            },
        );
        job.wrap = egui::text::TextWrapping {
            max_width: ui.available_width(),
            max_rows: DESCRIPTION_ROWS,
            break_anywhere: false,
            overflow_character: Some('…'),
        };
        ui.label(job);

        // The footer sits at a fixed offset from the top, whatever the
        // description needed.
        let footer_h = size::AVATAR_SM + space::SM + space::XS;
        let used = ui.cursor().top() - top;
        let slack = PROJECT_CARD_H - footer_h - used;
        if slack > 0.0 {
            ui.add_space(slack);
        }

        ui.horizontal(|ui| {
            let members: Vec<&str> = p
                .get("memberIds")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .filter_map(|id| names.get(id).map(String::as_str))
                        .collect()
                })
                .unwrap_or_default();
            if members.is_empty() {
                w::caption(ui, "Nobody assigned");
            }
            // Overlapping, like a stack of people, so five fit where three
            // would side by side. Names come back on hover.
            ui.spacing_mut().item_spacing.x = -AVATAR_OVERLAP;
            for name in members.iter().take(MAX_AVATARS) {
                let r = avatar::small(ui, name, size::AVATAR_SM).on_hover_text(*name);
                // A ring in the card's own colour, so each face cuts out of
                // the one beneath rather than bleeding into it.
                ui.painter().circle_stroke(
                    r.rect.center(),
                    size::AVATAR_SM / 2.0,
                    egui::Stroke::new(1.5, colour::SURFACE),
                );
            }
            ui.spacing_mut().item_spacing.x = space::SM;
            if members.len() > MAX_AVATARS {
                ui.add_space(AVATAR_OVERLAP + space::XS);
                w::caption(ui, &format!("+{}", members.len() - MAX_AVATARS));
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if total > 0 {
                    w::caption(ui, &format!("{done} of {total} done"));
                } else {
                    w::caption(ui, "No tasks yet");
                }
            });
        });
        ui.add_space(space::SM);
        w::progress(ui, fraction(done, total), ui.available_width(), colour::ACCENT);
    });

    let hit = out.response.interact(egui::Sense::click());
    ui.ctx().data_mut(|d| d.insert_temp(hover_id, hit.hovered()));
    if hit.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    hit.clicked()
}

// -------------------------------------------------------------- project detail

fn project(app: &mut App, ui: &mut egui::Ui, project_id: &str) {
    let status = app.board.status;
    let kind = app.board.assignee_kind;

    let detail_key = format!("board:project:{project_id}");
    let flow_key = format!("board:flow:{project_id}");
    let phases_key = format!("board:phases:{project_id}");
    let tasks_key = format!(
        "board:tasks:{project_id}:{}:{}",
        status.unwrap_or("any"),
        kind.unwrap_or("any")
    );
    let mut tasks_path = format!("/api/user/tasks?projectId={project_id}");
    if let Some(s) = status {
        tasks_path.push_str(&format!("&status={s}"));
    }
    if let Some(k) = kind {
        tasks_path.push_str(&format!("&assigneeKind={k}"));
    }

    let can_write = app.can_write();
    let net = app.net.as_mut().unwrap();

    // Fold in a claim that has come back, before anything reads the cache. On
    // success the board and the home screen both hold the old assignee; on
    // failure the reply stays put so the row below can show why.
    if app.board.claiming && !net.is_loading(CLAIM_KEY) {
        match net.peek(CLAIM_KEY) {
            Some(Ok(_)) => {
                app.board.claiming = false;
                // `board:claim` is itself under this prefix, so the reply is
                // dropped along with the stale lists. That is what we want.
                net.invalidate_prefix("board:");
                net.invalidate("home");
            }
            Some(Err(_)) => app.board.claiming = false,
            None => {}
        }
    }

    net.get_once(&detail_key, &format!("/api/user/projects/{project_id}"));
    net.get_once(&flow_key, &format!("/api/user/projects/{project_id}/flow"));
    net.get_once(&phases_key, &format!("/api/user/projects/{project_id}/phases"));
    net.get_once(&tasks_key, &tasks_path);
    // The roster is ids; the meta line wants faces and names. Same key the
    // list screen fills, so arriving from it costs nothing.
    net.get_once(PEOPLE_KEY, "/api/user/people");

    let detail = net.data(&detail_key).cloned();
    let names: HashMap<String, String> = array(net.data(PEOPLE_KEY))
        .iter()
        .map(|p| (str_at(p, "id").to_string(), str_at(p, "name").to_string()))
        .collect();

    let flow = net.data(&flow_key).cloned();
    let flow_loading = net.is_loading(&flow_key);
    let flow_error = net.error(&flow_key).map(str::to_string);

    let mut phases = array(net.data(&phases_key));
    phases.sort_by_key(|p| p.get("position").and_then(Value::as_i64).unwrap_or(0));
    let phases_loading = net.is_loading(&phases_key);
    let phases_error = net.error(&phases_key).map(str::to_string);

    let tasks = array(net.data(&tasks_key));
    let tasks_loading = net.is_loading(&tasks_key);
    let tasks_error = net.error(&tasks_key).map(str::to_string);
    let claim_error = net.error(CLAIM_KEY).map(str::to_string);
    let busy = net.is_loading(CLAIM_KEY) || app.board.claiming;

    // Tasks arrive for the whole project in one call; bucket them per phase,
    // and index them by id so a blocker can be named rather than numbered.
    let mut by_phase: HashMap<String, Vec<Value>> = HashMap::new();
    let mut titles: HashMap<String, String> = HashMap::new();
    for t in tasks {
        titles.insert(str_at(&t, "id").to_string(), str_at(&t, "title").to_string());
        by_phase.entry(str_at(&t, "phaseId").to_string()).or_default().push(t);
    }

    let mut back = false;
    let mut open_task: Option<String> = None;
    let mut claim: Option<String> = None;
    let mut filters_changed = false;

    if shell::back(ui, "Projects").clicked() {
        back = true;
    }
    // Name, state and identifier on one line. `page_title`'s trailing slot is
    // right-aligned, and a key belongs *to* the name — it reads as an aside
    // beside it, not as a control at the other end of the header.
    let fallback = Value::Null;
    let head = detail.as_ref().or(flow.as_ref()).unwrap_or(&fallback);
    let name = match str_at(head, "name") {
        "" => "Project",
        n => n,
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::SM;
        ui.label(
            egui::RichText::new(name)
                .size(text::TITLE)
                .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                .color(colour::TEXT),
        );
        let status = str_at(head, "status");
        if !status.is_empty() {
            c::chip(ui, status_label(status), c::status_tone(status), true);
        }
        let key = str_at(head, "key");
        if !key.is_empty() {
            w::mono_caption(ui, key);
        }
    });

    ui.add_space(space::MD);
    let (done, total) = flow
        .as_ref()
        .map(|f| (num_at(f, "done"), num_at(f, "total")))
        .unwrap_or((0, 0));
    meta_line(ui, head, &names, done, total);

    ui.add_space(space::LG);
    description(ui, str_at(head, "description"));

    ui.add_space(space::XL);
    if let Some(err) = flow_error {
        w::error(ui, &err);
    } else {
        match &flow {
            Some(f) => flow_strip(ui, f),
            None if flow_loading => w::loading(ui, "flow"),
            // No flow at all is a fetch that has not landed rather than a
            // project without work, so it says nothing rather than lying.
            None => {}
        }
    }

    ui.add_space(space::XL);
    ui.horizontal(|ui| {
        filters_changed |=
            filter(ui, "board:filter:status", "Any status", &STATUSES, &mut app.board.status);
        ui.add_space(space::SM);
        filters_changed |=
            filter(ui, "board:filter:kind", "Anyone", &KINDS, &mut app.board.assignee_kind);

        if app.board.status.is_some() || app.board.assignee_kind.is_some() {
            ui.add_space(space::SM);
            if w::secondary(ui, "Clear", true).clicked() {
                app.board.status = None;
                app.board.assignee_kind = None;
                filters_changed = true;
            }
        }
    });

    if let Some(err) = claim_error {
        ui.add_space(space::SM);
        w::error(ui, &err);
    }
    ui.add_space(space::LG);

    if let Some(err) = phases_error {
        w::error(ui, &err);
    } else if phases.is_empty() {
        if phases_loading {
            w::loading(ui, "phases");
        } else {
            // The common case on a fresh project, so it teaches the next move
            // rather than reporting an absence.
            w::empty(
                ui,
                "No phases yet.",
                "Phases hold the tasks. Add one to start planning this project.",
            );
        }
    } else {
        for p in &phases {
            let phase_id = str_at(p, "id");
            let none = Vec::new();
            let list = by_phase.get(phase_id).unwrap_or(&none);
            let done = list.iter().filter(|t| str_at(t, "status") == "done").count();

            phase_header(ui, p, done, list.len());

            if let Some(err) = &tasks_error {
                w::error(ui, err);
            } else if list.is_empty() {
                if tasks_loading {
                    w::loading(ui, "tasks");
                } else {
                    w::empty(ui, "No tasks in this phase.", "");
                }
            } else {
                for t in list {
                    match task_card(ui, t, &titles, can_write && !busy) {
                        Some(Hit::Open(id)) => open_task = Some(id),
                        Some(Hit::Claim(id)) => claim = Some(id),
                        None => {}
                    }
                }
            }
        }
    }

    if back {
        app.project = None;
    }
    if let Some(id) = open_task {
        app.task = Some(id);
    }
    let net = app.net.as_mut().unwrap();
    if filters_changed {
        net.invalidate_prefix("board:tasks");
    }
    if let Some(id) = claim {
        // Drop a previous claim's error, so the banner belongs to this attempt.
        net.invalidate(CLAIM_KEY);
        net.post(CLAIM_KEY, &format!("/api/user/tasks/{id}/claim"), Value::Null);
        app.board.claiming = true;
    }
}

/// The three facts about a project, in one quiet row: when it started, who is
/// on it, how much of it is done.
///
/// Not a card. Three facts do not need a container — the space around them
/// already groups them, and a box here would be the first of the nested cards
/// this page exists to avoid. Labels are muted, values full-strength: the
/// label is scaffolding you read once, the value is what you came for.
fn meta_line(
    ui: &mut egui::Ui,
    p: &Value,
    names: &HashMap<String, String>,
    done: i64,
    total: i64,
) {
    ui.horizontal(|ui| {
        // Tight inside a fact, generous between them: proximity does the
        // grouping that separators would otherwise have to.
        ui.spacing_mut().item_spacing.x = space::XS;

        if let Some((relative, absolute)) = created(str_at(p, "createdAt")) {
            label(ui, "Created");
            value(ui, &relative).on_hover_text(absolute);
            ui.add_space(space::XL);
        }

        let members: Vec<&str> = p
            .get("memberIds")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .filter_map(|id| names.get(id).map(String::as_str))
                    .collect()
            })
            .unwrap_or_default();
        if members.is_empty() {
            label(ui, "Nobody assigned");
        } else {
            // The same overlapping stack the cards use, so a roster is one
            // shape across the app. The ring is the canvas, not a surface:
            // there is no card behind this row to cut out of.
            ui.spacing_mut().item_spacing.x = -AVATAR_OVERLAP;
            for who in members.iter().take(MAX_AVATARS) {
                let r = avatar::small(ui, who, size::AVATAR_SM).on_hover_text(*who);
                ui.painter().circle_stroke(
                    r.rect.center(),
                    size::AVATAR_SM / 2.0,
                    egui::Stroke::new(1.5, colour::CANVAS),
                );
            }
            ui.spacing_mut().item_spacing.x = space::XS;
            ui.add_space(AVATAR_OVERLAP + space::XS);
            let shown = members.iter().take(MAX_AVATARS).copied().collect::<Vec<_>>().join(", ");
            let rest = members.len().saturating_sub(MAX_AVATARS);
            value(ui, &if rest > 0 { format!("{shown} +{rest}") } else { shown });
        }
        ui.add_space(space::XL);

        if total > 0 {
            value(ui, &format!("{done} of {total}"));
            label(ui, "done");
        } else {
            label(ui, "No tasks yet");
        }
    });
}

/// A meta-line label: the word, not the fact.
fn label(ui: &mut egui::Ui, s: &str) {
    ui.label(egui::RichText::new(s).size(text::SMALL).color(colour::TEXT_MUTED));
}

/// A meta-line value. Full ink — the owner wants the numbers readable at a
/// glance, and a muted figure is one the eye skips.
fn value(ui: &mut egui::Ui, s: &str) -> egui::Response {
    ui.label(egui::RichText::new(s).size(text::SMALL).color(colour::TEXT))
}

/// "3 days ago", with the absolute date for the hover.
///
/// Wordier than `age`, on purpose: a chip repeated down forty rows wants
/// `3d`, a fact you read once wants the sentence.
fn created(ts: &str) -> Option<(String, String)> {
    let then = DateTime::parse_from_rfc3339(ts).ok()?.with_timezone(&Utc);
    let secs = (Utc::now() - then).num_seconds().max(0);
    let relative = match secs {
        s if s < 3600 => "just now".to_owned(),
        s if s < 86_400 => units(s / 3600, "hour"),
        s if s < 2_592_000 => units(s / 86_400, "day"),
        s => units(s / 2_592_000, "month"),
    };
    Some((relative, then.format("%-d %B %Y, %H:%M UTC").to_string()))
}

fn units(n: i64, unit: &str) -> String {
    if n == 1 {
        format!("1 {unit} ago")
    } else {
        format!("{n} {unit}s ago")
    }
}

/// The description, as prose.
///
/// Held to `PROSE_W` rather than the content width: the page is 1080 wide and
/// a paragraph that wide loses the line it is on. Paragraphs are split on the
/// blank line the API sends them with; nothing wraps this in a card, because
/// text on the canvas is already the most readable thing we can do with it.
fn description(ui: &mut egui::Ui, body: &str) {
    let body = body.trim();
    if body.is_empty() {
        w::caption(ui, "No description");
        return;
    }
    ui.scope(|ui| {
        ui.set_max_width(PROSE_W.min(ui.available_width()));
        for (i, para) in body.split("\n\n").map(str::trim).filter(|p| !p.is_empty()).enumerate() {
            if i > 0 {
                ui.add_space(space::MD);
            }
            ui.label(egui::RichText::new(para).size(text::BODY).color(colour::TEXT_2));
        }
    });
}

/// The disciplines of a flow that have any work, in the usual order.
fn flow_columns(flow: &Value) -> Vec<&Value> {
    let mut columns: Vec<&Value> = flow
        .get("disciplines")
        .and_then(Value::as_array)
        .map(|d| d.iter().filter(|d| num_at(d, "total") > 0).collect())
        .unwrap_or_default();
    // Usual order first, then whatever else the server reports.
    columns.sort_by_key(|d| {
        FLOW_ORDER
            .iter()
            .position(|o| *o == str_at(d, "discipline"))
            .unwrap_or(FLOW_ORDER.len())
    });
    columns
}

/// The flow strip: one column per discipline that has tasks.
///
/// It reports, it does not gate — there is no arrow between the columns,
/// because frontend and backend can and do run before design has finished and
/// a glyph saying otherwise was the one piece of this page that was wrong
/// rather than merely plain. No card either: three labelled rules on the
/// canvas are already a group, and the box was drawing a border around them
/// for the sake of having drawn one.
fn flow_strip(ui: &mut egui::Ui, flow: &Value) {
    let columns = flow_columns(flow);
    let full = ui.available_width();

    if columns.is_empty() {
        // A fresh project: an empty bar reads as "nothing yet, and here is
        // where it will show" — a sentence alone reads as a page that failed.
        w::progress(ui, 0.0, full, colour::ACCENT);
        ui.add_space(space::SM);
        w::caption(ui, "No tasks yet");
        return;
    }

    let gaps = space::XL * (columns.len() as f32 - 1.0);
    // The floor stops a column collapsing to nothing; past enough disciplines
    // it makes the strip wider than the page, which is the only case that
    // scrolls. Three fit outright, so today nothing does.
    let width = ((full - gaps) / columns.len() as f32).max(DISCIPLINE_W);
    let overflows = width * columns.len() as f32 + gaps > full + 1.0;

    let body = |ui: &mut egui::Ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = space::XL;
            for d in &columns {
                let name = str_at(d, "discipline");
                let (done, total) = (num_at(d, "done"), num_at(d, "total"));
                ui.vertical(|ui| {
                    ui.set_width(width);
                    ui.label(
                        egui::RichText::new(name)
                            .size(text::SMALL)
                            .family(egui::FontFamily::Name(theme::MEDIUM.into()))
                            .color(colour::TEXT_2),
                    );
                    ui.add_space(space::SM);
                    w::progress(ui, fraction(done, total), width, discipline_colour(name));
                    ui.add_space(space::SM);
                    // Right-aligned under the bar's far end, so the figures
                    // line up as a column of their own down the strip.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!("{done}/{total}"))
                                .size(text::CAPTION)
                                .color(colour::TEXT),
                        );
                    });
                });
            }
        });
    };

    if overflows {
        egui::ScrollArea::horizontal().show(ui, body);
    } else {
        body(ui);
    }
}

/// The header above a phase's tasks: its number and name, how many tasks it
/// holds, then its state and progress on the right.
fn phase_header(ui: &mut egui::Ui, p: &Value, done: usize, total: usize) {
    let position = p.get("position").and_then(Value::as_i64).unwrap_or(0);
    let status = str_at(p, "status");
    let gate = p.get("gate").and_then(Value::as_bool).unwrap_or(false);
    let label = format!("{position:02} · {}", str_at(p, "name"));

    shell::section_count_with(ui, &label, total, |ui| {
        w::muted(ui, &format!("{done} / {total}"));
        ui.add_space(space::SM);
        if gate {
            c::chip(ui, "gate", c::Tone::Running, false);
        }
        c::chip(ui, status_label(status), c::status_tone(status), true);
    });
}

/// What a click on a card meant.
enum Hit {
    Open(String),
    Claim(String),
}

/// One task. `titles` names blockers that are in this project's task list.
///
/// Unclaimed work drops to a slim card: it has no state worth four chips and
/// no people, and the one thing to do with it is take it — the same shape the
/// claim zone has on My tasks.
fn task_card(
    ui: &mut egui::Ui,
    t: &Value,
    titles: &HashMap<String, String>,
    can_claim: bool,
) -> Option<Hit> {
    let status = str_at(t, "status").to_string();
    let is_agent = str_at(t, "assigneeKind") == "agent";
    let claimed = str_at(t, "claimedBy").to_string();
    let person = str_at(t, "assigneePersonId").to_string();
    let id = str_at(t, "id").to_string();
    let title = str_at(t, "title").to_string();
    let discipline = str_at(t, "discipline").to_string();
    let assigned = is_agent || !claimed.is_empty() || !person.is_empty();
    let blocked = num_at(t, "blockersDone") < num_at(t, "blockersTotal");

    if !assigned && !blocked {
        return claimable_card(ui, &id, &title, &discipline, can_claim);
    }

    // The blocker's title where we hold it, its short id where the blocker
    // lives in a phase the current filter excluded.
    let waiting = blocked.then(|| {
        let first = t
            .get("blockedBy")
            .and_then(Value::as_array)
            .and_then(|b| b.first())
            .and_then(Value::as_str)
            .unwrap_or("");
        titles
            .get(first)
            .cloned()
            .unwrap_or_else(|| first.get(..8).unwrap_or(first).to_string())
    });

    // Blockers beat the status column: a task marked in progress that waits on
    // someone else is not in progress, and the chip should not claim it is.
    let state = if blocked { "blocked" } else { status.as_str() };

    let mut chips: Vec<(String, c::Tone, bool)> = vec![(age(t), c::Tone::Quiet, false)];
    if !discipline.is_empty() {
        chips.push((discipline.clone(), c::discipline_tone(&discipline), false));
    }
    chips.push((capitalise(status_label(state)), c::status_tone(state), true));

    let context = match &waiting {
        Some(what) => waiting_on(what),
        None => str_at(t, "phaseName").to_string(),
    };

    let trailing: Vec<(String, c::Tone)> = if is_agent {
        let label = if claimed.is_empty() { "agent".to_owned() } else { claimed.clone() };
        vec![(label, c::Tone::Agent)]
    } else {
        Vec::new()
    };
    let seed = if claimed.is_empty() { person } else { claimed };
    // The agent chip already names who holds it; a face beside it is noise.
    let people: Vec<String> =
        if is_agent || seed.is_empty() { Vec::new() } else { vec![seed] };

    let hit = c::task_card(
        ui,
        &c::TaskCard {
            title: &title,
            chips: &chips,
            context: &context,
            trailing_chips: &trailing,
            people: &people,
        },
    );
    hit.clicked().then_some(Hit::Open(id))
}

/// An unclaimed task: a discipline, a title, and the one thing to do with it.
fn claimable_card(
    ui: &mut egui::Ui,
    id: &str,
    title: &str,
    discipline: &str,
    can_claim: bool,
) -> Option<Hit> {
    let mut claimed = false;
    let hit = c::slim_card(ui, |ui| {
        if !discipline.is_empty() {
            c::chip(ui, discipline, c::discipline_tone(discipline), false);
            ui.add_space(space::XS);
        }
        ui.label(
            egui::RichText::new(title)
                .size(text::BODY)
                .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                .color(colour::TEXT),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            claimed = w::button(ui, "Claim", w::Emphasis::Ghost, can_claim).clicked();
            ui.add_space(space::SM);
            w::muted(ui, "unassigned");
        });
    })
    .interact(egui::Sense::click());

    if claimed {
        return Some(Hit::Claim(id.to_string()));
    }
    hit.clicked().then(|| Hit::Open(id.to_string()))
}

// ---------------------------------------------------------------------- pieces

/// A one-of-many dropdown over `options`, `None` meaning any. Returns true when
/// the selection changed.
fn filter(
    ui: &mut egui::Ui,
    id: &str,
    any_label: &str,
    options: &[&'static str],
    slot: &mut Option<&'static str>,
) -> bool {
    let mut changed = false;
    let selected = status_label(slot.unwrap_or(any_label)).to_string();
    egui::ComboBox::from_id_salt(id)
        .selected_text(egui::RichText::new(selected).size(text::BODY))
        .show_ui(ui, |ui| {
            let any = egui::RichText::new(any_label).size(text::BODY);
            if ui.selectable_label(slot.is_none(), any).clicked() && slot.is_some() {
                *slot = None;
                changed = true;
            }
            for opt in options {
                let label = egui::RichText::new(status_label(opt)).size(text::BODY);
                if ui.selectable_label(*slot == Some(*opt), label).clicked()
                    && *slot != Some(*opt)
                {
                    *slot = Some(*opt);
                    changed = true;
                }
            }
        });
    changed
}

/// `w::blocked_by`'s wording, as a string — a card's context line is text, not
/// a widget.
/// ponytail: the same sentence lives in mytasks.rs. Fold them together the day
/// a third screen needs it.
fn waiting_on(what: &str) -> String {
    format!("\u{2933} waiting on \u{201c}{what}\u{201d}")
}

/// How long since the task last moved, for the quiet leading chip.
fn age(t: &Value) -> String {
    let Ok(then) = DateTime::parse_from_rfc3339(str_at(t, "updatedAt")) else {
        return "no activity".to_owned();
    };
    match (Utc::now() - then.with_timezone(&Utc)).num_seconds().max(0) {
        s if s < 60 => "just now".to_owned(),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86_400 => format!("{}h ago", s / 3600),
        s => format!("{}d ago", s / 86_400),
    }
}

fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn array(v: Option<&Value>) -> Vec<Value> {
    v.and_then(Value::as_array).cloned().unwrap_or_default()
}

fn str_at<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn num_at(v: &Value, key: &str) -> i64 {
    v.get(key).and_then(Value::as_i64).unwrap_or(0)
}

fn fraction(done: i64, total: i64) -> f32 {
    if total <= 0 {
        return 0.0;
    }
    done as f32 / total as f32
}
