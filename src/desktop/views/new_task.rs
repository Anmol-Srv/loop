//! New task: one made on its own, from anywhere — Home's table, My Tasks,
//! the palette, or `C` (Linear's key). With no project it is standalone;
//! with one it lands in that project's first phase.

use egui::{Key, Modifiers};
use serde_json::{json, Value};

use super::board::str_at;
use super::projects::{label_picker, person_option, DEFAULT_PRIORITY, LABELS_KEY, PEOPLE_KEY, PRIORITIES};
use crate::desktop::design::{colour, space, text, viz, widgets as w};
use crate::desktop::App;

/// Not under `task:` or `home`: a success sweeps those, and the reply has to
/// be read first.
const KEY: &str = "newtask:create";
/// A label typed into the picker; its reply joins the draft's set.
const LABEL_KEY: &str = "newtask:label";
const FILE_KEY: &str = "newtask:file";

pub struct Draft {
    title: String,
    body: String,
    assignee: Option<String>,
    priority: Option<String>,
    category: Option<String>,
    project: Option<String>,
    /// Label ids, in the order they were picked.
    labels: Vec<String>,
    /// Focus goes to the title once, on the first frame.
    focused: bool,
    sent: bool,
    files: Vec<Value>,
    swallow_paste: bool,
}

/// The open dialog, if there is one. Lives in `board::State`, beside the
/// other task actions.
#[derive(Default)]
pub struct State {
    draft: Option<Draft>,
    pending: Vec<(String, Value)>,
    uploading: Option<String>,
}

/// Open it fresh, assigned to the viewer. Nothing for a read-only viewer: the
/// server would refuse what the form sends.
pub fn open(app: &mut App) {
    if !app.can_write() {
        return;
    }
    let me = app.net.as_ref().and_then(|n| n.data("__me")).map(|m| str_at(m, "personId").to_owned());
    app.board.new_task.draft = Some(Draft {
        title: String::new(),
        body: String::new(),
        assignee: me.filter(|m| !m.is_empty()),
        priority: Some(DEFAULT_PRIORITY.to_owned()),
        category: None,
        project: None,
        labels: Vec::new(),
        focused: false,
        sent: false,
        files: Vec::new(),
        swallow_paste: false,
    });
}

/// `C` with nothing focused and nothing else open: what Linear does.
pub fn shortcut(app: &mut App, ctx: &egui::Context) {
    if app.board.new_task.draft.is_some() || app.palette.open {
        return;
    }
    let free = !ctx.egui_wants_keyboard_input() && ctx.memory(|m| m.top_modal_layer().is_none());
    if free && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::C)) {
        open(app);
    }
}

pub fn intake(app: &mut App, ctx: &egui::Context) {
    let Some(d) = app.board.new_task.draft.as_mut() else { return };
    d.files.extend(super::task::pasted_files(ctx, &mut d.swallow_paste));
    d.files.extend(super::task::dropped_files(ctx));
}

fn settle_uploads(ctx: &egui::Context, net: &mut crate::desktop::net::Net, s: &mut State) {
    if let Some(name) = s.uploading.clone() {
        if net.is_loading(FILE_KEY) {
            return;
        }
        if let Some(Err(e)) = net.peek(FILE_KEY) {
            w::toast(ctx, format!("Could not attach {name}: {e}"), true);
        }
        s.uploading = None;
        net.invalidate(FILE_KEY);
        net.invalidate_prefix("task:");
    }
    if s.pending.is_empty() {
        return;
    }
    let (task, body) = s.pending.remove(0);
    s.uploading = Some(str_at(&body, "name").to_owned());
    net.post(FILE_KEY, &format!("/api/user/tasks/{task}/files"), body);
}

