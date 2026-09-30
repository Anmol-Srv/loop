//! Settings: things scoped to you, not to a project. Its own window, with its
//! own sidebar — opened from the account menu, ⌘,, or "Manage workspaces…".
//! `ui` also draws the same content into any `Ui`, so `Tab::Settings` (and a
//! render harness that cannot open a second window) still work.
//!
//! Account: who you are to the server, and where you're signed in.
//! Appearance: how Loop looks on this Mac.
//! Folders: where your agent works when a task has no project (or its
//! project has no folder for you). Private, like a repo's local path —
//! nobody else's list, and nobody else sees yours.
//! Members: admin only — the team, from `acp-admin` moved into the app.
//! Workspaces: every server you've signed in to, and the private one if
//! there is one.

use chrono::{DateTime, Utc};
use egui::RichText;
use egui_phosphor::regular as icon;
use serde_json::{json, Value};

use super::board::str_at;
use super::projects::PEOPLE_KEY;
use crate::desktop::creds;
use crate::desktop::design::{
    avatar, cards as c, colour, motion, radius, shell, size, space, text, theme, viz, widgets as w,
};
use crate::desktop::net::Net;
use crate::desktop::App;

pub(super) const FOLDERS_KEY: &str = "settings:folders";
const ACTION_KEY: &str = "settings:folders:action";

const MEMBERS_KEY: &str = "settings:members";
const MEMBERS_ACTION_KEY: &str = "settings:members:action";

/// The window's own left sidebar — narrower than the app's, since it lists
/// five destinations rather than a whole workspace.
const SIDEBAR_W: f32 = 208.0;
/// Room for the traffic lights, same idea as `shell::sidebar`'s.
const TRAFFIC_LIGHTS: f32 = 28.0;
/// A row is a target you scan, not data you pack tight — taller than a list
/// row.
const ROW_MIN_H: f32 = 46.0;
/// Bigger than a list avatar: this is the page about you.
const ACCOUNT_AVATAR: f32 = 48.0;

/// Which section of the window is showing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Account,
    Appearance,
    Folders,
    Members,
    Workspaces,
}

pub struct State {
    section: Section,
    /// The window is open on its own viewport. `false` on every workspace
    /// switch, so the next open starts on `Account`... no — it keeps
    /// whatever section it was on, which is what a settings window should do.
    window_open: bool,
    /// Set whenever `open_section`/`open_invite` runs, so `window` can bring
    /// an already-open window forward instead of leaving a second click
    /// looking like it did nothing.
    focus_pending: bool,
    adding: bool,
    name: String,
    path: String,
    error: Option<String>,
    removing: Option<String>,
    busy: bool,
    members: Members,
    /// The server of the workspace whose name is being edited inline, if any.
    renaming: Option<String>,
    rename_text: String,
    /// True for the one frame the rename field should claim focus.
    rename_focus: bool,
    /// The server of the workspace a remove confirm is asking about.
    ws_removing: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            section: Section::Account,
            window_open: false,
            focus_pending: false,
            adding: false,
            name: String::new(),
            path: String::new(),
            error: None,
            removing: None,
            busy: false,
            members: Members::default(),
            renaming: None,
            rename_text: String::new(),
            rename_focus: false,
            ws_removing: None,
        }
    }
}

/// What the setup-code result card, once it lands, needs to build the invite
/// message: whose it is and the code itself.
#[derive(Clone)]
struct InviteResult {
    name: String,
    email: String,
    code: String,
}

/// Which mutation `MEMBERS_ACTION_KEY` is carrying, so `settle_members` knows
/// what a success means: a new code to show, or nothing to say at all.
enum MemberAction {
    Add,
    Invite { name: String, email: String },
    Patch,
    Revoke,
}

#[derive(Default)]
struct Members {
    adding: bool,
    name: String,
    email: String,
    department: Option<String>,
    role: Option<String>,
    error: Option<String>,
    busy: bool,
    action: Option<MemberAction>,
    result: Option<InviteResult>,
    revoking: Option<String>,
}

/// Select a section and open the window — the account menu's "Settings…",
/// ⌘, and "Manage workspaces…".
pub fn open_section(s: &mut State, section: Section) {
    s.section = section;
    s.window_open = true;
    s.focus_pending = true;
}

/// Open Settings on Members with the invite form ready — the account menu's
/// "Invite people". Harmless for a non-admin: the tab never shows for them.
pub fn open_invite(s: &mut State) {
    s.section = Section::Members;
    s.window_open = true;
    s.focus_pending = true;
    if s.members.result.is_none() && !s.members.adding {
        start_member_add(s);
    }
}

const MEMBER_DEPARTMENTS: [&str; 3] = ["design", "frontend", "backend"];
const MEMBER_ROLES: [&str; 3] = ["member", "manager", "admin"];

/// The viewer's own folders, for pickers elsewhere (the task rail, triage's
/// menu). Empty until a page that needs them has asked — call `want_folders`
/// first.
pub(super) fn folders(net: &Net) -> Vec<Value> {
    net.data(FOLDERS_KEY)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

pub(super) fn want_folders(net: &mut Net) {
    net.get_once(FOLDERS_KEY, "/api/user/folders");
}

/// The name of the one folder with `isDefault`, if there is one.
pub(super) fn default_name(folders: &[Value]) -> Option<&str> {
    folders
        .iter()
        .find(|f| f.get("isDefault").and_then(Value::as_bool) == Some(true))
        .and_then(|f| f.get("name"))
        .and_then(Value::as_str)
}

// -------------------------------------------------------------------- window

/// Shown once a frame, from the chrome, while the window is open. A separate
/// macOS window with its own traffic lights and its own sidebar — cmux's
/// shape for the same idea.
pub fn window(app: &mut App, ctx: &egui::Context) {
    if !app.settings.window_open {
        return;
    }
    let id = egui::ViewportId::from_hash_of("loop-settings");
    let focus = std::mem::take(&mut app.settings.focus_pending);

    ctx.show_viewport_immediate(
        id,
        egui::ViewportBuilder::default()
            .with_title("Settings")
            .with_inner_size([860.0, 620.0])
            .with_min_inner_size([700.0, 480.0])
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false),
        |vui, class| {
            let vctx = vui.ctx().clone();
            if focus {
                vctx.send_viewport_cmd_to(id, egui::ViewportCommand::Focus);
            }
            let close = vctx.input(|i| {
                i.viewport().close_requested()
                    || i.key_pressed(egui::Key::Escape)
                    || (i.modifiers.command && i.key_pressed(egui::Key::W))
            });
            if close {
                app.settings.window_open = false;
                return;
            }

            if class == egui::ViewportClass::EmbeddedWindow {
                // A renderer or test harness that cannot open a second
                // window: the same content, in a plain egui window instead.
                egui::Window::new("Settings")
                    .default_size([860.0, 620.0])
                    .resizable(true)
                    .collapsible(false)
                    .show(&vctx, |ui| self::ui(app, ui));
            } else {
                self::ui(app, vui);
            }
        },
    );
}

// ----------------------------------------------------------------------- ui

