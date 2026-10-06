use std::sync::{Mutex, PoisonError};

use egui::{Event, Key, Modifiers, ViewportId};

const CMD: Modifiers = Modifiers::MAC_CMD.plus(Modifiers::COMMAND);

const SETTINGS: &str = "loop.settings";
const UNDO: &str = "edit.undo";
const REDO: &str = "edit.redo";
const CUT: &str = "edit.cut";
const COPY: &str = "edit.copy";
const PASTE: &str = "edit.paste";
const SELECT_ALL: &str = "edit.select-all";

static PICKED: Mutex<Vec<String>> = Mutex::new(Vec::new());
static HELD: Mutex<Vec<(ViewportId, Held)>> = Mutex::new(Vec::new());

enum Held {
    Events(Vec<Event>),
    Paste,
}

#[cfg(target_os = "macos")]
pub fn install(ctx: &egui::Context, version: &str) {
    let ctx = ctx.clone();
    muda::MenuEvent::set_event_handler(Some(move |e: muda::MenuEvent| {
        pick(&e.id.0);
        ctx.request_repaint();
    }));
    match build(version) {
        Ok(menu) => {
            menu.init_for_nsapp();
            std::mem::forget(menu);
        }
        Err(e) => tracing::warn!("menu bar not installed: {e}"),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn install(_ctx: &egui::Context, _version: &str) {}

#[cfg(target_os = "macos")]
fn build(version: &str) -> muda::Result<muda::Menu> {
    use muda::accelerator::{Accelerator, Code, Modifiers as Mods, CMD_OR_CTRL};
    use muda::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem as Pre, Submenu};

    let item = |id: &str, text: &str, mods: Mods, key: Code| {
        MenuItem::with_id(id, text, true, Some(Accelerator::new(mods, key)))
    };
    let about = AboutMetadata { name: Some("Loop".into()), version: Some(version.into()), ..Default::default() };

    let app = Submenu::with_items(
        "Loop",
        true,
        &[
            &Pre::about(Some("About Loop"), Some(about)),
            &Pre::separator(),
            &item(SETTINGS, "Settings\u{2026}", CMD_OR_CTRL, Code::Comma),
            &Pre::separator(),
            &Pre::services(None),
            &Pre::separator(),
            &Pre::hide(Some("Hide Loop")),
            &Pre::hide_others(None),
            &Pre::show_all(None),
            &Pre::separator(),
            &Pre::quit(Some("Quit Loop")),
        ],
    )?;
    let edit = Submenu::with_items(
        "Edit",
        true,
        &[
            &item(UNDO, "Undo", CMD_OR_CTRL, Code::KeyZ),
            &item(REDO, "Redo", CMD_OR_CTRL | Mods::SHIFT, Code::KeyZ),
            &Pre::separator(),
            &item(CUT, "Cut", CMD_OR_CTRL, Code::KeyX),
            &item(COPY, "Copy", CMD_OR_CTRL, Code::KeyC),
            &item(PASTE, "Paste", CMD_OR_CTRL, Code::KeyV),
            &item(SELECT_ALL, "Select All", CMD_OR_CTRL, Code::KeyA),
        ],
    )?;
    let window = Submenu::with_items(
        "Window",
        true,
        &[
            &Pre::minimize(None),
            &Pre::zoom(None),
            &Pre::fullscreen(None),
            &Pre::separator(),
            &Pre::close_window(None),
            &Pre::separator(),
            &Pre::bring_all_to_front(None),
        ],
    )?;
    let help = Submenu::new("Help", true);
    let menu = Menu::with_items(&[&app, &edit, &window, &help])?;
    window.set_as_windows_menu_for_nsapp();
    help.set_as_help_menu_for_nsapp();
    Ok(menu)
}

pub fn paste() {
    pick(PASTE);
}

pub fn pick(id: &str) {
    PICKED.lock().unwrap_or_else(PoisonError::into_inner).push(id.to_owned());
}

pub fn route(raw: &mut egui::RawInput) {
    let picked = std::mem::take(&mut *PICKED.lock().unwrap_or_else(PoisonError::into_inner));
    if picked.is_empty() {
        return;
    }
    let focused = raw
        .viewports
        .iter()
        .find(|(_, v)| v.focused == Some(true))
        .map_or(ViewportId::ROOT, |(id, _)| *id);
    let mut held = HELD.lock().unwrap_or_else(PoisonError::into_inner);
    for id in picked {
        let (to, events) = match id.as_str() {
            SETTINGS => (ViewportId::ROOT, chord(Key::Comma, CMD)),
            UNDO => (focused, chord(Key::Z, CMD)),
            REDO => (focused, chord(Key::Z, CMD.plus(Modifiers::SHIFT))),
            SELECT_ALL => (focused, chord(Key::A, CMD)),
            CUT => (focused, vec![Event::Cut]),
            COPY => (focused, vec![Event::Copy]),
            PASTE => {
                held.push((focused, Held::Paste));
                continue;
            }
            _ => continue,
        };
        if to == ViewportId::ROOT {
            raw.events.extend(events);
        } else {
            held.push((to, Held::Events(events)));
        }
    }
}

pub fn deliver(ctx: &egui::Context) {
    let here = ctx.viewport_id();
    let mine: Vec<Held> = {
        let mut held = HELD.lock().unwrap_or_else(PoisonError::into_inner);
        let (mine, rest) = std::mem::take(&mut *held).into_iter().partition(|(to, _)| *to == here);
        *held = rest;
        mine.into_iter().map(|(_, h)| h).collect()
    };
    for h in mine {
        match h {
            Held::Events(events) => ctx.input_mut(|i| i.events.extend(events)),
            Held::Paste => {
                let frame = ctx.cumulative_frame_nr();
                ctx.data_mut(|d| d.insert_temp(egui::Id::new((PASTED, here)), frame));
                if let Some(text) = clipboard_text() {
                    ctx.input_mut(|i| i.events.push(Event::Paste(text)));
                }
            }
        }
    }
}

const PASTED: &str = "menu:pasted";

fn clipboard_text() -> Option<String> {
    let text = arboard::Clipboard::new().ok()?.get_text().ok()?.replace("\r\n", "\n");
    (!text.is_empty()).then_some(text)
}

pub fn pasted(ctx: &egui::Context) -> bool {
    let id = egui::Id::new((PASTED, ctx.viewport_id()));
    let frame = ctx.cumulative_frame_nr();
    ctx.data_mut(|d| d.remove_temp::<u64>(id)).is_some_and(|at| at == frame)
}

fn chord(key: Key, modifiers: Modifiers) -> Vec<Event> {
    [true, false]
        .map(|pressed| Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers })
        .into()
}
