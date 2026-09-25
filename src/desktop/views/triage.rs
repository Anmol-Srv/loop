//! Triage: tasks an intake agent filed for someone, waiting on a yes or a no.
//!
//! Linear's Triage is the model — a compact row per item, the source it came
//! from underneath, and the decision a hover away. The Triage tab draws the
//! list, My Tasks a line that leads to it, the task page the source card and
//! Accept / Dismiss, and every row's right-click menu offers them too (see
//! `menus::task_items`). The requests themselves go through `menus::Tasks`,
//! like every other task action, so the toast and the invalidation are one.

use egui::{RichText, Sense, Vec2};
use serde_json::Value;

use super::menus::{Pick, Viewer};
use super::projects::{PROJECTS_KEY, PROSE_W};
use crate::desktop::design::{
    cards as c, colour, glyph, motion, radius, shell, size, space, text, theme, viz, widgets as w,
};
use crate::desktop::net::Net;

pub(super) const NOT_YOURS: &str = "Only the person it was filed for, or an admin, can triage it.";

/// Two lines — the title, then where it came from — at a list row's pitch.
const ROW_H: f32 = 56.0;

/// The categories an intake agent files under, wire value first.
pub(super) const CATEGORIES: [(&str, &str); 5] =
    [("bug", "Bug"), ("feature", "Feature"), ("feedback", "Feedback"), ("question", "Question"), ("chore", "Chore")];

pub(super) fn category_label(value: &str) -> &str {
    CATEGORIES.iter().find(|(v, _)| *v == value).map_or(value, |(_, l)| l)
}

/// One tinted chip per category, from the chip tones: every one of them
/// clears 4.5:1 on its own fill.
pub(super) fn category_tone(value: &str) -> c::Tone {
    match value {
        "bug" => c::Tone::Blocked,
        "feature" => c::Tone::Agent,
        "feedback" => c::Tone::Info,
        _ => c::Tone::Quiet,
    }
}

