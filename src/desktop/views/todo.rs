//! To-do: a scratchpad for the small things — write it down, tick it off.
//! Not a task: nothing is tracked, shared or handed to an agent. It lives on
//! this Mac only, in a file beside Loop's other preferences, and the same
//! list shows whichever workspace is open.

use std::cell::RefCell;

use egui::{RichText, Sense, Vec2};
use egui_phosphor::regular as icon;
use serde::{Deserialize, Serialize};

use crate::desktop::creds;
use crate::desktop::design::{colour, radius, shell, size, space, text, widgets as w};
use crate::desktop::design::agent as face;

const FILE: &str = "todos.json";

#[derive(Clone, Serialize, Deserialize)]
struct Item {
    id: u64,
    text: String,
    done: bool,
}

#[derive(Default)]
struct State {
    loaded: bool,
    items: Vec<Item>,
    draft: String,
    /// The item being edited, and its text so far.
    editing: Option<(u64, String)>,
    done_open: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn load(s: &mut State) {
    if !s.loaded {
        s.items = creds::load_pref(FILE).and_then(|raw| serde_json::from_str(&raw).ok()).unwrap_or_default();
        s.loaded = true;
    }
}

fn save(s: &State) {
    if let Ok(raw) = serde_json::to_string(&s.items) {
        // ponytail: a failed write loses only the last change; no retry.
        let _ = creds::store_pref(FILE, &raw);
    }
}

/// How many are left, for the sidebar.
pub fn open_count() -> usize {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        load(&mut s);
        s.items.iter().filter(|i| !i.done).count()
    })
}

pub fn ui(ui: &mut egui::Ui) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        load(&mut s);
        page(ui, &mut s);
    });
}

enum Act {
    Toggle(u64),
    Remove(u64),
    Edit(u64, String),
    Save,
}

fn page(ui: &mut egui::Ui, s: &mut State) {
    let done_n = s.items.iter().filter(|i| i.done).count();
    let mut clear = false;
    shell::page_title(ui, "To-do", "A scratchpad on this Mac \u{2014} not tracked, not shared.", |ui| {
        if done_n > 0 && w::ghost(ui, "Clear completed").clicked() {
            clear = true;
        }
    });

    ui.add_space(space::MD);
    let field = w::field(ui, "", &mut s.draft, false, "Write it down\u{2026}  Enter to add");
    if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        let text = s.draft.trim().to_owned();
        if !text.is_empty() {
            let id = s.items.iter().map(|i| i.id).max().unwrap_or(0) + 1;
            // Newest on top: what was just written is what is on your mind.
            s.items.insert(0, Item { id, text, done: false });
            s.draft.clear();
            save(s);
        }
        field.request_focus();
    }
    ui.add_space(space::LG);

    let mut act: Option<Act> = None;
    let open: Vec<Item> = s.items.iter().filter(|i| !i.done).cloned().collect();
    if open.is_empty() {
        w::caption(ui, if done_n > 0 { "All done." } else { "Nothing yet \u{2014} anything you type above lands here." });
    }
    for item in &open {
        row(ui, item, &mut s.editing, &mut act);
    }

    if done_n > 0 {
        ui.add_space(space::LG);
        face::disclosure(ui, egui::Id::new("todo:done"), "Done", Some(done_n), &mut s.done_open);
        if s.done_open {
            ui.add_space(space::XS);
            let done: Vec<Item> = s.items.iter().filter(|i| i.done).cloned().collect();
            for item in &done {
                row(ui, item, &mut s.editing, &mut act);
            }
        }
    }
    ui.add_space(space::XXL);

    match act {
        Some(Act::Toggle(id)) => {
            if let Some(i) = s.items.iter_mut().find(|i| i.id == id) {
                i.done = !i.done;
            }
            save(s);
        }
        Some(Act::Remove(id)) => {
            s.items.retain(|i| i.id != id);
            save(s);
        }
        Some(Act::Edit(id, text)) => s.editing = Some((id, text)),
        Some(Act::Save) => {
            if let Some((id, text)) = s.editing.take() {
                let text = text.trim().to_owned();
                if text.is_empty() {
                    s.items.retain(|i| i.id != id);
                } else if let Some(i) = s.items.iter_mut().find(|i| i.id == id) {
                    i.text = text;
                }
                save(s);
            }
        }
        None => {}
    }
    if clear {
        s.items.retain(|i| !i.done);
        save(s);
    }
}

/// One line: a round check, the words (double-click to edit), and a remove ×.
fn row(ui: &mut egui::Ui, item: &Item, editing: &mut Option<(u64, String)>, act: &mut Option<Act>) {
    let width = ui.available_width().min(720.0);
    ui.horizontal(|ui| {
        ui.set_width(width);
        ui.set_min_height(size::ROW);
        ui.spacing_mut().item_spacing.x = space::SM;

        // The check.
        let (r, check) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
        check.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, item.done, &item.text)
        });
        let hot = check.hovered() || check.has_focus();
        let p = ui.painter();
        let c = r.center();
        if item.done {
            p.circle_filled(c, 9.0, colour::OK());
            p.text(c, egui::Align2::CENTER_CENTER, icon::CHECK, egui::FontId::proportional(text::CAPTION), colour::CANVAS());
        } else {
            p.circle_stroke(c, 8.5, egui::Stroke::new(1.5, if hot { colour::TEXT_2() } else { colour::TEXT_FAINT() }));
        }
        if hot {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if check.clicked() {
            *act = Some(Act::Toggle(item.id));
        }

        // The words, or the field editing them.
        let mine = editing.as_mut().filter(|(id, _)| *id == item.id);
        if let Some((_, draft)) = mine {
            let f = ui.add(egui::TextEdit::singleline(draft).desired_width(width - 80.0).font(egui::FontId::proportional(text::BODY)));
            if !f.has_focus() && !f.lost_focus() {
                f.request_focus();
            }
            if f.lost_focus() {
                *act = Some(Act::Save);
            }
        } else {
            let words = RichText::new(&item.text).size(text::BODY);
            let words = if item.done {
                words.strikethrough().color(colour::TEXT_FAINT())
            } else {
                words.color(colour::TEXT())
            };
            let l = ui.add(egui::Label::new(words).wrap().sense(Sense::click()));
            if l.double_clicked() {
                *act = Some(Act::Edit(item.id, item.text.clone()));
            }
            l.on_hover_text("Double-click to edit");
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let x = ui.add(
                egui::Button::new(RichText::new(icon::X).size(text::SMALL).color(colour::TEXT_FAINT()))
                    .frame(false)
                    .corner_radius(radius::SM),
            );
            x.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Remove"));
            if x.on_hover_text("Remove").clicked() {
                *act = Some(Act::Remove(item.id));
            }
        });
    });
}