/// The sidebar and the pane, drawn into whatever `Ui` is handed in — the
/// settings window's own viewport, or (for a harness that cannot open one)
/// `Tab::Settings`'s spot in the main window.
pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let can_write = app.can_write();
    let scopes = app.scopes.clone();

    let is_admin;
    let me_name;
    let me_email;
    let me_role;
    let me_department;
    let base_url;
    {
        let net = app.net.as_mut().expect("signed in");
        want_folders(net);
        settle(net, &mut app.settings);
        is_admin = net.data("__me").is_some_and(|m| str_at(m, "role") == "admin");
        if is_admin {
            net.get_once(MEMBERS_KEY, "/api/admin/people");
        }
        settle_members(ui.ctx(), net, &mut app.settings);

        let me = net.data("__me");
        let field = |k: &str| me.and_then(|m| m.get(k)).and_then(Value::as_str).unwrap_or("").to_owned();
        me_email = field("email");
        me_name = Some(field("name")).filter(|n| !n.is_empty()).unwrap_or_else(|| me_email.clone());
        me_role = field("role");
        me_department = field("department");
        base_url = net.base_url.clone();
    }

    // Members only shows for an admin; a stale selection (a role that
    // changed under someone, or a window reopened after a workspace switch)
    // falls back to Account rather than a blank pane.
    let section = if app.settings.section == Section::Members && !is_admin {
        Section::Account
    } else {
        app.settings.section
    };

    if let Some(picked) = sidebar(ui, section, is_admin) {
        app.settings.section = picked;
    }

    pane(ui, section, |ui| match section {
        Section::Account => {
            if account_section(ui, &me_name, &me_email, &me_role, &me_department, &scopes, &base_url) {
                app.sign_out();
                crate::desktop::views::chrome::forget_workspaces(ui.ctx());
                app.settings.window_open = false;
            }
        }
        Section::Appearance => appearance_section(ui),
        Section::Folders => {
            let net = app.net.as_mut().expect("signed in");
            folders_section(ui, net, &mut app.settings, can_write);
        }
        Section::Members => {
            let net = app.net.as_mut().expect("signed in");
            members_section(ui, net, &mut app.settings);
        }
        Section::Workspaces => {
            let email = app
                .net
                .as_ref()
                .and_then(|n| n.data("__me"))
                .and_then(|m| m.get("email"))
                .and_then(Value::as_str)
                .unwrap_or("you@airtribe.live")
                .to_owned();
            if let Some(target) = workspaces_section(ui, &mut app.settings, &email) {
                app.switch_workspace(&target, ui.ctx());
            }
        }
    });
}

fn nav_sections(is_admin: bool) -> Vec<(&'static str, &'static str, Section)> {
    let mut v = vec![
        (icon::USER_CIRCLE, "Account", Section::Account),
        (icon::CIRCLE_HALF, "Appearance", Section::Appearance),
        (icon::FOLDER_SIMPLE, "Folders", Section::Folders),
    ];
    if is_admin {
        v.push((icon::USERS, "Members", Section::Members));
    }
    v.push((icon::STACK, "Workspaces", Section::Workspaces));
    v
}

/// The window's own sidebar: icon+label rows, a rounded pill on the selected
/// one — `shell::nav_row`, the same component the app's own sidebar uses.
fn sidebar(ui: &mut egui::Ui, section: Section, is_admin: bool) -> Option<Section> {
    let mut picked = None;
    let panel = egui::Panel::left("settings-sidebar")
        .exact_size(SIDEBAR_W)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(colour::CHROME())
                .inner_margin(egui::Margin::symmetric(space::MD as i8, space::MD as i8)),
        )
        .show(ui, |ui| {
            ui.add_space(TRAFFIC_LIGHTS);
            for (glyph, label, s) in nav_sections(is_admin) {
                let item = shell::NavItem::new(glyph, label, section == s);
                if shell::nav_row(ui, &item).clicked() {
                    picked = Some(s);
                }
            }
        });
    let r = panel.response.rect;
    ui.painter().vline(r.right(), r.y_range(), egui::Stroke::new(1.0, colour::LINE_SOFT()));
    picked
}

fn section_title(s: Section) -> &'static str {
    match s {
        Section::Account => "Account",
        Section::Appearance => "Appearance",
        Section::Folders => "Folders",
        Section::Members => "Members",
        Section::Workspaces => "Workspaces",
    }
}

fn section_subtitle(s: Section) -> &'static str {
    match s {
        Section::Account => "Your identity, and where you're signed in.",
        Section::Appearance => "How Loop looks on this Mac.",
        Section::Folders => "Where your agent works when a task has no project.",
        Section::Members => "Who's on the team, and their access.",
        Section::Workspaces => "Every server you've signed in to.",
    }
}

/// The right pane: a title, a subtitle, then whatever the section draws.
/// Crossfades between sections rather than cutting, unless motion is off.
fn pane(ui: &mut egui::Ui, section: Section, body: impl FnOnce(&mut egui::Ui)) {
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(colour::CANVAS())
                .inner_margin(egui::Margin::symmetric(space::XXL as i8, space::XL as i8)),
        )
        .show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("settings:pane").show(ui, |ui| {
                let alpha = pane_fade(ui, section);
                ui.multiply_opacity(alpha);
                ui.label(
                    RichText::new(section_title(section))
                        .size(text::TITLE)
                        .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                        .color(colour::TEXT()),
                );
                ui.add_space(space::XXS);
                ui.label(RichText::new(section_subtitle(section)).size(text::SMALL).color(colour::TEXT_MUTED()));
                ui.add_space(space::LG);
                body(ui);
                ui.add_space(space::XL);
            });
        });
}

/// 0→1 since the pane last changed section — a crossfade rather than a cut,
/// and a fresh one on every switch (not just the first visit to a section).
fn pane_fade(ui: &egui::Ui, section: Section) -> f32 {
    if ui.style().animation_time <= f32::EPSILON {
        return 1.0;
    }
    let id = egui::Id::new("settings:pane:fade");
    let now = ui.input(|i| i.time);
    let changed_at = match ui.ctx().data(|d| d.get_temp::<(Section, f64)>(id)) {
        Some((s, at)) if s == section => at,
        _ => {
            ui.ctx().data_mut(|d| d.insert_temp(id, (section, now)));
            now
        }
    };
    let alpha = (((now - changed_at) as f32) / motion::BASE).clamp(0.0, 1.0);
    if alpha < 1.0 {
        ui.ctx().request_repaint();
    }
    alpha
}

// ------------------------------------------------------------------- groups

/// A caption over a rounded container of rows. For a group whose caption
/// needs a trailing action (an "Add" or "Invite" button), use `group_header`
/// and `group_body` directly — two statements rather than two closures
/// fighting over the same `&mut State`.
fn group(ui: &mut egui::Ui, caption: &str, body: impl FnOnce(&mut egui::Ui)) {
    if !caption.is_empty() {
        group_header(ui, caption, |_| {});
    }
    group_body(ui, body);
}

fn group_header(ui: &mut egui::Ui, caption: &str, trailing: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(caption)
                .size(text::SMALL)
                .family(egui::FontFamily::Name(theme::MEDIUM.into()))
                .color(colour::TEXT_MUTED()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), trailing);
    });
    ui.add_space(space::XS);
}