/// The projects a triaged task can be accepted into, from the list the
/// Projects page keeps: (id, name), live ones only.
pub(super) fn projects(net: &Net) -> Vec<(String, String)> {
    net.data(PROJECTS_KEY)
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter(|p| !super::board::archived(p))
                .filter_map(|p| Some((str_of(p, "id")?.to_owned(), str_of(p, "name")?.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

/// Ask for that list, on a page that has triage on it.
pub(super) fn want_projects(net: &mut Net) {
    net.get_once(PROJECTS_KEY, "/api/user/projects");
}

/// Whether the viewer may decide on `t`.
pub(super) fn decides(t: &Value, viewer: &Viewer) -> bool {
    viewer.admin || (!viewer.me.is_empty() && str_of(t, "assigneePersonId") == Some(viewer.me.as_str()))
}

// ------------------------------------------------------------------ the source

fn source_of(t: &Value) -> Option<&Value> {
    t.get("source").filter(|s| s.is_object())
}

/// A DM: the request said so, or the channel is a Slack DM id.
fn private(src: &Value) -> bool {
    src.get("private").and_then(Value::as_bool).unwrap_or(false)
        || str_of(src, "channel").is_some_and(|c| c.starts_with('D'))
}

/// "#issues-and-feedback", "Direct message", or nothing for a bare channel id
/// that has no name to show.
fn channel(src: &Value) -> Option<String> {
    if private(src) {
        return Some("Direct message".to_owned());
    }
    let raw = str_of(src, "channelName").or_else(|| str_of(src, "channel"))?.trim();
    if raw.is_empty() {
        return None;
    }
    if raw.starts_with('#') {
        return Some(raw.to_owned());
    }
    // "C04AB12CD": an id, not a name. Better nothing than a code.
    let id_like = raw.len() >= 9 && raw.chars().all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit());
    (!id_like).then(|| format!("#{raw}"))
}

fn kind_word(kind: &str) -> String {
    match kind {
        "slack" => "Slack".to_owned(),
        "github" => "GitHub".to_owned(),
        other => {
            let mut ch = other.chars();
            ch.next().map(|f| f.to_uppercase().chain(ch).collect()).unwrap_or_else(|| "Intake".to_owned())
        }
    }
}

/// "Slack · #issues-and-feedback · Priya · 12m". The author is absent for a
/// teammate reading someone else's DM, and so is left out.
pub(super) fn source_words(src: &Value, short_age: bool) -> String {
    let mut parts = vec![kind_word(str_of(src, "kind").unwrap_or("slack"))];
    parts.extend(channel(src));
    if let Some(a) = str_of(src, "author").map(str::trim).filter(|a| !a.is_empty()) {
        parts.push(a.to_owned());
    }
    if let Some(at) = str_of(src, "receivedAt") {
        let age = if short_age { super::mytasks::age(at) } else { super::task::ago(at) };
        if !age.is_empty() {
            parts.push(age);
        }
    }
    parts.join(" \u{00B7} ")
}

/// The source's mark, at `side`.
fn mark(ui: &mut egui::Ui, src: &Value, side: f32, ink: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
    glyph::source(ui.painter(), rect.center(), side, str_of(src, "kind").unwrap_or("slack"), ink);
}

/// The source, as a card under the description: who said it, where and
/// when, the message itself rendered from Slack's mrkdwn as a quote, the
/// images that came with it, and why the agent filed it.
///
/// A teammate reading a task filed from someone's DM gets the fact that it
/// came from one and nothing of what was said — the server withholds the
/// text, the author and the files; this only declines to invent them.
pub(super) fn source_card(ui: &mut egui::Ui, net: &mut Net, t: &Value) {
    let Some(src) = source_of(t) else { return };
    // A DM read by a teammate: the server sends a stand-in for the text and
    // no author. The stand-in is not the message, so it is not quoted.
    let withheld = private(src) && str_of(src, "author").is_none();
    let said = str_of(src, "text").map(str::trim).filter(|s| !s.is_empty() && !withheld);
    let files: &[Value] = src.get("files").and_then(Value::as_array).map_or(&[], Vec::as_slice);
    let readable = said.is_some() || !files.is_empty();
    let owner = str_of(t, "assigneeName").and_then(|n| n.split_whitespace().next()).unwrap_or("its owner");

    shell::section(ui, "Source");
    ui.scope(|ui| {
        ui.set_max_width(PROSE_W.min(ui.available_width()));
        w::card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = space::SM;
            header(ui, src, readable);
            if readable {
                quote(ui, net, said, files);
            } else if private(src) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::XS;
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(text::SMALL + 1.0), Sense::hover());
                    glyph::lock(ui.painter(), r.center(), r.width(), colour::TEXT_MUTED);
                    w::muted(ui, &format!("From a direct message \u{2014} only {owner} can read it."));
                });
            }
            filed_by(ui, src);
        });
    });
    lightbox(ui.ctx(), net);
    open_pending(ui.ctx(), net);
}

/// Who said it, then where and when; "Open in Slack" at the far end.
fn header(ui: &mut egui::Ui, src: &Value, readable: bool) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::XS;
        mark(ui, src, text::BODY, colour::TEXT_MUTED);
        let url = str_of(src, "url").filter(|u| u.starts_with("http"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(url) = url.filter(|_| readable) {
                let label = match str_of(src, "kind") {
                    Some("slack") | None => "Open in Slack".to_owned(),
                    Some(k) => format!("Open in {}", kind_word(k)),
                };
                if w::link(ui, &label).on_hover_text(url).clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                }
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = space::XS;
                let author = str_of(src, "author").map(str::trim).is_some_and(|a| !a.is_empty());
                if let Some(a) = str_of(src, "author").map(str::trim).filter(|a| !a.is_empty()) {
                    ui.add(
                        egui::Label::new(
                            RichText::new(a)
                                .size(text::BODY)
                                .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                                .color(colour::TEXT),
                        )
                        .selectable(false),
                    );
                }
                let mut rest: Vec<String> = author.then(String::new).into_iter().collect();
                rest.extend(channel(src));
                let at = str_of(src, "receivedAt");
                if let Some(age) = at.map(super::task::ago).filter(|a| !a.is_empty()) {
                    rest.push(age);
                }
                if rest.len() <= usize::from(author) {
                    rest.push(kind_word(str_of(src, "kind").unwrap_or("slack")));
                }
                let r = ui.add(
                    egui::Label::new(RichText::new(rest.join(" \u{00B7} ").trim_start()).size(text::SMALL).color(colour::TEXT_MUTED))
                        .truncate(),
                );
                if let Some(at) = at {
                    r.on_hover_text(super::task::exact(at));
                }
            });
        });
    });
}

