//! Projects: the list, and the form that makes one.
//!
//! The detail screen is `board.rs`; this file is everything before you click
//! into a project. Split out so the two can change independently.

use std::collections::HashMap;

use serde_json::Value;

use super::board::{array, fraction, num_at, str_at};
use crate::desktop::design::{
    avatar, cards as c, colour, shell, size, space, status_label, text, viz,
    widgets as w,
};
use crate::desktop::App;

const PROJECTS_KEY: &str = "board:projects";
pub(super) const PEOPLE_KEY: &str = "board:people";
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
pub(super) const AVATAR_OVERLAP: f32 = 6.0;
/// Past this the roster shows a "+N" instead of more faces.
pub(super) const MAX_AVATARS: usize = 5;
/// Room a status chip needs at the end of the title row. The title truncates
/// before it, never under it.
const STATUS_CHIP_W: f32 = 96.0;
/// The prose measure for a project description: ~70 characters at `text::BODY`,
/// which is where a paragraph stops needing a finger to track the line.
pub(super) const PROSE_W: f32 = 640.0;

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


// ---------------------------------------------------------------- project list

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
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