fn group_body(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(colour::SURFACE())
        .stroke(egui::Stroke::new(1.0, colour::LINE()))
        .corner_radius(radius::LG)
        .inner_margin(egui::Margin::symmetric(space::LG as i8, space::XS as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            body(ui);
        });
    ui.add_space(space::XL);
}

/// One row inside a group: a label over a muted description on the left, a
/// control right-aligned and vertically centred. `top_line` draws the hairline
/// above it — every row but a group's first.
fn row(ui: &mut egui::Ui, label: &str, description: &str, top_line: bool, control: impl FnOnce(&mut egui::Ui)) {
    // Laid out by hand rather than with nested egui layouts: those centre an
    // item against the row's height when it is placed, not the height the
    // row ends up with, which left controls hanging a line below their label.
    // Here the text is measured first, the row takes its final height, and
    // both halves are centred in it. The control's width comes from last
    // frame, so the text knows where to wrap.
    let width = ui.available_width();
    let id = ui.next_auto_id().with("settings:row-control-w");
    let control_w = ui.ctx().data(|d| d.get_temp::<f32>(id)).unwrap_or(width * 0.35);
    let text_w = (width - control_w - space::LG).max(width * 0.35);

    let wrap = |text: &str, font: egui::FontId, ink: egui::Color32| {
        ui.painter().layout(text.to_owned(), font, ink, text_w)
    };
    let title = wrap(label, egui::FontId::new(text::BODY, egui::FontFamily::Name(theme::MEDIUM.into())), colour::TEXT());
    let note = (!description.is_empty())
        .then(|| wrap(description, egui::FontId::proportional(text::SMALL), colour::TEXT_MUTED()));
    let gap = 2.0;
    let text_h = title.size().y + note.as_ref().map_or(0.0, |n| gap + n.size().y);
    let height = (text_h + space::MD * 2.0).max(ROW_MIN_H);

    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    // The hairline sits in the gap above the row rather than taking a strip of
    // its own, so a group of rows stays as dense as a native settings list.
    if top_line {
        let y = rect.top() - ui.spacing().item_spacing.y / 2.0;
        ui.painter().hline(rect.x_range(), y, egui::Stroke::new(1.0, colour::LINE_SOFT()));
    }

    let mut right = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    control(&mut right);
    let used = right.min_rect().width();
    ui.ctx().data_mut(|d| d.insert_temp(id, used));

    let top = rect.center().y - text_h / 2.0;
    let mut left = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_size(egui::pos2(rect.left(), top), egui::vec2(text_w, text_h)))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    left.spacing_mut().item_spacing.y = gap;
    left.add(egui::Label::new(title).selectable(false));
    if let Some(note) = note {
        left.add(egui::Label::new(note).selectable(false));
    }
}

fn row_divider(ui: &mut egui::Ui) {
    ui.add_space(space::XS);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, egui::Stroke::new(1.0, colour::LINE_SOFT()));
    ui.add_space(space::XS);
}

// ----------------------------------------------------------------- account

fn account_section(
    ui: &mut egui::Ui,
    name: &str,
    email: &str,
    role: &str,
    department: &str,
    scopes: &[String],
    base_url: &str,
) -> bool {
    c::surface(ui, false, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::LG;
            avatar::small(ui, email, ACCOUNT_AVATAR);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = space::XXS;
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(name)
                            .size(text::TITLE)
                            .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                            .color(colour::TEXT()),
                    );
                    if !role.is_empty() {
                        c::chip(ui, &sentence(role), c::Tone::Neutral, false);
                    }
                });
                ui.label(RichText::new(email).size(text::BODY).color(colour::TEXT_MUTED()));
            });
        });
    });
    ui.add_space(space::XL);

    group(ui, "Details", |ui| {
        row(ui, "Department", "", false, |ui| {
            if department.is_empty() {
                w::muted(ui, "None");
            } else {
                c::chip(ui, &sentence(department), c::discipline_tone(department), true);
            }
        });
        row(ui, "Server", "", true, |ui| {
            ui.label(RichText::new(base_url).monospace().size(text::SMALL).color(colour::TEXT_2()));
        });
        row(ui, "Access", "", true, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::XS;
                if scopes.is_empty() {
                    w::muted(ui, "None");
                }
                for scope in scopes {
                    c::chip(ui, &sentence(scope), c::Tone::Quiet, false);
                }
            });
        });
    });
    ui.label(
        RichText::new("Your name, role and department are managed by an admin.")
            .size(text::SMALL)
            .color(colour::TEXT_FAINT()),
    );
    ui.add_space(space::XL);

    let mut sign_out = false;
    // Not in a private workspace: it lives on this Mac, and its account has no
    // password to sign back in with.
    if !creds::active().is_some_and(|w| w.private) {
        group(ui, "", |ui| {
            row(ui, "Sign out of this workspace", "Other workspaces stay signed in.", false, |ui| {
                if w::danger(ui, "Sign out", true).clicked() {
                    sign_out = true;
                }
            });
        });
    }
    sign_out
}

fn sentence(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map(|f| f.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

// --------------------------------------------------------------- appearance

fn appearance_section(ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let mut a = theme::appearance(&ctx);
    let before = a;

    group(ui, "Theme", |ui| {
        ui.add_space(space::XS);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(space::MD, space::MD);
            for mode in theme::Mode::ALL {
                if theme_tile(ui, mode, a.mode == mode, a.accent).clicked() {
                    a.mode = mode;
                }
            }
        });
        ui.add_space(space::SM);
        row(ui, "Accent", colour::ACCENTS[a.accent].name, true, |ui| {
            if let Some(i) = accent_swatches(ui, a.accent) {
                a.accent = i;
            }
        });
    });
    ui.label(
        RichText::new("Saved on this Mac. System follows macOS as it switches between light and dark.")
            .size(text::SMALL)
            .color(colour::TEXT_FAINT()),
    );

    if a != before {
        theme::set_appearance(&ctx, a);
    }
}

const TILE: egui::Vec2 = egui::vec2(156.0, 98.0);

/// A theme choice drawn as a small window in that palette — sidebar, a card,
/// the accent button — so the choice is seen rather than read. System is the
/// two halves side by side.
fn theme_tile(ui: &mut egui::Ui, mode: theme::Mode, on: bool, accent: usize) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(TILE + egui::vec2(0.0, space::XL), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, on, format!("{} theme", mode.label()))
    });
    let response = motion::operable(ui, response, radius::LG as f32);
    let hot = response.hovered() || response.has_focus();
    let tile = egui::Rect::from_min_size(rect.min, TILE);
    let a = &colour::ACCENTS[accent];
    match mode {
        theme::Mode::Dark => paint_window(ui.painter(), tile, &colour::DUSK, a.dark[0]),
        theme::Mode::Light => paint_window(ui.painter(), tile, &colour::DAYLIGHT, a.light[0]),
        theme::Mode::System => {
            let (left, right) = tile.split_left_right_at_fraction(0.5);
            paint_window(&ui.painter().with_clip_rect(left), tile, &colour::DUSK, a.dark[0]);
            paint_window(&ui.painter().with_clip_rect(right), tile, &colour::DAYLIGHT, a.light[0]);
        }
    }
    let p = ui.painter();
    let (ring, width) = match (on, hot) {
        (true, _) => (colour::ACCENT(), 2.0),
        (false, true) => (colour::LINE_STRONG(), 1.0),
        _ => (colour::LINE(), 1.0),
    };
    p.rect_stroke(tile, radius::LG as f32, egui::Stroke::new(width, ring), egui::StrokeKind::Outside);
    p.text(
        egui::pos2(tile.left() + space::XXS, tile.bottom() + space::SM),
        egui::Align2::LEFT_TOP,
        mode.label(),
        egui::FontId::new(
            text::BODY,
            egui::FontFamily::Name(if on { theme::SEMIBOLD } else { theme::MEDIUM }.into()),
        ),
        if on { colour::TEXT() } else { colour::TEXT_MUTED() },
    );
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