/// The original words, as a quote: a rule down the left and the message
/// beside it, its images underneath. Not a bubble — this is a record of what
/// was said, not a conversation.
fn quote(ui: &mut egui::Ui, net: &mut Net, said: Option<&str>, files: &[Value]) {
    let out = egui::Frame::new()
        .inner_margin(egui::Margin { left: space::MD as i8, right: 0, top: space::XXS as i8, bottom: space::XXS as i8 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = space::SM;
            if let Some(said) = said {
                super::mrkdwn::show(ui, said, colour::TEXT_2);
            }
            if !files.is_empty() {
                attachments(ui, net, files);
            }
        });
    let r = out.response.rect;
    ui.painter().rect_filled(
        egui::Rect::from_min_size(r.min, Vec2::new(space::XXS, r.height())),
        radius::SM as f32,
        colour::LINE_STRONG,
    );
}

// ------------------------------------------------------------------ files

/// A thumbnail's bounds: wide enough to read a screenshot's shape, small
/// enough that three sit in a row of the prose column.
const THUMB_W: f32 = 240.0;
const THUMB_H: f32 = 180.0;
/// The side a texture is kept at, past which it is scaled down on decode.
const MAX_TEXTURE: u32 = 4096;
const LIGHTBOX: &str = "source:lightbox";
const OPENING: &str = "source:opening";

fn file_key(id: &str) -> String {
    format!("file:{id}")
}

fn want(net: &mut Net, id: &str) {
    net.get_bytes_once(&file_key(id), &format!("/api/user/files/{id}"));
}

/// A file's texture, decoded once and kept in the temp store. The lookup and
/// the load are separate statements: `load_texture` takes the context lock
/// `data_mut` holds (see `shell::mark_texture`).
fn texture(ctx: &egui::Context, net: &Net, id: &str) -> Option<Result<egui::TextureHandle, String>> {
    let tid = egui::Id::new(("source:texture", id));
    if let Some(held) = ctx.data(|d| d.get_temp::<Result<egui::TextureHandle, String>>(tid)) {
        return Some(held);
    }
    let decoded = match net.bytes(&file_key(id))? {
        Err(e) => Err(e.clone()),
        Ok(bytes) => match image::load_from_memory(bytes) {
            Err(e) => Err(format!("This image could not be read ({e}).")),
            Ok(img) => {
                let img = if img.width().max(img.height()) > MAX_TEXTURE { img.thumbnail(MAX_TEXTURE, MAX_TEXTURE) } else { img };
                let rgba = img.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                Ok(ctx.load_texture(
                    file_key(id),
                    egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
                    egui::TextureOptions::LINEAR,
                ))
            }
        },
    };
    ctx.data_mut(|d| d.insert_temp(tid, decoded.clone()));
    Some(decoded)
}

/// "120 KB", "2.4 MB".
fn file_size(bytes: i64) -> String {
    match bytes {
        b if b < 1024 => format!("{b} B"),
        b if b < 1024 * 1024 => format!("{} KB", b / 1024),
        b => format!("{:.1} MB", b as f64 / (1024.0 * 1024.0)),
    }
}

/// Images as a grid of thumbnails, anything else as a file chip under them.
fn attachments(ui: &mut egui::Ui, net: &mut Net, files: &[Value]) {
    let (images, others): (Vec<&Value>, Vec<&Value>) =
        files.iter().partition(|f| str_of(f, "mime").is_some_and(|m| m.starts_with("image/")));
    if !images.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(space::SM);
            for f in images {
                thumbnail(ui, net, f);
            }
        });
    }
    if !others.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(space::SM);
            for f in others {
                file_chip(ui, net, f);
            }
        });
    }
}

