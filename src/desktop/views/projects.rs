//! Projects: the list, and the form that makes one.
//!
//! The detail screen is `board.rs`; this file is everything before you click
//! into a project. Split out so the two can change independently.
//!
//! The list is a table, not a grid of cards. Cards were the right unit when a
//! project was a paragraph; they are the wrong one for "which of these twenty
//! is behind", because you cannot compare a column of cards down a page. Same
//! reasoning that turned Home into a table, same column/hover/keyboard
//! machinery — a project row should feel like a task row.
//!
//! The create form makes tasks too. A project with no tasks is a heading, and
//! the old flow made you create one, click into it, and start again; handing
//! out the first few in the same step is the difference between a plan and an
//! empty board.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use egui::{Align, Layout, RichText};
use egui_extras::{Column, TableBuilder};
use serde_json::Value;

use super::board::{array, fraction, num_at, str_at};
use crate::desktop::design::{
    avatar, cards as c, colour, motion, radius, shell, size, space, status_label, text, theme, viz,
    widgets as w,
};
use crate::desktop::App;

const PROJECTS_KEY: &str = "board:projects";
pub(super) const PEOPLE_KEY: &str = "board:people";
/// Where a create's reply is collected.
const CREATE_KEY: &str = "board:create";
/// Which row the pointer was over last frame. A table row's own response only
/// exists after its first cell, which is too late to tint that cell.
const HOVER: &str = "projects:hover";

/// How far each avatar in the roster sits over the one before it.
pub(super) const AVATAR_OVERLAP: f32 = 6.0;
/// Past this the roster shows a "+N" instead of more faces.
pub(super) const MAX_AVATARS: usize = 5;
/// The prose measure for a project description: ~70 characters at `text::BODY`,
/// which is where a paragraph stops needing a finger to track the line.
pub(super) const PROSE_W: f32 = 640.0;

// ---- table geometry. Fixed so the columns line up with the header and with
// ---- each other; the description takes whatever is left.
const ROW_H: f32 = 38.0;
/// A project name is the thing being scanned for, so it gets the widest fixed
/// column — past this a name truncates rather than pushing the row around.
const COL_NAME: f32 = 220.0;
/// The description's floor. Below this a one-liner is cut to nothing useful,
/// so the table would rather squeeze the window than this column.
const COL_DESCRIPTION: f32 = 200.0;
/// Five faces at `AVATAR_SM` overlapping by `AVATAR_OVERLAP`, plus room for
/// the "+N" that replaces the sixth.
const COL_PEOPLE: f32 = 116.0;
/// The bar plus its "3/8".
const COL_PROGRESS: f32 = 140.0;
const PROGRESS_BAR_W: f32 = 90.0;
const COL_STATUS: f32 = 104.0;
const COL_CREATED: f32 = 78.0;

/// What the trailing controls on a draft-task row need: three menus, a Remove,
/// and the gaps between them. The title input takes the rest.
const TASK_CONTROLS_W: f32 = 420.0;
/// Below this the title input stops giving ground; the row wraps instead.
const TASK_TITLE_MIN_W: f32 = 160.0;

/// Priority, as the server stores it and as a person reads it. The value is a
/// string because that is what `viz::select` slots hold; it becomes an int on
/// submit.
const PRIORITIES: [(&str, &str); 5] = [
    ("0", "P0 Urgent"),
    ("1", "P1 High"),
    ("2", "P2 Normal"),
    ("3", "P3 Low"),
    ("4", "P4 Someday"),
];
/// The disciplines a first task can be filed under. Same three the flow strip
/// orders by; anything else is a discipline the board invented later.
const DISCIPLINES: [&str; 3] = ["design", "frontend", "backend"];
/// What a new draft task defaults to: normal, not urgent. A form that defaults
/// to P0 produces a board where everything is P0.
const DEFAULT_PRIORITY: &str = "2";

/// What the create form holds. `None` on `State::creating` means the form is
/// closed, which is also how the Create project button knows not to redraw
/// itself over an open form.
#[derive(Default)]
pub struct Draft {
    pub title: String,
    pub description: String,
    /// Person ids, in the order they were picked. Empty is a real answer.
    pub members: Vec<String>,
    /// The project's first tasks. Rows with no title are dropped on submit,
    /// so an accidental Add task costs nothing.
    pub tasks: Vec<DraftTask>,
}

/// One row of the form's Tasks section. Every optional field is an
/// `Option<String>` because that is the shape `viz::select` writes into.
pub struct DraftTask {
    pub title: String,
    pub assignee: Option<String>,
    pub discipline: Option<String>,
    pub priority: Option<String>,
}