fn paint_window(p: &egui::Painter, r: egui::Rect, pal: &colour::Palette, accent: egui::Color32) {
    let rr = radius::LG as f32;
    p.rect_filled(r, rr, pal.CANVAS);
    let side = egui::Rect::from_min_max(r.min, egui::pos2(r.left() + r.width() * 0.3, r.bottom()));
    p.rect_filled(side, egui::CornerRadius { nw: radius::LG, sw: radius::LG, ne: 0, se: 0 }, pal.CHROME);
    for (i, w) in [0.55, 0.7, 0.45].iter().enumerate() {
        let y = side.top() + 16.0 + i as f32 * 12.0;
        let bar = egui::Rect::from_min_size(egui::pos2(side.left() + 8.0, y), egui::vec2(side.width() * w, 4.0));
        if i == 0 {
            p.rect_filled(bar.expand2(egui::vec2(4.0, 3.0)), 3.0, pal.SURFACE_ACTIVE);
        }
        p.rect_filled(bar, 2.0, if i == 0 { pal.TEXT } else { pal.TEXT_FAINT });
    }
    let card = egui::Rect::from_min_max(
        egui::pos2(side.right() + 10.0, r.top() + 14.0),
        egui::pos2(r.right() - 10.0, r.bottom() - 14.0),
    );
    p.rect_filled(card, 4.0, pal.SURFACE);
    p.rect_stroke(card, 4.0, egui::Stroke::new(1.0, pal.LINE), egui::StrokeKind::Inside);
    let line = |y: f32, w: f32, c: egui::Color32| {
        p.rect_filled(egui::Rect::from_min_size(egui::pos2(card.left() + 8.0, y), egui::vec2(card.width() * w, 4.0)), 2.0, c);
    };
    line(card.top() + 10.0, 0.6, pal.TEXT);
    line(card.top() + 20.0, 0.8, pal.TEXT_MUTED);
    line(card.top() + 28.0, 0.5, pal.TEXT_MUTED);
    let button = egui::Rect::from_min_size(
        egui::pos2(card.left() + 8.0, card.bottom() - 18.0),
        egui::vec2(card.width() * 0.4, 10.0),
    );
    p.rect_filled(button, 3.0, accent);
}

fn accent_swatches(ui: &mut egui::Ui, selected: usize) -> Option<usize> {
    let light = colour::is_light();
    let items: Vec<(egui::Color32, &str)> = colour::ACCENTS
        .iter()
        .map(|a| (if light { a.light[0] } else { a.dark[0] }, a.name))
        .collect();
    viz::swatches(ui, &items, selected)
}

/// The system folder panel, opened at `start` when that is a folder. `None`
/// when it is cancelled. Blocks the frame while the panel is up, which is
/// what a modal panel means; macOS runs its own loop meanwhile.
pub(super) fn choose_folder(title: &str, start: &str) -> Option<String> {
    let mut panel = rfd::FileDialog::new().set_title(title);
    let start = std::path::Path::new(start.trim());
    if start.is_dir() {
        panel = panel.set_directory(start);
    }
    panel.pick_folder().map(|p| p.display().to_string())
}

/// The button beside a path field that opens the panel.
pub(super) fn choose_button(ui: &mut egui::Ui, enabled: bool) -> bool {
    w::icon_button(ui, icon::FOLDER_OPEN, "Choose\u{2026}", w::Emphasis::Secondary, enabled)
        .on_hover_text("Pick the folder in Finder")
        .clicked()
}

/// Room for the Choose… button beside a path field.
pub(super) const CHOOSE_W: f32 = 100.0;

// ------------------------------------------------------------------ folders

fn folders_section(ui: &mut egui::Ui, net: &mut Net, s: &mut State, can_write: bool) {
    let rows = folders(net);
    let error = net.error(FOLDERS_KEY).map(str::to_owned);
    let loading = net.is_loading(FOLDERS_KEY) && rows.is_empty() && error.is_none();

    if let Some(err) = &error {
        w::error(ui, err);
        return;
    }

    if s.adding {
        add_form(ui, net, s);
        ui.add_space(space::LG);
    }
    if let Some(err) = &s.error {
        w::error(ui, err);
        ui.add_space(space::SM);
    }

    if loading {
        w::loading(ui, "Loading folders");
        return;
    }

    group_header(ui, "Folders", |ui| {
        if can_write && !s.adding && w::ghost(ui, "+ Add folder").clicked() {
            s.adding = true;
            s.name.clear();
            s.path.clear();
            s.error = None;
        }
    });
    group_body(ui, |ui| {
        if rows.is_empty() {
            if !s.adding {
                w::empty(
                    ui,
                    "No folders yet.",
                    "Add one so your agent has somewhere to work when a task has no project.",
                );
            }
        } else {
            for (i, row) in rows.iter().enumerate() {
                if i > 0 {
                    row_divider(ui);
                }
                folder_row(ui, net, row, s, can_write);
            }
        }
    });

    if let Some(id) = s.removing.clone() {
        let name = rows
            .iter()
            .find(|f| f.get("id").and_then(Value::as_str) == Some(id.as_str()))
            .and_then(|f| f.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("this folder")
            .to_owned();
        let title = format!("Remove the \u{201c}{name}\u{201d} folder?");
        let body = "Your agent won\u{2019}t offer it for a task with no project. A task already pinned \
                    to it falls back to your default.";
        match confirm_remove(ui.ctx(), egui::Id::new("settings:folder:remove"), &title, body) {
            Some(true) => {
                s.removing = None;
                s.busy = true;
                net.invalidate(ACTION_KEY);
                net.send(
                    ACTION_KEY,
                    reqwest::Method::DELETE,
                    &format!("/api/user/folders/{id}"),
                    Value::Null,
                );
            }
            Some(false) => s.removing = None,
            None => {}
        }
    }
}

fn add_form(ui: &mut egui::Ui, net: &mut Net, s: &mut State) {
    c::surface(ui, false, |ui| {
        ui.set_width(ui.available_width());
        w::field(ui, "Name", &mut s.name, false, "e.g. mycohort-api\u{2026}");
        ui.add_space(space::MD);
        ui.horizontal(|ui| {
            let room = ui.available_width() - CHOOSE_W - space::SM;
            ui.allocate_ui_with_layout(
                egui::vec2(room, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| w::field(ui, "Path", &mut s.path, false, "/Users/you/code/mycohort-api"),
            );
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                if choose_button(ui, true) {
                    if let Some(path) = choose_folder("Choose a folder for your agent", &s.path) {
                        // The folder's own name is almost always what it
                        // should be called here.
                        if s.name.trim().is_empty() {
                            s.name = std::path::Path::new(&path)
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                        }
                        s.path = path;
                    }
                }
            });
        });
        let path_typed = !s.path.trim().is_empty();
        let path_ok = s.path.trim().starts_with('/');
        if path_typed && !path_ok {
            ui.add_space(space::XS);
            w::error(
                ui,
                "Needs a full path starting with /, like /Users/you/code/mycohort-api.",
            );
        }
        ui.add_space(space::MD);
        let ready = !s.name.trim().is_empty() && path_ok && !s.busy;
        ui.horizontal(|ui| {
            if w::primary(ui, "Add", ready).clicked() {
                s.busy = true;
                s.error = None;
                net.invalidate(ACTION_KEY);
                net.post(
                    ACTION_KEY,
                    "/api/user/folders",
                    json!({ "name": s.name.trim(), "path": s.path.trim() }),
                );
            }
            ui.add_space(space::XS);
            if w::ghost(ui, "Cancel").clicked() {
                s.adding = false;
            }
        });
    });
}