fn thumbnail(ui: &mut egui::Ui, net: &mut Net, f: &Value) {
    let (Some(id), name) = (str_of(f, "id"), str_of(f, "name").unwrap_or("image")) else { return };
    want(net, id);
    let tex = texture(ui.ctx(), net, id);
    if let Some(Err(e)) = &tex {
        // Unreadable here: still a file, still openable where it can be read.
        file_chip(ui, net, f).on_hover_text(e.as_str());
        return;
    }
    let natural = match &tex {
        Some(Ok(t)) => t.size_vec2(),
        _ => {
            let dim = |k: &str| f.get(k).and_then(Value::as_f64).filter(|v| *v > 0.0).map(|v| v as f32);
            dim("width").zip(dim("height")).map_or(Vec2::new(THUMB_W, THUMB_H * 0.75), |(w, h)| Vec2::new(w, h))
        }
    };
    let scale = (THUMB_W / natural.x).min(THUMB_H / natural.y).min(1.0);
    let shown = (natural * scale).max(Vec2::splat(space::XXL));
    let (rect, response) = ui.allocate_exact_size(shown, Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Open image {name}")));
    let response = motion::operable(ui, response, radius::MD as f32);
    let p = ui.painter();
    match &tex {
        Some(Ok(t)) => egui::Image::new((t.id(), shown)).corner_radius(radius::MD).paint_at(ui, rect),
        _ => {
            p.rect_filled(rect, radius::MD as f32, colour::INSET);
            glyph::evidence(p, rect.center(), text::HEADING, "doc", colour::TEXT_FAINT);
        }
    }
    let hot = response.hovered() || response.has_focus();
    p.rect_stroke(
        rect,
        radius::MD as f32,
        egui::Stroke::new(1.0, if hot { colour::LINE_STRONG } else { colour::LINE_SOFT }),
        egui::StrokeKind::Inside,
    );
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let response = response.on_hover_text(name);
    if response.clicked() {
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new(LIGHTBOX), (id.to_owned(), name.to_owned())));
    }
}

/// A file that is not an image: a page mark, its name and size. A click
/// opens it with the Mac's own viewer, once its bytes are here.
fn file_chip(ui: &mut egui::Ui, net: &mut Net, f: &Value) -> egui::Response {
    let id = str_of(f, "id").unwrap_or_default().to_owned();
    let name = str_of(f, "name").unwrap_or("file");
    let size = f.get("size").and_then(Value::as_i64).map(file_size).unwrap_or_default();
    let words = if size.is_empty() { name.to_owned() } else { format!("{name} \u{00B7} {size}") };
    let galley = ui.painter().layout_no_wrap(words.clone(), egui::FontId::proportional(text::SMALL), colour::TEXT_2);
    let height = size::CONTROL;
    let width = (galley.size().x + text::BODY + space::SM * 2.0 + space::XS).min(THUMB_W * 1.5);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Open {name}")));
    let response = motion::operable_sm(ui, response);
    let hot = response.hovered() || response.has_focus();
    let p = ui.painter();
    p.rect_filled(rect, radius::SM as f32, if hot { colour::GLASS_HOVER } else { colour::GLASS });
    p.rect_stroke(rect, radius::SM as f32, egui::Stroke::new(1.0, if hot { colour::EDGE_HI_HOVER } else { colour::EDGE_MID }), egui::StrokeKind::Inside);
    let icon = egui::pos2(rect.left() + space::SM + text::BODY / 2.0, rect.center().y);
    glyph::evidence(p, icon, text::BODY, "doc", colour::TEXT_MUTED);
    let text_rect = egui::Rect::from_min_max(egui::pos2(icon.x + text::BODY / 2.0 + space::XS, rect.top()), rect.max - Vec2::new(space::SM, 0.0));
    let shown = w::truncated(ui, &words, egui::FontId::proportional(text::SMALL), colour::TEXT_2, text_rect.width());
    p.galley(egui::pos2(text_rect.left(), rect.center().y - shown.size().y / 2.0), shown, colour::TEXT_2);
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if response.clicked() && !id.is_empty() {
        want(net, &id);
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new(OPENING), (id, name.to_owned())));
    }
    response
}

/// A file chip clicked: once its bytes land, write them where the Mac can
/// open them and hand the file over.
fn open_pending(ctx: &egui::Context, net: &Net) {
    let key = egui::Id::new(OPENING);
    let Some((id, name)) = ctx.data(|d| d.get_temp::<(String, String)>(key)) else { return };
    let Some(got) = net.bytes(&file_key(&id)) else { return };
    ctx.data_mut(|d| d.remove::<(String, String)>(key));
    let written = got.clone().and_then(|bytes| {
        let dir = std::env::temp_dir().join("loop-files");
        let safe: String = name.chars().map(|c| if c == '/' || c == '\\' { '-' } else { c }).collect();
        let path = dir.join(format!("{}-{safe}", &id[..8.min(id.len())]));
        std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, bytes.as_slice())).map_err(|e| e.to_string())?;
        Ok(path)
    });
    match written {
        Ok(path) => ctx.open_url(egui::OpenUrl::new_tab(format!("file://{}", path.display()))),
        Err(e) => w::toast(ctx, format!("Could not open {name}: {e}"), true),
    }
}

