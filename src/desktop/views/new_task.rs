//! New task: one made on its own, from anywhere — Home's table, My Tasks,
//! the palette, or `C` (Linear's key). With no project it is standalone;
//! with one it lands in that project's first phase.

use egui::{Key, Modifiers};
use serde_json::json;

use super::board::str_at;
use super::projects::{person_option, DEFAULT_PRIORITY, PEOPLE_KEY, PRIORITIES};
use crate::desktop::design::{space, viz, widgets as w};
use crate::desktop::App;

/// Not under `task:` or `home`: a success sweeps those, and the reply has to
/// be read first.
const KEY: &str = "newtask:create";

pub struct Draft {
    title: String,
    body: String,
    assignee: Option<String>,
    priority: Option<String>,
    category: Option<String>,
    project: Option<String>,
    /// Focus goes to the title once, on the first frame.
    focused: bool,
    sent: bool,
}

/// The open dialog, if there is one. Lives in `board::State`, beside the
/// other task actions.
#[derive(Default)]
pub struct State(Option<Draft>);

/// Open it fresh, assigned to the viewer. Nothing for a read-only viewer: the
/// server would refuse what the form sends.
pub fn open(app: &mut App) {
    if !app.can_write() {
        return;
    }
    let me = app.net.as_ref().and_then(|n| n.data("__me")).map(|m| str_at(m, "personId").to_owned());
    app.board.new_task.0 = Some(Draft {
        title: String::new(),
        body: String::new(),
        assignee: me.filter(|m| !m.is_empty()),
        priority: Some(DEFAULT_PRIORITY.to_owned()),
        category: None,
        project: None,
        focused: false,
        sent: false,
    });
}

/// `C` with nothing focused and nothing else open: what Linear does.
pub fn shortcut(app: &mut App, ctx: &egui::Context) {
    if app.board.new_task.0.is_some() || app.palette.open {
        return;
    }
    let free = !ctx.egui_wants_keyboard_input() && ctx.memory(|m| m.top_modal_layer().is_none());
    if free && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::C)) {
        open(app);
    }
}

/// The dialog, once a frame from the shell so it sits over whichever page.
pub fn ui(app: &mut App, ctx: &egui::Context) {
    let Some(d) = app.board.new_task.0.as_mut() else { return };
    let net = app.net.as_mut().expect("chrome runs signed in");
    net.get_once(PEOPLE_KEY, "/api/user/people");
    super::triage::want_projects(net);

    // The reply to a create sent on an earlier frame.
    if d.sent && !net.is_loading(KEY) {
        d.sent = false;
        if let Some(Ok(_)) = net.peek(KEY) {
            w::toast(ctx, format!("Created \u{201c}{}\u{201d}.", d.title.trim()), false);
            net.invalidate(KEY);
            net.invalidate_prefix("home");
            net.invalidate_prefix("mytasks");
            net.invalidate_prefix("board:");
            net.invalidate_prefix("palette:");
            net.invalidate(super::chrome::COUNTS);
            app.board.new_task.0 = None;
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
        ui.add_space(space::MD);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(space::SM, space::SM);
            viz::select(ui, "Unassigned", &people, &mut d.assignee);
            viz::select(ui, "P2 Normal", &priorities, &mut d.priority);
            viz::select(ui, "No category", &categories, &mut d.category);
            viz::select(ui, "No project", &projects, &mut d.project);
        });
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

    if go {
        let body = json!({
            "title": d.title.trim(),
            "body": d.body.trim(),
            "assigneeId": d.assignee,
            "priority": d.priority.as_deref().and_then(|p| p.parse::<i32>().ok()).unwrap_or(2),
            "category": d.category,
            "projectId": d.project,
        });
        net.invalidate(KEY);
        net.post(KEY, "/api/user/tasks", body);
        d.sent = true;
    } else if (close || modal.should_close()) && !busy {
        app.board.new_task.0 = None;
    }
}

/// "New task", for a page header: whether it was clicked. Call `open` then.
pub fn button(ui: &mut egui::Ui) -> bool {
    w::secondary(ui, "New task", true).on_hover_text("New task (C)").clicked()
}