fn folder_row(ui: &mut egui::Ui, net: &mut Net, row: &Value, s: &mut State, can_write: bool) {
    let id = row
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let name = row.get("name").and_then(Value::as_str).unwrap_or_default();
    let path = row.get("path").and_then(Value::as_str).unwrap_or_default();
    let is_default = row
        .get("isDefault")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    ui.horizontal(|ui| {
        ui.set_min_height(ROW_MIN_H);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if can_write {
                viz::more(ui, |ui| {
                    if !is_default && viz::menu_item(ui, "Set as default", false, None) {
                        s.busy = true;
                        net.invalidate(ACTION_KEY);
                        net.post(
                            ACTION_KEY,
                            &format!("/api/user/folders/{id}/default"),
                            Value::Null,
                        );
                    }
                    if viz::menu_item(ui, "Remove\u{2026}", true, None) {
                        s.removing = Some(id.clone());
                    }
                });
            }
            if is_default {
                ui.add_space(space::XS);
                c::chip(ui, "Default", c::Tone::Ok, false);
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.set_width(ui.available_width());
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(name)
                            .size(text::BODY)
                            .family(egui::FontFamily::Name(theme::MEDIUM.into()))
                            .color(colour::TEXT()),
                    );
                    ui.add_space(2.0);
                    ui.label(RichText::new(path).size(text::SMALL).color(colour::TEXT_MUTED()));
                });
            });
        });
    });
}

/// Fold in whatever the add, remove or set-default request returned.
fn settle(net: &mut Net, s: &mut State) {
    if s.busy && !net.is_loading(ACTION_KEY) {
        s.busy = false;
        match net.peek(ACTION_KEY).cloned() {
            Some(Ok(_)) => {
                net.invalidate(ACTION_KEY);
                net.invalidate(FOLDERS_KEY);
                s.adding = false;
            }
            Some(Err(e)) => {
                net.invalidate(ACTION_KEY);
                s.error = Some(e);
            }
            None => {}
        }
    }
}

/// "Remove the "mycohort-api" folder?" — Some(true) to remove, Some(false) to
/// keep, None while it is still asking. The same shape as `viz::confirm_remove`,
/// which is a label's wording and private to `tag_picker`; this is a folder's.
fn confirm_remove(ctx: &egui::Context, id: egui::Id, title: &str, body: &str) -> Option<bool> {
    let mut answer = None;
    let modal = egui::Modal::new(id.with("modal"))
        .backdrop_color(colour::CANVAS().gamma_multiply(0.7))
        .frame(
            egui::Frame::new()
                .fill(colour::SURFACE())
                .stroke(egui::Stroke::new(1.0, colour::LINE_STRONG()))
                .corner_radius(radius::LG)
                .inner_margin(egui::Margin::same(space::XL as i8)),
        )
        .show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(
                RichText::new(title)
                    .size(text::CARD)
                    .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                    .color(colour::TEXT()),
            );
            ui.add_space(space::SM);
            ui.label(
                RichText::new(body)
                .size(text::BODY)
                .color(colour::TEXT_2()),
            );
            ui.add_space(space::LG);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if w::danger(ui, "Remove", true).clicked() {
                    answer = Some(true);
                }
                if w::ghost(ui, "Cancel").clicked() {
                    answer = Some(false);
                }
            });
        });
    if modal.should_close() && answer.is_none() {
        answer = Some(false);
    }
    answer
}

// ------------------------------------------------------------------ members

fn members(net: &Net) -> Vec<Value> {
    net.data(MEMBERS_KEY).and_then(Value::as_array).cloned().unwrap_or_default()
}