/// The dialog, once a frame from the shell so it sits over whichever page.
pub fn ui(app: &mut App, ctx: &egui::Context) {
    let net = app.net.as_mut().expect("chrome runs signed in");
    let state = &mut app.board.new_task;
    settle_uploads(ctx, net, state);
    let Some(d) = state.draft.as_mut() else { return };
    net.get_once(PEOPLE_KEY, "/api/user/people");
    super::triage::want_projects(net);
    net.get_once(LABELS_KEY, "/api/user/labels");
    if let Some(label) = net.data(LABEL_KEY).cloned() {
        net.invalidate(LABEL_KEY);
        net.invalidate(LABELS_KEY);
        let id = str_at(&label, "id").to_owned();
        if !id.is_empty() && !d.labels.contains(&id) {
            d.labels.push(id);
        }
    }
    let label_error = net.error(LABEL_KEY).map(str::to_owned);
    let all_labels = super::board::array(net.data(LABELS_KEY));
    let mut new_label = None;

    // The reply to a create sent on an earlier frame.
    if d.sent && !net.is_loading(KEY) {
        d.sent = false;
        if let Some(Ok(task)) = net.peek(KEY) {
            let id = str_at(task, "id").to_owned();
            let held = d.files.len();
            let attaching = match held {
                0 => String::new(),
                1 => "; attaching 1 file".to_owned(),
                n => format!("; attaching {n} files"),
            };
            w::toast(ctx, format!("Created \u{201c}{}\u{201d}{attaching}.", d.title.trim()), false);
            state.pending.extend(d.files.drain(..).map(|f| (id.clone(), f)));
            net.invalidate(KEY);
            net.invalidate_prefix("home");
            net.invalidate_prefix("mytasks");
            net.invalidate_prefix("board:");
            net.invalidate_prefix("palette:");
            net.invalidate(super::chrome::COUNTS);
            state.draft = None;
            return;
        }
    }
    let error = net.error(KEY).map(str::to_owned);
    let people: Vec<(String, String)> = super::board::array(net.data(PEOPLE_KEY)).iter().map(person_option).collect();
    let projects = super::triage::projects(net);
    let priorities = super::projects::owned(&PRIORITIES);
    let categories = super::projects::owned(&super::triage::CATEGORIES);

    let busy = d.sent;
    let ready = !d.title.trim().is_empty() && !busy;
    let submit_keys = ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Enter));
    let (mut go, mut close) = (ready && submit_keys, false);
    let modal = super::agents::dialog(ctx, "new-task", super::agents::DIALOG_W * 1.2, |ui| {
        super::agents::heading(ui, "New task");
        ui.add_space(space::LG);
        let title = w::field(ui, "Title", &mut d.title, false, "e.g. Fix the saved-card checkout\u{2026}");
        if !d.focused {
            title.request_focus();
            d.focused = true;
        }
        ui.add_space(space::MD);
        w::field_multiline(ui, "Description", &mut d.body, 3, "Add detail, links, acceptance\u{2026}");
        if !d.files.is_empty() {
            ui.add_space(space::SM);
            let mut remove = None;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(space::MD, space::XS);
                for (i, f) in d.files.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::XS;
                        let icon = if str_at(f, "mime").starts_with("image/") {
                            egui_phosphor::regular::IMAGE
                        } else {
                            egui_phosphor::regular::FILE_TEXT
                        };
                        ui.label(egui::RichText::new(icon).size(text::SMALL).color(colour::TEXT_MUTED()));
                        ui.label(egui::RichText::new(str_at(f, "name")).size(text::SMALL).color(colour::TEXT_2()));
                        if !busy && super::task::remove_x(ui).clicked() {
                            remove = Some(i);
                        }
                    });
                }
            });
            w::caption(ui, "Attached once the task is created.");
            if let Some(i) = remove {
                d.files.remove(i);
            }
        }
        ui.add_space(space::MD);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(space::SM, space::SM);
            viz::select(ui, "Unassigned", &people, &mut d.assignee);
            viz::select(ui, "P2 Normal", &priorities, &mut d.priority);
            viz::select(ui, "No category", &categories, &mut d.category);
            viz::select(ui, "No project", &projects, &mut d.project);
        });
        ui.add_space(space::SM);
        new_label = label_picker(ui, "Add labels", &all_labels, &mut d.labels);
        if let Some(err) = &label_error {
            ui.label(egui::RichText::new(format!("Could not make that label: {err}")).size(text::CAPTION).color(colour::DANGER()));
        }
        if let Some(err) = &error {
            ui.add_space(space::MD);
            w::error(ui, err);
        }
        ui.add_space(space::XL);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = space::SM;
            go |= w::primary(ui, if busy { "Creating\u{2026}" } else { "Create task" }, ready)
                .on_hover_text("Cmd+Return")
                .clicked();
            close = w::ghost(ui, "Cancel").clicked();
        });
    });

    if let Some(body) = new_label {
        net.post(LABEL_KEY, "/api/user/labels", body);
    }
    if go {
        let body = json!({
            "title": d.title.trim(),
            "body": d.body.trim(),
            "assigneeId": d.assignee,
            "priority": d.priority.as_deref().and_then(|p| p.parse::<i32>().ok()).unwrap_or(2),
            "category": d.category,
            "projectId": d.project,
            "labelIds": d.labels,
        });
        net.invalidate(KEY);
        net.post(KEY, "/api/user/tasks", body);
        d.sent = true;
    } else if (close || modal.should_close()) && !busy {
        state.draft = None;
    }
}

/// "New task", for a page header: whether it was clicked. Call `open` then.
pub fn button(ui: &mut egui::Ui) -> bool {
    w::cta(ui, egui_phosphor::regular::PLUS, "New task", "C").on_hover_text("New task (C)").clicked()
}