/// The full image over the page, with a close button; Escape or a click
/// outside closes it too.
fn lightbox(ctx: &egui::Context, net: &Net) {
    let key = egui::Id::new(LIGHTBOX);
    let Some((id, name)) = ctx.data(|d| d.get_temp::<(String, String)>(key)) else { return };
    let tex = texture(ctx, net, &id);
    let screen = ctx.content_rect();
    let room = Vec2::new(screen.width() * 0.8, screen.height() * 0.72);
    let natural = match &tex {
        Some(Ok(t)) => t.size_vec2(),
        _ => Vec2::new(THUMB_W * 2.0, THUMB_H * 2.0),
    };
    let shown = natural * (room.x / natural.x).min(room.y / natural.y).min(1.0);
    let mut close = false;
    let modal = super::agents::dialog(ctx, LIGHTBOX, shown.x.max(super::agents::DIALOG_W * 0.8), |ui| {
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = w::ghost(ui, "Close").clicked();
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(&name)
                                .size(text::HEADING)
                                .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                                .color(colour::TEXT),
                        )
                        .truncate(),
                    );
                });
            });
        });
        ui.add_space(space::MD);
        match &tex {
            Some(Ok(t)) => {
                ui.vertical_centered(|ui| {
                    let r = ui.add(egui::Image::new((t.id(), shown)).corner_radius(radius::MD));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, name.as_str()));
                });
            }
            Some(Err(e)) => w::error(ui, e),
            None => w::loading(ui, "Loading image"),
        }
    });
    if close || modal.should_close() {
        ctx.data_mut(|d| d.remove::<(String, String)>(key));
    }
}

/// "Filed by Slack Agent — looks like a bug: … · 0.86".
fn filed_by(ui: &mut egui::Ui, src: &Value) {
    let agent = str_of(src, "agentName")
        .or_else(|| src.get("agent").and_then(|a| str_of(a, "name")))
        .unwrap_or("an intake agent");
    let mut line = format!("Filed by {agent}");
    if let Some(reason) = str_of(src, "reason").map(str::trim).filter(|r| !r.is_empty()) {
        line += &format!(" \u{2014} {reason}");
    }
    let confidence = src.get("confidence").and_then(Value::as_f64);
    if let Some(c) = confidence {
        line += &format!(" \u{00B7} {c:.2}");
    }
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = space::XS;
        w::agent_mark(ui, text::BODY);
        // Wraps: an agent's reason can run long, and an unwrapped line in a
        // horizontal row widens the whole column and pushes the rail off-screen.
        let r = ui.add(egui::Label::new(RichText::new(line).size(text::SMALL).color(colour::TEXT_MUTED)).wrap());
        if confidence.is_some() {
            r.on_hover_text("The last figure is how sure the agent was, from 0 to 1.");
        }
    });
}

// ------------------------------------------------------------------ category

/// The category chip, and — for someone who may edit it — a menu that sets
/// it. Returns the category picked this frame. Without a category, an
/// editor gets a quiet "Category" chip to set one; a reader gets nothing.
pub(super) fn category_chip(ui: &mut egui::Ui, current: Option<&str>, editable: bool) -> Option<String> {
    let chip = match current {
        Some(v) => c::chip(ui, category_label(v), category_tone(v), false),
        None if editable => c::chip(ui, "Category", c::Tone::Neutral, false),
        None => return None,
    };
    if !editable {
        return None;
    }
    let id = chip.id.with("pick");
    let r = ui.interact(chip.rect, id, Sense::click());
    r.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::ComboBox,
            true,
            format!("Category: {}", current.map_or("none", category_label)),
        )
    });
    let r = motion::operable_sm(ui, r);
    if r.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        ui.painter().rect_stroke(chip.rect, radius::SM as f32, egui::Stroke::new(1.0, colour::LINE_STRONG), egui::StrokeKind::Inside);
    }
    let mut picked = None;
    viz::click_menu(&r, |ui| {
        for (value, label) in CATEGORIES {
            if viz::menu_choice(ui, label, current == Some(value)) && current != Some(value) {
                picked = Some(value.to_owned());
            }
        }
    });
    picked
}