fn members_section(ui: &mut egui::Ui, net: &mut Net, s: &mut State) {
    let rows = members(net);
    let error = net.error(MEMBERS_KEY).map(str::to_owned);
    let loading = net.is_loading(MEMBERS_KEY) && rows.is_empty() && error.is_none();
    let me_id = net.data("__me").map(|m| str_at(m, "personId").to_owned());

    if let Some(result) = s.members.result.clone() {
        if member_result_card(ui, net, &result) {
            s.members.result = None;
        }
        ui.add_space(space::LG);
    }

    if let Some(err) = &error {
        w::error(ui, err);
        return;
    }
    if let Some(err) = &s.members.error {
        w::error(ui, err);
        ui.add_space(space::SM);
    }

    if s.members.adding {
        member_add_form(ui, net, s);
        ui.add_space(space::LG);
    }

    if loading {
        w::loading(ui, "Loading members");
        return;
    }

    group_header(ui, "Members", |ui| {
        if !s.members.adding && w::primary(ui, "Invite someone", true).clicked() {
            start_member_add(s);
        }
    });
    group_body(ui, |ui| {
        if rows.is_empty() {
            if !s.members.adding {
                w::empty(ui, "No members yet.", "Invite someone to get the team onto Loop.");
            }
        } else {
            for (i, row) in rows.iter().enumerate() {
                if i > 0 {
                    row_divider(ui);
                }
                member_row(ui, net, row, s, me_id.as_deref());
            }
        }
    });

    if let Some(id) = s.members.revoking.clone() {
        let name = rows
            .iter()
            .find(|p| p.get("id").and_then(Value::as_str) == Some(id.as_str()))
            .and_then(|p| p.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("this person")
            .to_owned();
        match confirm_revoke(ui.ctx(), egui::Id::new("settings:member:revoke"), &name) {
            Some(true) => {
                if let Some(row) = rows.iter().find(|p| p.get("id").and_then(Value::as_str) == Some(id.as_str())) {
                    let email = row.get("email").and_then(Value::as_str).unwrap_or_default().to_owned();
                    s.members.revoking = None;
                    s.members.busy = true;
                    s.members.action = Some(MemberAction::Revoke);
                    net.invalidate(MEMBERS_ACTION_KEY);
                    net.post(MEMBERS_ACTION_KEY, "/api/admin/revoke", json!({ "email": email }));
                }
            }
            Some(false) => s.members.revoking = None,
            None => {}
        }
    }
}

fn start_member_add(s: &mut State) {
    s.members.adding = true;
    s.members.name.clear();
    s.members.email.clear();
    s.members.department = None;
    s.members.role = Some("member".to_owned());
    s.members.error = None;
}

fn member_add_form(ui: &mut egui::Ui, net: &mut Net, s: &mut State) {
    c::surface(ui, false, |ui| {
        ui.set_width(ui.available_width());
        w::field(ui, "Name", &mut s.members.name, false, "e.g. Priya Sharma\u{2026}");
        ui.add_space(space::MD);
        w::field(ui, "Email", &mut s.members.email, false, "name@airtribe.live");
        let email_typed = !s.members.email.trim().is_empty();
        let email_ok = s.members.email.trim().to_lowercase().ends_with("@airtribe.live");
        if email_typed && !email_ok {
            ui.add_space(space::XS);
            w::error(ui, "Needs an @airtribe.live address.");
        }
        ui.add_space(space::MD);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                w::caption(ui, "Department");
                ui.add_space(space::XXS);
                let options: Vec<(String, String)> =
                    MEMBER_DEPARTMENTS.iter().map(|d| (d.to_string(), sentence(d))).collect();
                viz::select(ui, "Department", &options, &mut s.members.department);
            });
            ui.add_space(space::MD);
            ui.vertical(|ui| {
                w::caption(ui, "Role");
                ui.add_space(space::XXS);
                let options: Vec<(String, String)> =
                    MEMBER_ROLES.iter().map(|r| (r.to_string(), sentence(r))).collect();
                viz::select(ui, "Role", &options, &mut s.members.role);
            });
        });
        ui.add_space(space::MD);
        let ready = !s.members.name.trim().is_empty()
            && email_ok
            && s.members.department.is_some()
            && s.members.role.is_some()
            && !s.members.busy;
        ui.horizontal(|ui| {
            if w::primary(ui, "Add and create setup code", ready).clicked() {
                s.members.busy = true;
                s.members.error = None;
                s.members.action = Some(MemberAction::Add);
                net.invalidate(MEMBERS_ACTION_KEY);
                net.post(
                    MEMBERS_ACTION_KEY,
                    "/api/admin/people",
                    json!({
                        "email": s.members.email.trim(),
                        "name": s.members.name.trim(),
                        "department": s.members.department.clone().unwrap_or_default(),
                        "role": s.members.role.clone().unwrap_or_else(|| "member".to_owned()),
                    }),
                );
            }
            ui.add_space(space::XS);
            if w::ghost(ui, "Cancel").clicked() {
                s.members.adding = false;
            }
        });
    });
}

fn member_row(ui: &mut egui::Ui, net: &mut Net, row: &Value, s: &mut State, me_id: Option<&str>) {
    let id = row.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
    let name = row.get("name").and_then(Value::as_str).unwrap_or_default();
    let email = row.get("email").and_then(Value::as_str).unwrap_or_default();
    let role = row.get("role").and_then(Value::as_str).unwrap_or_default();
    let department = row.get("department").and_then(Value::as_str).unwrap_or_default();
    let invite_expires_at = row.get("inviteExpiresAt").and_then(Value::as_str);
    let last_seen_at = row.get("lastSeenAt").and_then(Value::as_str);
    // A live session counts as joined too: someone signed in by an admin's
    // `acp-admin session` has no password yet, but they are plainly in.
    let joined = row.get("joined").and_then(Value::as_bool).unwrap_or(false) || last_seen_at.is_some();
    let is_me = me_id == Some(id.as_str());

    ui.horizontal(|ui| {
        ui.set_min_height(ROW_MIN_H);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            viz::more(ui, |ui| {
                if viz::menu_item(ui, "New setup code", false, None) {
                    s.members.busy = true;
                    s.members.action =
                        Some(MemberAction::Invite { name: name.to_owned(), email: email.to_owned() });
                    net.invalidate(MEMBERS_ACTION_KEY);
                    net.post(MEMBERS_ACTION_KEY, "/api/admin/invite", json!({ "email": email }));
                }
                let why = is_me.then_some("You can\u{2019}t remove your own access");
                if viz::menu_item(ui, "Remove access\u{2026}", true, why) {
                    s.members.revoking = Some(id.clone());
                }
            });

            ui.add_space(space::SM);
            let mut dept_slot: Option<String> = None;
            let dept_options: Vec<(String, String)> = MEMBER_DEPARTMENTS
                .iter()
                .filter(|d| **d != department)
                .map(|d| (d.to_string(), sentence(d)))
                .collect();
            viz::value_select(ui, &sentence(department), &dept_options, &mut dept_slot);
            if let Some(next) = dept_slot {
                s.members.busy = true;
                s.members.action = Some(MemberAction::Patch);
                net.invalidate(MEMBERS_ACTION_KEY);
                net.patch(MEMBERS_ACTION_KEY, &format!("/api/admin/people/{id}"), json!({ "department": next }));
            }

            ui.add_space(space::XS);
            let mut role_slot: Option<String> = None;
            let role_options: Vec<(String, String)> = MEMBER_ROLES
                .iter()
                .filter(|r| **r != role)
                .map(|r| (r.to_string(), sentence(r)))
                .collect();
            viz::value_select(ui, &sentence(role), &role_options, &mut role_slot);
            if let Some(next) = role_slot {
                s.members.busy = true;
                s.members.action = Some(MemberAction::Patch);
                net.invalidate(MEMBERS_ACTION_KEY);
                net.patch(MEMBERS_ACTION_KEY, &format!("/api/admin/people/{id}"), json!({ "role": next }));
            }

            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.x = space::SM;
                avatar::small(ui, email, size::AVATAR_MD);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = space::XXS;
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(name)
                                .size(text::BODY)
                                .family(egui::FontFamily::Name(theme::MEDIUM.into()))
                                .color(colour::TEXT()),
                        );
                        member_status_chip(ui, joined, invite_expires_at);
                    });
                    let sub = last_seen_at
                        .map(|t| format!("{email} \u{00b7} last seen {}", relative(t)))
                        .unwrap_or_else(|| email.to_owned());
                    ui.label(RichText::new(sub).size(text::SMALL).color(colour::TEXT_MUTED()));
                });
            });
        });
    });
}

fn member_status_chip(ui: &mut egui::Ui, joined: bool, invite_expires_at: Option<&str>) {
    if joined {
        c::chip(ui, "Joined", c::Tone::Ok, false);
        return;
    }
    match invite_expires_at {
        Some(t) => {
            let h = hours_left(t);
            if h > 0 {
                c::chip(ui, &format!("Invite pending \u{00b7} expires in {h}h"), c::Tone::Running, false);
            } else {
                c::chip(ui, "Invite expired", c::Tone::Quiet, false);
            }
        }
        None => {
            c::chip(ui, "Not invited", c::Tone::Quiet, false);
        }
    }
}