impl Default for DraftTask {
    fn default() -> Self {
        Self {
            title: String::new(),
            assignee: None,
            discipline: None,
            priority: Some(DEFAULT_PRIORITY.to_owned()),
        }
    }
}

// ---------------------------------------------------------------- project list

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let can_write = app.can_write();
    let net = app.net.as_mut().unwrap();

    // A finished create has invalidated the list, the sidebar counts and every
    // rollup that counts projects. Tasks came with it, so the task caches go
    // too.
    if net.data(CREATE_KEY).is_some() {
        net.invalidate(CREATE_KEY);
        net.invalidate(PROJECTS_KEY);
        net.invalidate("home");
        net.invalidate_prefix("task:");
        app.board.creating = None;
    }
    let net = app.net.as_mut().unwrap();

    net.get_once(PROJECTS_KEY, "/api/user/projects");
    // The assignee picker needs names, and the rows need them to resolve
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
    shell::page_title(ui, "Projects", &subtitle(list.len(), loading), |ui| {
        // The form is the button's own state: while it is open the button
        // would only re-open what is already open.
        if app.board.creating.is_none() && w::primary(ui, "Create project", can_write).clicked() {
            start_create = true;
        }
    });
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

    let mut open: Option<String> = None;
    if let Some(err) = error {
        w::error(ui, &err);
    } else if list.is_empty() {
        if loading {
            w::loading(ui, "projects");
        } else if app.board.creating.is_none() {
            w::empty(
                ui,
                "No projects yet.",
                "Create one and hand out its first tasks in the same step.",
            );
        }
    } else {
        table(ui, &list, &flows, &names, &mut open);
        ui.add_space(space::XXL);
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

// --------------------------------------------------------------------- table

fn table(
    ui: &mut egui::Ui,
    rows: &[Value],
    flows: &HashMap<String, Value>,
    names: &HashMap<String, String>,
    open: &mut Option<String>,
) {
    let hover_id = egui::Id::new(HOVER);
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
                .id_salt("projects:table")
                .vscroll(false)
                .sense(egui::Sense::click())
                .cell_layout(Layout::left_to_right(Align::Center))
                .column(Column::exact(COL_NAME).clip(true))
                .column(Column::remainder().at_least(COL_DESCRIPTION).clip(true))
                .column(Column::exact(COL_PEOPLE))
                .column(Column::exact(COL_PROGRESS))
                .column(Column::exact(COL_STATUS))
                .column(Column::exact(COL_CREATED))
                .header(size::CONTROL, |mut row| {
                    for name in
                        ["Project", "Description", "Assignees", "Progress", "Status", "Created"]
                    {
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
                    for (i, p) in rows.iter().enumerate() {
                        body.row(ROW_H, |mut row| {
                            row.set_hovered(was == Some(i));
                            row.set_overline(i > 0);
                            project_row(&mut row, p, flows.get(str_at(p, "id")), names);
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
                    *open = Some(str_at(&rows[i], "id").to_owned());
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

fn project_row(
    row: &mut egui_extras::TableRow<'_, '_>,
    p: &Value,
    flow: Option<&Value>,
    names: &HashMap<String, String>,
) {
    let (done, total) = flow.map(|f| (num_at(f, "done"), num_at(f, "total"))).unwrap_or((0, 0));
    let status = str_at(p, "status");

    row.col(|ui| {
        ui.label(
            RichText::new(str_at(p, "name"))
                .size(text::BODY)
                .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                .color(colour::TEXT),
        );
    });

    row.col(|ui| {
        let description = str_at(p, "description");
        let (copy, ink) = if description.is_empty() {
            ("No description", colour::TEXT_FAINT)
        } else {
            (description, colour::TEXT_MUTED)
        };
        // One line. The column clips, and a wrapped cell would make one row
        // taller than the rest of the table.
        ui.label(RichText::new(copy).size(text::SMALL).color(ink));
    });

    row.col(|ui| roster(ui, p, names));

    row.col(|ui| {
        if total > 0 {
            w::progress(ui, fraction(done, total), PROGRESS_BAR_W, colour::ACCENT);
            ui.add_space(space::SM);
            ui.label(
                RichText::new(format!("{done}/{total}")).size(text::CAPTION).color(colour::TEXT),
            );
        } else {
            w::caption(ui, "\u{2014}");
        }
    });

    row.col(|ui| {
        c::chip(ui, status_label(status), c::status_tone(status), true);
    });

    row.col(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(age(p)).size(text::SMALL).color(colour::TEXT_MUTED));
        });
    });
}

/// The overlapping stack of faces on a project. Five fit where three would
/// side by side, and names come back on hover.
fn roster(ui: &mut egui::Ui, p: &Value, names: &HashMap<String, String>) {
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
        w::caption(ui, "\u{2014}");
        return;
    }

    ui.spacing_mut().item_spacing.x = -AVATAR_OVERLAP;
    for name in members.iter().take(MAX_AVATARS) {
        let r = avatar::small(ui, name, size::AVATAR_SM).on_hover_text(*name);
        // A ring in the row's own colour, so each face cuts out of the one
        // beneath rather than bleeding into it.
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
}

/// How old the project is, in the two characters a table column has room for.
/// `board::age` reads `updatedAt`; a project's interesting date is when it was
/// started, so the field differs and the wording is shorter.
fn age(p: &Value) -> String {
    let Ok(then) = DateTime::parse_from_rfc3339(str_at(p, "createdAt")) else {
        return String::new();
    };
    match (Utc::now() - then.with_timezone(&Utc)).num_seconds().max(0) {
        s if s < 3600 => format!("{}m", (s / 60).max(1)),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

// ---------------------------------------------------------------- create form

/// The create form, inline under the header rather than in a modal.
///
/// Returns the request body once, on the frame Create is clicked. A modal was
/// the obvious first thought and the wrong one: covering the list to ask what
/// to add to the list is a worse trade than pushing it down — and the form is
/// now long enough that a modal would need its own scrollbar.
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

        // Tasks can only be handed to people who are on the project, so this
        // menu is the Assignees set above, not the whole company.
        let assignable: Vec<(String, String)> = options
            .iter()
            .filter(|(id, _)| draft.members.contains(id))
            .cloned()
            .collect();
        tasks_section(ui, draft, &assignable);
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
                    "tasks": task_bodies(draft),
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

/// The first tasks, one row each. Empty by default: a project that is only a
/// name is still a legitimate thing to create, and pre-seeding a blank row
/// makes the form look like it is demanding one.
fn tasks_section(ui: &mut egui::Ui, draft: &mut Draft, assignable: &[(String, String)]) {
    w::heading(ui, "Tasks");
    ui.add_space(space::XS);
    w::caption(ui, "Handed out with the project. Rows left blank are dropped.");
    ui.add_space(space::SM);

    let disciplines: Vec<(String, String)> =
        DISCIPLINES.iter().map(|d| ((*d).to_owned(), (*d).to_owned())).collect();
    let priorities: Vec<(String, String)> =
        PRIORITIES.iter().map(|(v, l)| ((*v).to_owned(), (*l).to_owned())).collect();

    let mut remove: Option<usize> = None;
    for (i, task) in draft.tasks.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::SM;
            let width = (ui.available_width() - TASK_CONTROLS_W).max(TASK_TITLE_MIN_W);
            task_title(ui, width, &mut task.title);
            viz::select(ui, "Unassigned", assignable, &mut task.assignee);
            viz::select(ui, "Any discipline", &disciplines, &mut task.discipline);
            viz::select(ui, "P2 Normal", &priorities, &mut task.priority);
            if w::ghost(ui, "Remove").clicked() {
                remove = Some(i);
            }
        });
        ui.add_space(space::XS);
    }
    if let Some(i) = remove {
        draft.tasks.remove(i);
    }

    ui.add_space(space::XXS);
    if w::secondary(ui, "Add task", true).clicked() {
        draft.tasks.push(DraftTask::default());
    }
}

/// A bare single-line input, sized to the row. `w::field` is the labelled
/// version and stacks its own caption above the box; a task row's columns are
/// labelled once by the placeholders, not once per row.
///
/// Private to this file because it is the only place a form row needs an
/// unlabelled input — promote it into `widgets` when a second one turns up.
fn task_title(ui: &mut egui::Ui, width: f32, value: &mut String) -> egui::Response {
    ui.add_sized(
        [width, viz::HEIGHT],
        egui::TextEdit::singleline(value)
            .hint_text(
                RichText::new("What needs doing")
                    .size(text::BODY)
                    .color(colour::TEXT_DISABLED),
            )
            .margin(egui::Margin::symmetric(space::MD as i8, space::SM as i8)),
    )
}

/// The `tasks` array of the create request. Untitled rows never leave the
/// form: an accidental Add task should not create an unnamed task.
fn task_bodies(draft: &Draft) -> Vec<Value> {
    draft
        .tasks
        .iter()
        .filter(|t| !t.title.trim().is_empty())
        .map(|t| {
            serde_json::json!({
                "title": t.title.trim(),
                "body": "",
                "assigneeId": t.assignee,
                "discipline": t.discipline,
                "priority": t
                    .priority
                    .as_deref()
                    .and_then(|p| p.parse::<i32>().ok())
                    .unwrap_or(2),
            })
        })
        .collect()
}