// ------------------------------------------------------------------ decisions

/// The task page's decision: Accept as a secondary button, Dismiss as a ghost
/// beside it. Accept into another project lives in the page's ⋯ menu, which
/// draws the same items as a row's right-click. Right to left: call from a
/// right-to-left layout.
pub(super) fn page_actions(ui: &mut egui::Ui, t: &Value) -> Option<Pick> {
    let hover = match str_of(t, "projectName").filter(|n| !n.is_empty()) {
        Some(p) => format!("Make it an open task in {p}"),
        None => "Make it an open task, in no project".to_owned(),
    };
    let mut pick = None;
    if w::secondary(ui, "Accept", true).on_hover_text(hover).clicked() {
        pick = Some(Pick::Accept(None));
    }
    if w::ghost(ui, "Dismiss").on_hover_text("Drop it, with an optional reason").clicked() {
        pick = Some(Pick::Dismiss);
    }
    pick
}

/// A row's icon button: square, this side.
const ICON_BTN: f32 = 26.0;

/// A ghost icon button painted at `rect`: nothing at rest but the glyph, a
/// soft fill on hover. `alpha` is the row's reveal, so the pair fades in and
/// out with the row's hover rather than popping.
fn icon_action(ui: &mut egui::Ui, rect: egui::Rect, id: egui::Id, name: String, tip: &str, alpha: f32, accept: bool) -> egui::Response {
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name.as_str()));
    let response = motion::operable_sm(ui, response);
    let hot = response.hovered() || response.has_focus();
    let fill = motion::hover_fill(ui, id.with("fill"), hot, colour::TRANSPARENT, colour::GLASS_HOVER);
    let p = ui.painter();
    if fill != colour::TRANSPARENT {
        p.rect_filled(rect, radius::SM as f32, fill.gamma_multiply(alpha));
    }
    let ink = if hot { colour::TEXT } else { colour::TEXT_MUTED }.gamma_multiply(alpha);
    if accept {
        glyph::tick(p, rect.center(), text::BODY, ink);
    } else {
        glyph::cross(p, rect.center(), text::BODY, ink);
    }
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.on_hover_text(tip)
}

// ------------------------------------------------------------------ My Tasks

/// "3 to triage", one quiet line at the top of My Tasks that opens the
/// Triage tab. True when clicked.
pub(super) fn link_row(ui: &mut egui::Ui, n: usize) -> bool {
    let words = format!("{n} to triage");
    let font = egui::FontId::proportional(text::BODY);
    let galley = ui.painter().layout_no_wrap(words.clone(), font, colour::TEXT_2);
    // Flush with the page's left column; the hover fill bleeds past it.
    let width = text::HEADING + space::XS + galley.size().x + space::SM + text::SMALL;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, size::CONTROL), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Link, true, words.as_str()));
    let response = motion::operable_sm(ui, response);
    let hot = response.hovered() || response.has_focus();
    let fill = motion::hover_fill(ui, response.id.with("hover"), hot, colour::TRANSPARENT, colour::GLASS_HOVER);
    let p = ui.painter();
    if fill != colour::TRANSPARENT {
        p.rect_filled(rect.expand2(Vec2::new(space::SM, 0.0)), radius::SM as f32, fill);
    }
    let ink = if hot { colour::TEXT } else { colour::TEXT_2 };
    let mut x = rect.left();
    p.text(
        egui::pos2(x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        egui_phosphor::thin::TRAY,
        egui::FontId::proportional(text::HEADING),
        colour::TEXT_MUTED,
    );
    x += text::HEADING + space::XS;
    p.galley(egui::pos2(x, rect.center().y - galley.size().y / 2.0), galley, ink);
    let caret_x = rect.right() - text::SMALL / 2.0;
    glyph::caret(p, egui::pos2(caret_x, rect.center().y), text::SMALL, 0.0, if hot { colour::TEXT } else { colour::TEXT_FAINT });
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.on_hover_text("Open Triage").clicked()
}

// ------------------------------------------------------------------ the tab

/// What the list asked for: a task to open, or a decision.
enum Out {
    Open(String),
    Pick(Value, Pick),
}