/// The setup-code result card. True once "Done" is clicked, for the caller to
/// dismiss it — this draws the card and nothing else, so it cannot also hold
/// the state it would need to dismiss itself.
fn member_result_card(ui: &mut egui::Ui, net: &Net, r: &InviteResult) -> bool {
    let mut done = false;
    w::card(ui, |ui| {
        ui.set_width(ui.available_width());
        w::caption(ui, "Setup code");
        ui.add_space(space::XS);
        ui.label(
            RichText::new(&r.code)
                .monospace()
                .size(text::TITLE)
                .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                .color(colour::TEXT()),
        );
        ui.add_space(space::SM);
        ui.label(
            RichText::new(format!("Valid 48 hours, single use. For {}.", r.email))
                .size(text::SMALL)
                .color(colour::TEXT_MUTED()),
        );
        ui.add_space(space::MD);
        ui.horizontal(|ui| {
            if w::secondary(ui, "Copy code", true).clicked() {
                ui.ctx().copy_text(r.code.clone());
                w::toast(ui.ctx(), "Copied.", false);
            }
            ui.add_space(space::XS);
            if w::secondary(ui, "Copy invite message", true).clicked() {
                ui.ctx().copy_text(invite_message(r, &net.base_url));
                w::toast(ui.ctx(), "Copied.", false);
            }
            ui.add_space(space::XS);
            if w::ghost(ui, "Done").clicked() {
                done = true;
            }
        });
    });
    done
}

fn invite_message(r: &InviteResult, base_url: &str) -> String {
    format!(
        "Hey {name}, you\u{2019}re set up on Loop \u{2014} here\u{2019}s how to get in:\n\n\
         1. Install Loop \u{2014} paste this into Terminal:\n   \
            {install}\n   \
            (Run the same command any time to update.)\n\
         2. Loop opens; choose \u{201c}First time here?\u{201d}\n\
         3. Enter:\n   \
            Email: {email}\n   \
            Code: {code}\n   \
            Password: pick one, 8+ characters\n   \
            Server: {base_url}\n\n\
         The code is valid for 48 hours and works once, so set it up soon.",
        name = r.name,
        email = r.email,
        code = r.code,
        install = install_command(base_url),
    )
}

/// Installs Loop, and updates it: the latest build this server hands out
/// (`src/routes/downloads.rs`, uploaded by `scripts/release-mac.sh`).
fn install_command(base_url: &str) -> String {
    format!("curl -fsSL {base_url}/install.sh | bash")
}

/// "Remove Jane's access?" — the same shape as `confirm_remove`, worded for a
/// person rather than a folder: what leaves is their sessions and their place
/// on the team, not a row they can re-add in a click.
fn confirm_revoke(ctx: &egui::Context, id: egui::Id, name: &str) -> Option<bool> {
    let mut answer = None;
    let modal = egui::Modal::new(id.with("modal"))
        .backdrop_color(colour::CANVAS().gamma_multiply(0.7))
        .frame(
            egui::Frame::new()
                .fill(colour::SURFACE())
                .stroke(egui::Stroke::new(1.0, colour::LINE_STRONG()))
                .corner_radius(radius::LG)
                .inner_margin(egui::Margin::same(space::XL as i8)),
        )
        .show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(
                RichText::new(format!("Remove {name}\u{2019}s access?"))
                    .size(text::CARD)
                    .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                    .color(colour::TEXT()),
            );
            ui.add_space(space::SM);
            ui.label(
                RichText::new(
                    "Every session they hold ends immediately. Their history on the board stays; \
                     they can be invited back later under the same address.",
                )
                .size(text::BODY)
                .color(colour::TEXT_2()),
            );
            ui.add_space(space::LG);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if w::danger(ui, "Remove access", true).clicked() {
                    answer = Some(true);
                }
                if w::ghost(ui, "Cancel").clicked() {
                    answer = Some(false);
                }
            });
        });
    if modal.should_close() && answer.is_none() {
        answer = Some(false);
    }
    answer
}

/// Fold in whatever the add, invite, patch or revoke request returned.
fn settle_members(ctx: &egui::Context, net: &mut Net, s: &mut State) {
    if !s.members.busy || net.is_loading(MEMBERS_ACTION_KEY) {
        return;
    }
    s.members.busy = false;
    let action = s.members.action.take();
    match net.peek(MEMBERS_ACTION_KEY).cloned() {
        Some(Ok(data)) => {
            net.invalidate(MEMBERS_ACTION_KEY);
            net.invalidate(MEMBERS_KEY);
            net.invalidate(PEOPLE_KEY);
            match action {
                Some(MemberAction::Add) => {
                    let person = data.get("person");
                    let name = person.and_then(|p| p.get("name")).and_then(Value::as_str).unwrap_or_default();
                    let email = person.and_then(|p| p.get("email")).and_then(Value::as_str).unwrap_or_default();
                    let code = data.get("code").and_then(Value::as_str).unwrap_or_default();
                    s.members.result = Some(InviteResult {
                        name: name.to_owned(),
                        email: email.to_owned(),
                        code: code.to_owned(),
                    });
                    s.members.adding = false;
                }
                Some(MemberAction::Invite { name, email }) => {
                    let code = data.get("code").and_then(Value::as_str).unwrap_or_default().to_owned();
                    s.members.result = Some(InviteResult { name, email, code });
                }
                Some(MemberAction::Revoke) => {
                    w::toast(ctx, "Access removed.", false);
                }
                Some(MemberAction::Patch) | None => {}
            }
        }
        Some(Err(e)) => {
            net.invalidate(MEMBERS_ACTION_KEY);
            match action {
                Some(MemberAction::Add) => s.members.error = Some(e),
                _ => w::toast(ctx, e, true),
            }
        }
        None => {}
    }
}

/// Parse a server timestamp. Every date this view reads comes back as
/// RFC3339, the same as `board`'s.
fn parse_ts(ts: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(ts).ok().map(|t| t.with_timezone(&Utc))
}

/// Hours left on a setup code, rounded up so "expires in 1h" still means
/// there is time, not none. 0 once it has actually expired.
fn hours_left(expires_at: &str) -> i64 {
    let Some(then) = parse_ts(expires_at) else { return 0 };
    let mins = (then - Utc::now()).num_minutes();
    if mins <= 0 { 0 } else { (mins + 59) / 60 }
}

/// How long ago, for a last-seen caption.
fn relative(ts: &str) -> String {
    let Some(then) = parse_ts(ts) else { return String::new() };
    match (Utc::now() - then).num_seconds().max(0) {
        s if s < 60 => "just now".to_owned(),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86_400 => format!("{}h ago", s / 3600),
        s => format!("{}d ago", s / 86_400),
    }
}

// --------------------------------------------------------------- workspaces

fn same_server(a: &str, b: &str) -> bool {
    a.trim().trim_end_matches('/') == b.trim().trim_end_matches('/')
}

