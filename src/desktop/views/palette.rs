//! The command palette (⌘K): find a task or a project by name, or jump to a
//! screen, without leaving the keyboard.
//!
//! It fetches its own lists rather than borrowing another view's: the home
//! payload holds only your tasks, and a palette that cannot find a colleague's
//! task by title is a palette nobody trusts twice.

use egui::{Align, Key, Layout, Modifiers, RichText};
use serde_json::Value;

use crate::desktop::design::{cards as c, colour, pad, radius, size, space, status_label, text, viz};
use crate::desktop::{App, Tab};

const TASKS: &str = "palette:tasks";
const PROJECTS: &str = "palette:projects";
/// Per group. Past six the answer is "type more", not "scroll".
const CAP: usize = 6;
/// How far down the window the panel hangs. Anchored to the top rather than
/// centred so it does not jump as results arrive and leave.
const DROP: f32 = 0.14;

#[derive(Default)]
pub struct State {
    /// Whether the palette is showing.
    pub open: bool,
    pub query: String,
    /// The row Enter would open, as an index into the flat result list.
    pub highlight: usize,
}

#[derive(Clone)]
enum Target {
    Task(String),
    Project(String),
    Tab(Tab),
    CreateProject,
    NewTask,
}

struct Hit {
    group: &'static str,
    title: String,
    /// Muted text after the title: a task's project.
    context: String,
    /// The raw status, drawn as a chip on the right.
    status: Option<String>,
    target: Target,
}

/// Open it fresh: an empty query, the first row lit, and both lists refetched
/// so a task created a minute ago is findable.
pub fn open(app: &mut App) {
    app.palette = State { open: true, ..State::default() };
    if let Some(n) = app.net.as_mut() {
        n.invalidate(TASKS);
        n.invalidate(PROJECTS);
    }
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    if !app.palette.open {
        return;
    }
    let can_write = app.can_write();
    let Some(net) = app.net.as_mut() else { return };
    net.get_once(TASKS, "/api/user/tasks");
    net.get_once(PROJECTS, "/api/user/projects");
    let loading = net.is_loading(TASKS) || net.is_loading(PROJECTS);
    let error = net.error(TASKS).or_else(|| net.error(PROJECTS)).map(str::to_owned);

    let hits = results(net.data(TASKS), net.data(PROJECTS), &app.palette.query, can_write);
    let ctx = ui.ctx().clone();

    // Taken before the text field sees them: a single-line edit treats Enter
    // and Escape as "leave the field", and the arrows as cursor moves.
    let (down, up, enter, escape) = ctx.input_mut(|i| {
        (
            i.consume_key(Modifiers::NONE, Key::ArrowDown),
            i.consume_key(Modifiers::NONE, Key::ArrowUp),
            i.consume_key(Modifiers::NONE, Key::Enter),
            i.consume_key(Modifiers::NONE, Key::Escape),
        )
    });
    let s = &mut app.palette;
    if !hits.is_empty() {
        if down {
            s.highlight = (s.highlight + 1) % hits.len();
        }
        if up {
            s.highlight = (s.highlight + hits.len() - 1) % hits.len();
        }
    }
    s.highlight = s.highlight.min(hits.len().saturating_sub(1));

    let mut chosen = enter.then(|| hits.get(s.highlight).map(|h| h.target.clone())).flatten();
    let top = ctx.content_rect().height() * DROP;

    let modal = egui::Modal::new(egui::Id::new("palette"))
        .area(
            egui::Modal::default_area(egui::Id::new("palette"))
                .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, top)),
        )
        .backdrop_color(colour::CANVAS.gamma_multiply(0.7))
        .frame(
            egui::Frame::new()
                .fill(colour::SURFACE)
                .stroke(egui::Stroke::new(1.0, colour::LINE_STRONG))
                .corner_radius(radius::MD)
                .inner_margin(egui::Margin::same(pad::LIST.0 as i8)),
        )
        .show(&ctx, |ui| {
            ui.set_width(size::RAIL_W * 2.0);
            let before = s.query.clone();
            let field = ui.add(
                egui::TextEdit::singleline(&mut s.query)
                    .id(egui::Id::new("palette:query"))
                    .frame(egui::Frame::NONE)
                    .desired_width(f32::INFINITY)
                    .margin(egui::Margin::symmetric(space::SM as i8, space::SM as i8))
                    .font(egui::FontId::proportional(text::CARD))
                    .text_color(colour::TEXT)
                    .hint_text(
                        RichText::new("Search tasks and projects, or jump to a page…")
                            .size(text::CARD)
                            .color(colour::TEXT_FAINT),
                    ),
            );
            // The only input here, so it keeps focus: a click on a row must
            // not leave the next keystroke going nowhere.
            field.request_focus();
            if s.query != before {
                s.highlight = 0;
            }

            let line = ui.available_rect_before_wrap();
            ui.painter().hline(line.x_range(), line.top(), egui::Stroke::new(1.0, colour::LINE));
            ui.add_space(space::XS);

            if let Some(err) = &error {
                muted(ui, &format!("Could not load everything to search. {err} Close and reopen to retry."));
            } else if hits.is_empty() && loading {
                muted(ui, "Loading…");
            } else if hits.is_empty() {
                muted(ui, "No tasks, projects or pages match. Try fewer letters.");
            }

            let moved = ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
            let mut group = "";
            for (i, hit) in hits.iter().enumerate() {
                if hit.group != group {
                    group = hit.group;
                    ui.add_space(space::XS);
                    ui.label(
                        RichText::new(group)
                            .size(text::CAPTION)
                            .color(colour::TEXT_FAINT),
                    );
                }
                let response = row(ui, hit, i == s.highlight);
                if response.hovered() && moved {
                    s.highlight = i;
                }
                if response.clicked() {
                    chosen = Some(hit.target.clone());
                }
            }
        });

    if escape || modal.backdrop_response.clicked() {
        app.palette.open = false;
    }
    let Some(target) = chosen else { return };
    app.palette.open = false;
    match target {
        Target::Task(id) => app.task = Some(id),
        Target::Project(id) => {
            app.tab = Tab::Projects;
            app.project = Some(id);
            app.task = None;
        }
        Target::Tab(tab) => {
            app.tab = tab;
            app.project = None;
            app.task = None;
        }
        Target::CreateProject => {
            app.tab = Tab::Projects;
            app.project = None;
            app.task = None;
            app.board.creating = Some(Default::default());
        }
        Target::NewTask => super::new_task::open(app),
    }
}