/// The Triage tab: what intake agents filed for you, newest first, waiting
/// on a yes or a no. Stays put when the last one is decided, on the empty
/// state, rather than bouncing you somewhere else.
pub fn page(app: &mut crate::desktop::App, ui: &mut egui::Ui) {
    let viewer = Viewer::of(app);
    let net = app.net.as_mut().expect("signed in");
    net.get_once(super::mytasks::MINE, "/api/user/tasks/mine");
    want_projects(net);
    let viewer = Viewer { projects: projects(net), ..viewer };
    let loading = net.is_loading(super::mytasks::MINE);
    let error = net.error(super::mytasks::MINE).map(str::to_owned);
    let all = net.shared(super::mytasks::MINE);
    let mut rows: Vec<&Value> = all
        .as_deref()
        .and_then(Value::as_array)
        .map(|a| a.iter().filter(|t| str_of(t, "status") == Some("triage")).collect())
        .unwrap_or_default();
    rows.sort_by(|a, b| str_of(b, "createdAt").cmp(&str_of(a, "createdAt")));

    let subtitle = match rows.len() {
        0 => String::new(),
        1 => "1 waiting on a yes or a no".to_owned(),
        n => format!("{n} waiting on a yes or a no"),
    };
    shell::page_title(ui, "Triage", &subtitle, |_| {});

    if let Some(err) = error {
        w::error(ui, &format!("Could not load triage. {err} Use Refresh in the sidebar to try again."));
        return;
    }
    if rows.is_empty() {
        if loading && all.is_none() {
            w::loading(ui, "Loading triage");
        } else {
            w::empty(ui, "Nothing to triage.", "Tasks your intake agents file land here.");
        }
        return;
    }
    let out = list(ui, &rows, &viewer, app.board.tasks.deciding.as_deref());
    match out {
        Some(Out::Open(id)) => app.task = Some(id),
        Some(Out::Pick(t, p)) => app.board.tasks.pick(app.net.as_mut().unwrap(), &t, p),
        None => {}
    }
}

fn list(ui: &mut egui::Ui, rows: &[&Value], viewer: &Viewer, deciding: Option<&str>) -> Option<Out> {
    let mut out = None;
    w::card_list(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 0.0;
        for (i, t) in rows.iter().enumerate() {
            if i > 0 {
                let y = ui.cursor().top();
                ui.painter().hline(ui.max_rect().x_range(), y, egui::Stroke::new(1.0, colour::LINE_SOFT));
            }
            let busy = deciding.is_some() && deciding == str_of(t, "id");
            if let Some(o) = row(ui, t, viewer, busy) {
                out = Some(o);
            }
        }
    });
    out
}