fn workspaces_section(ui: &mut egui::Ui, s: &mut State, email: &str) -> Option<creds::Workspace> {
    let ctx = ui.ctx().clone();
    let list = creds::workspaces();
    let active_server = creds::base_url();
    let mut switch_to = None;

    if list.is_empty() {
        w::empty(ui, "No workspaces yet.", "Sign in to a server to see it here.");
    } else {
        group(ui, "Workspaces", |ui| {
            for (i, w) in list.iter().enumerate() {
                if i > 0 {
                    row_divider(ui);
                }
                workspace_row(ui, &ctx, w, &active_server, s, &mut switch_to);
            }
        });
    }

    private_workspace_group(ui, &list, email);
    switch_to
}

fn workspace_row(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    ws: &creds::Workspace,
    active_server: &str,
    s: &mut State,
    switch_to: &mut Option<creds::Workspace>,
) {
    let is_current = same_server(&ws.server, active_server);
    let key = ws.server.clone();
    let renaming = s.renaming.as_deref() == Some(key.as_str());

    ui.horizontal(|ui| {
        ui.set_min_height(ROW_MIN_H);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            viz::more(ui, |ui| {
                let why = is_current.then_some("Switch away from it first");
                if viz::menu_item(ui, "Remove\u{2026}", true, why) {
                    s.ws_removing = Some(key.clone());
                }
            });
            ui.add_space(space::XS);
            if is_current {
                c::chip(ui, "Current", c::Tone::Info, false);
            } else if w::secondary(ui, "Switch", true).clicked() {
                *switch_to = Some(ws.clone());
            }
            ui.add_space(space::SM);

            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.x = space::SM;
                let (mark, _) = ui.allocate_exact_size(egui::Vec2::splat(22.0), egui::Sense::hover());
                shell::paint_mark(ui, mark, ws.private);
                ui.vertical(|ui| {
                    if renaming {
                        let resp = ui.add(egui::TextEdit::singleline(&mut s.rename_text).desired_width(180.0));
                        if s.rename_focus {
                            resp.request_focus();
                            s.rename_focus = false;
                        }
                        if resp.lost_focus() {
                            commit_rename(ctx, s, &key);
                        }
                    } else {
                        let label = ui.add(
                            egui::Label::new(
                                RichText::new(&ws.name)
                                    .size(text::BODY)
                                    .family(egui::FontFamily::Name(theme::MEDIUM.into()))
                                    .color(colour::TEXT()),
                            )
                            .sense(egui::Sense::click()),
                        );
                        if label.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if label.clicked() {
                            s.renaming = Some(key.clone());
                            s.rename_text = ws.name.clone();
                            s.rename_focus = true;
                        }
                    }
                    let place = if ws.private {
                        "Only on this Mac".to_owned()
                    } else {
                        ws.server.split("://").last().unwrap_or(&ws.server).to_owned()
                    };
                    let place = if ws.token.is_none() { format!("{place} \u{00b7} signed out") } else { place };
                    ui.label(RichText::new(place).size(text::SMALL).color(colour::TEXT_MUTED()));
                });
            });
        });
    });

    if s.ws_removing.as_deref() == Some(key.as_str()) {
        let title = format!("Remove {} from your workspaces?", ws.name);
        let body = if ws.private {
            "It leaves the switcher only. Its server and everything in it stay on this Mac; \
             run scripts/private-workspace.sh again to bring it back."
        } else {
            "It leaves the switcher and signs this Mac out of it. Nothing on the server changes; \
             you can add it again with Add workspace."
        };
        match confirm_remove(ctx, egui::Id::new("settings:workspace:remove").with(&key), &title, body) {
            Some(true) => {
                let mut list = creds::workspaces();
                list.retain(|w| !same_server(&w.server, &key));
                let _ = creds::save_workspaces(&list);
                crate::desktop::views::chrome::forget_workspaces(ctx);
                s.ws_removing = None;
            }
            Some(false) => s.ws_removing = None,
            None => {}
        }
    }
}

fn commit_rename(ctx: &egui::Context, s: &mut State, server: &str) {
    let name = s.rename_text.trim();
    if !name.is_empty() {
        let mut list = creds::workspaces();
        if let Some(w) = list.iter_mut().find(|w| same_server(&w.server, server)) {
            if w.name != name {
                w.name = name.to_owned();
                let _ = creds::save_workspaces(&list);
                crate::desktop::views::chrome::forget_workspaces(ctx);
            }
        }
    }
    s.renaming = None;
}

/// The one workspace private to this Mac, if there is one — or, if not, how
/// to make one.
fn private_workspace_group(ui: &mut egui::Ui, list: &[creds::Workspace], email: &str) {
    let private = list.iter().find(|w| w.private);
    // The setup script is served by the team server (src/routes/downloads.rs),
    // so it is reachable on any teammate's Mac — no repository needed.
    let team = list
        .iter()
        .find(|w| !w.private)
        .map(|w| w.server.clone())
        .unwrap_or_else(creds::base_url);
    let command = format!("curl -fsSL {team}/private-workspace.sh | bash -s -- {email}");
    let prompt = format!(
        "Set up a private Loop workspace on this Mac: its own database and a server that only this \
         Mac can reach, so tasks and projects created there never go to the team server.\n\n\
         1. Run this in Terminal:\n   {command}\n\
         2. It needs Postgres and installs it with Homebrew if nothing is running. If it stops because \
         Homebrew is missing, install Homebrew from https://brew.sh and run step 1 again.\n\
         3. When it finishes, check that http://127.0.0.1:8181/health/ready answers \"ready\".\n\
         4. Tell me when it's done. Loop then shows \"Private\" in its workspace switcher (top left, \u{2318}2).\n\n\
         Don't change anything else on this Mac."
    );

    group(ui, "Private workspace", |ui| match private {
        Some(p) if p.token.is_some() => {
            row(ui, &p.name, "Only on this Mac \u{00b7} starts at login \u{00b7} nothing reaches the team server", false, |ui| {
                c::chip(ui, "Running here", c::Tone::Ok, true);
            });
        }
        state => {
            let (title, detail) = match state {
                Some(p) => (
                    format!("Reconnect {}", p.name),
                    "Its server and data are still on this Mac; running the setup again signs it back in \
                     here. It keeps everything.",
                ),
                None => (
                    "Set up a private workspace".to_owned(),
                    "A workspace that lives only on this Mac: its own server and data, so tasks and \
                     projects you create there never reach the team server.",
                ),
            };
            row(ui, &title, detail, false, |ui| {
                if w::primary(ui, "Copy setup prompt for Claude", true)
                    .on_hover_text("Paste into Claude Code; it runs the setup and handles anything missing")
                    .clicked()
                {
                    ui.ctx().copy_text(prompt.clone());
                    w::toast(ui.ctx(), "Copied \u{2014} paste it into Claude Code.", false);
                }
            });
            ui.add_space(space::XS);
            ui.horizontal(|ui| {
                ui.add(
                    egui::Label::new(RichText::new(&command).monospace().size(text::SMALL).color(colour::TEXT_2()))
                        .truncate(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if w::ghost(ui, "Copy command").clicked() {
                        ui.ctx().copy_text(command.clone());
                        w::toast(ui.ctx(), "Copied.", false);
                    }
                });
            });
            ui.add_space(space::SM);
        }
    });
}