/// Tasks, then projects, then pages. An empty query shows only the pages:
/// the first six tasks of an unordered list answer nothing.
fn results(tasks: Option<&Value>, projects: Option<&Value>, query: &str, can_write: bool) -> Vec<Hit> {
    let mut hits = Vec::new();
    let typed = !query.trim().is_empty();

    if typed {
        let list = |v: Option<&Value>| v.and_then(Value::as_array).cloned().unwrap_or_default();
        for t in list(tasks)
            .iter()
            .filter(|t| viz::matches(query, &[str_at(t, "title"), str_at(t, "projectName")]))
            .take(CAP)
        {
            hits.push(Hit {
                group: "Tasks",
                title: str_at(t, "title").to_owned(),
                context: str_at(t, "projectName").to_owned(),
                status: Some(str_at(t, "status").to_owned()),
                target: Target::Task(str_at(t, "id").to_owned()),
            });
        }
        for p in list(projects).iter().filter(|p| viz::matches(query, &[str_at(p, "name")])).take(CAP) {
            hits.push(Hit {
                group: "Projects",
                title: str_at(p, "name").to_owned(),
                context: String::new(),
                status: Some(str_at(p, "status").to_owned()),
                target: Target::Project(str_at(p, "id").to_owned()),
            });
        }
    }

    let mut pages = vec![
        ("Home", Target::Tab(Tab::Home)),
        ("My Tasks", Target::Tab(Tab::MyTasks)),
        ("Triage", Target::Tab(Tab::Triage)),
        ("Projects", Target::Tab(Tab::Projects)),
        ("Agents", Target::Tab(Tab::Agents)),
    ];
    // Offered only to someone who could submit the form it opens.
    if can_write {
        pages.push(("New task", Target::NewTask));
        pages.push(("Create project", Target::CreateProject));
    }
    for (name, target) in pages.into_iter().filter(|(n, _)| viz::matches(query, &[n])) {
        hits.push(Hit {
            group: "Go to",
            title: name.to_owned(),
            context: String::new(),
            status: None,
            target,
        });
    }
    hits
}

fn row(ui: &mut egui::Ui, hit: &Hit, lit: bool) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), size::ROW), egui::Sense::click());
    if lit {
        ui.painter().rect_filled(rect, radius::SM as f32, colour::SURFACE_ACTIVE);
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let mut inner = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(space::SM, 0.0)))
            .layout(Layout::right_to_left(Align::Center)),
    );
    if let Some(status) = &hit.status {
        c::chip(&mut inner, status_label(status), c::status_tone(status), true);
        inner.add_space(space::SM);
    }
    inner.with_layout(Layout::left_to_right(Align::Center), |ui| {
        ui.add(
            egui::Label::new(RichText::new(&hit.title).size(text::BODY).color(colour::TEXT))
                .truncate()
                .selectable(false),
        );
        if !hit.context.is_empty() {
            ui.add_space(space::XS);
            ui.add(
                egui::Label::new(
                    RichText::new(&hit.context).size(text::SMALL).color(colour::TEXT_MUTED),
                )
                .truncate()
                .selectable(false),
            );
        }
    });
    response
}

fn muted(ui: &mut egui::Ui, s: &str) {
    ui.add_space(space::XS);
    ui.label(RichText::new(s).size(text::SMALL).color(colour::TEXT_MUTED));
    ui.add_space(space::XS);
}

fn str_at<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or_default()
}