/// One filing: category and title, then where it came from. Nothing to press
/// at rest; hover or focus fades in a tick and a cross at the right end, and
/// A or D decides from the keyboard. Accept into another project is on the
/// right-click menu.
fn row(ui: &mut egui::Ui, t: &Value, viewer: &Viewer, busy: bool) -> Option<Out> {
    let title = str_of(t, "title").unwrap_or("Untitled");
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title));
    let response = motion::operable_sm(ui, response);

    let mut pick = None;
    let menu_open = viz::context_menu(&response, |ui| {
        if let Some(p) = super::menus::task_items(ui, t, viewer, true) {
            pick = Some(p);
        }
    });
    // `contains_pointer`, not `hovered`: the icon buttons sit over the row,
    // and pointing at one must not put the row out.
    let decides = decides(t, viewer);
    let (accept_id, dismiss_id) = (response.id.with("accept"), response.id.with("dismiss"));
    let icon_focus = ui.memory(|m| m.has_focus(accept_id) || m.has_focus(dismiss_id));
    let lit = response.contains_pointer() || response.has_focus() || menu_open || icon_focus;
    let tint = motion::hover_fill(ui, response.id.with("hover"), lit, colour::TRANSPARENT, colour::SURFACE_HOVER);
    if tint != colour::TRANSPARENT {
        ui.painter().rect_filled(rect, radius::SM as f32, tint);
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    // The pair's room is kept whether or not it shows, so the title never
    // re-truncates under the pointer.
    let reserve = if decides { ICON_BTN * 2.0 + space::XXS + space::SM } else { 0.0 };
    if decides {
        let y = rect.center().y - ICON_BTN / 2.0;
        let dismiss = egui::Rect::from_min_size(egui::pos2(rect.right() - space::SM - ICON_BTN, y), Vec2::splat(ICON_BTN));
        let accept = dismiss.translate(Vec2::new(-(ICON_BTN + space::XXS), 0.0));
        if busy {
            let spin = egui::Rect::from_center_size(dismiss.center(), Vec2::splat(text::BODY));
            ui.put(spin, egui::Spinner::new().size(text::BODY));
        } else {
            let alpha = motion::to(ui, response.id.with("reveal"), lit, motion::FAST);
            if alpha > 0.0 {
                if icon_action(ui, accept, accept_id, format!("Accept {title}"), "Accept (A)", alpha, true).clicked() {
                    pick = Some(Pick::Accept(None));
                }
                if icon_action(ui, dismiss, dismiss_id, format!("Dismiss {title}"), "Dismiss (D)", alpha, false).clicked() {
                    pick = Some(Pick::Dismiss);
                }
            }
            // A and D, while the row is the one pointed at or focused — and
            // not while a text field has the keyboard.
            if lit && !menu_open && !ui.ctx().text_edit_focused() {
                let (a, d) = ui.input_mut(|i| {
                    (i.consume_key(egui::Modifiers::NONE, egui::Key::A), i.consume_key(egui::Modifiers::NONE, egui::Key::D))
                });
                if a {
                    pick = Some(Pick::Accept(None));
                } else if d {
                    pick = Some(Pick::Dismiss);
                }
            }
        }
    }

    // Two lines at fixed heights: nested layouts each take a control's height
    // and spill into the next row.
    let left = rect.left() + space::MD;
    let right = rect.right() - space::MD - reserve;
    let mut line = |top: f32, h: f32| {
        ui.new_child(
            egui::UiBuilder::new()
                .max_rect(egui::Rect::from_x_y_ranges(left..=right, top..=top + h))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        )
    };
    let mut first = line(rect.top() + space::SM, size::CONTROL - space::XS);
    first.spacing_mut().item_spacing.x = space::SM;
    if let Some(cat) = str_of(t, "category") {
        c::chip(&mut first, category_label(cat), category_tone(cat), false);
    }
    first.add(
        egui::Label::new(
            RichText::new(title)
                .size(text::BODY)
                .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                .color(colour::TEXT),
        )
        .truncate()
        .selectable(false),
    );
    if let Some(src) = source_of(t) {
        let mut second = line(rect.bottom() - space::SM - text::SMALL * 1.5, text::SMALL * 1.5);
        second.spacing_mut().item_spacing.x = space::XS;
        mark(&mut second, src, text::SMALL, colour::TEXT_MUTED);
        let mut words = source_words(src, true);
        let withheld = private(src) && str_of(src, "author").is_none();
        if let Some(said) = str_of(src, "text").map(str::trim).filter(|s| !s.is_empty() && !withheld) {
            words += &format!("  \u{201c}{}\u{201d}", super::mrkdwn::plain(said));
        }
        second.add(
            egui::Label::new(RichText::new(words).size(text::SMALL).color(colour::TEXT_MUTED))
                .truncate()
                .selectable(false),
        );
    }

    match pick {
        Some(Pick::Open) => str_of(t, "id").map(|id| Out::Open(id.to_owned())),
        Some(p) => Some(Out::Pick(t.clone(), p)),
        None if response.clicked() => str_of(t, "id").map(|id| Out::Open(id.to_owned())),
        None => None,
    }
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn source_words_read_like_a_line() {
        let s = json!({"kind": "slack", "channel": "C04AB12CD", "channelName": "issues-and-feedback", "author": "Priya"});
        assert_eq!(source_words(&s, true), "Slack \u{00B7} #issues-and-feedback \u{00B7} Priya");
        // A bare id with no name says nothing rather than a code.
        assert_eq!(source_words(&json!({"kind": "slack", "channel": "C04AB12CD"}), true), "Slack");
        // A DM, as a teammate sees it: no author, and never the id.
        assert_eq!(source_words(&json!({"kind": "slack", "channel": "D0123ABCD"}), true), "Slack \u{00B7} Direct message");
        assert_eq!(source_words(&json!({"channel": "#general", "private": false}), true), "Slack \u{00B7} #general");
    }
}
