//! One agent's page: who it is and whether it is alive, what it has done,
//! and the record behind that — its activity, its tasks, the runs it
//! reported and its step log.
//!
//! Reads top to bottom from glance to detail. The header says who and what
//! it is doing now; the rail beside it holds the facts you check (status,
//! last seen, setup). Under the header a strip of figures and thirty days of
//! activity answer "is it pulling its weight"; the tabs under those are the
//! evidence. Everything here is the owner's (and admins'), enforced by the
//! overview endpoint, so nothing is hidden per item.
//!
//! Nothing moves unless the agent is working: the face's ring, and the
//! spinner on its now line, are the only motion, and both go still with
//! `AIRTRIBE_REDUCE_MOTION`.

use chrono::{DateTime, Local, NaiveDate};
use egui::{pos2, vec2, RichText};
use serde_json::Value;

use super::agent_session::{entry, prose, short_name, Mark, Node};
use super::agents::{
    has, role_chips, role_items, runtime_label, setup_checks, state_tone, state_words,
    status_words, Opened, Role,
};
use super::task::{ago, exact};
use crate::desktop::design::agent::{self as face, Presence};
use crate::desktop::design::table::{self, Col};
use crate::desktop::design::{
    cards as c, colour, pad, radius, shell, space, status_label, text, theme, viz, widgets as w,
};
use crate::desktop::net::Net;

/// What the page asks the Agents view to do.
pub(super) enum Ask {
    Back,
    Task(String),
    /// One of the card's actions, on this agent: its name, and which.
    Card(String, CardAsk),
}

pub(super) enum CardAsk {
    Continue,
    Role(Role, bool),
    Rotate,
    Revoke,
}

/// Below this the rail's facts fold into a line under the header. The
/// shell's own rail breakpoint.
const NARROW: f32 = 760.0;
const TABS: [&str; 4] = ["Activity", "Tasks", "Runs", "Logs"];
/// The chart's bars, and how tall the tallest day stands.
const CHART_H: f32 = 64.0;
const BAR_GAP: f32 = 3.0;
/// The run strip: one tick per run, oldest first.
const TICK_W: f32 = 4.0;
const TICK_H: f32 = 14.0;

fn key(id: &str) -> String {
    format!("agent:overview:{id}")
}

/// The page. `listed` is the agent as the owner's list has it — there when
/// the viewer owns it, which is also what offers the management menu — so
/// the header draws before the overview lands.
pub(super) fn show(
    ui: &mut egui::Ui,
    net: &mut Net,
    opened: &mut Opened,
    listed: Option<Value>,
    refresh: bool,
    error: Option<&str>,
    seed: &str,
) -> Option<Ask> {
    let key = key(&opened.id);
    let path = format!("/api/user/agents/{}/overview", opened.id);
    if refresh {
        net.get(&key, &path);
    } else {
        net.get_once(&key, &path);
    }
    let overview = net.shared(&key);
    let owned = listed.is_some();
    let agent = overview
        .as_deref()
        .and_then(|o| o.get("agent"))
        .cloned()
        .or(listed)
        .unwrap_or(Value::Null);
    let mut ask = None;

    let back_label = if opened.back.is_some() {
        "Back"
    } else {
        "All agents"
    };
    if shell::back(ui, back_label).clicked() {
        ask = Some(Ask::Back);
    }
    if let Some(err) = net.error(&key).filter(|_| overview.is_none()) {
        let words = if err.contains("403") || err.to_lowercase().contains("only the agent") {
            "This agent\u{2019}s page is its owner\u{2019}s to see.".to_owned()
        } else {
            format!("Could not load this agent. {err}")
        };
        w::error(ui, &words);
        return ask;
    }

    let name = str_of(&agent, "name").unwrap_or("Agent").to_owned();
    let short = short_name(&name).to_owned();
    let tab = &mut opened.tab;
    let mut from_rail = None;
    // Narrow, the rail's facts would stand a screen tall between the header
    // and the figures; they fold into one wrapped line instead.
    if ui.available_width() < NARROW {
        if let Some(a) = header(ui, &agent, owned, seed, &name, &short) {
            ask = Some(a);
        }
        ui.add_space(space::MD);
        from_rail = facts(ui, &agent, owned);
        if let Some(err) = error {
            ui.add_space(space::MD);
            w::error(ui, err);
        }
        match overview.as_deref() {
            None => skeleton(ui),
            Some(o) => {
                if let Some(a) = body(ui, o, &agent, tab, owned, seed, &short) {
                    ask = Some(a);
                }
            }
        }
        ui.add_space(space::XXL);
        return ask.or(from_rail.map(|a| Ask::Card(name, a)));
    }
    shell::with_rail(
        ui,
        |ui, part| match part {
            shell::Part::Header => {
                if let Some(a) = header(ui, &agent, owned, seed, &name, &short) {
                    ask = Some(a);
                }
            }
            shell::Part::Body => {
                if let Some(err) = error {
                    ui.add_space(space::MD);
                    w::error(ui, err);
                }
                match overview.as_deref() {
                    None => skeleton(ui),
                    Some(o) => {
                        if let Some(a) = body(ui, o, &agent, tab, owned, seed, &short) {
                            ask = Some(a);
                        }
                    }
                }
            }
        },
        |ui| {
            from_rail = rail(ui, &agent, owned);
        },
    );
    ui.add_space(space::XXL);
    ask.or(from_rail.map(|a| Ask::Card(name, a)))
}

// ------------------------------------------------------------------ header

fn header(
    ui: &mut egui::Ui,
    a: &Value,
    owned: bool,
    seed: &str,
    name: &str,
    short: &str,
) -> Option<Ask> {
    let status = str_of(a, "status").unwrap_or("waiting");
    let current = a.get("currentTask").filter(|t| t.is_object());
    let state = current.and_then(|t| str_of(t, "state")).unwrap_or("idle");
    let mut ask = None;

    ui.add_space(space::SM);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = space::LG;
        match status {
            "connected" => face::avatar(
                ui,
                seed,
                face::XL,
                Presence::of(state, str_of(a, "lastSeenAt")),
                name,
            ),
            "revoked" => face::avatar_still(ui, seed, face::XL, Presence::Offline, name),
            _ => face::avatar_still(ui, seed, face::XL, Presence::Waiting, name),
        };
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            if owned {
                viz::more(ui, |ui| {
                    if status == "waiting" && viz::menu_item(ui, "Continue setup", false, None) {
                        ask = Some(Ask::Card(name.to_owned(), CardAsk::Continue));
                    }
                    if viz::menu_item(ui, "Copy handle", false, None) {
                        ui.ctx()
                            .copy_text(str_of(a, "handle").unwrap_or_default().to_owned());
                        w::toast(ui.ctx(), "Copied.", false);
                    }
                    if status != "revoked" {
                        if let Some((role, on)) = role_items(ui, a) {
                            ask = Some(Ask::Card(name.to_owned(), CardAsk::Role(role, on)));
                        }
                        if viz::menu_item(ui, "Rotate token\u{2026}", false, None) {
                            ask = Some(Ask::Card(name.to_owned(), CardAsk::Rotate));
                        }
                        viz::menu_rule(ui);
                        if viz::menu_item(ui, "Revoke\u{2026}", true, None) {
                            ask = Some(Ask::Card(name.to_owned(), CardAsk::Revoke));
                        }
                    }
                });
            }
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = space::XXS;
                ui.add(
                    egui::Label::new(
                        RichText::new(name)
                            .size(text::TITLE)
                            .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                            .color(colour::TEXT),
                    )
                    .truncate(),
                );
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::XS;
                    w::mono_caption(ui, str_of(a, "handle").unwrap_or_default());
                    let mut who = format!(
                        "\u{00B7} {}",
                        runtime_label(str_of(a, "runtime").unwrap_or("other"))
                    );
                    if owned {
                        who += " \u{00B7} your agent";
                    }
                    ui.label(
                        RichText::new(who)
                            .size(text::SMALL)
                            .color(colour::TEXT_MUTED),
                    );
                    ui.add_space(space::SM);
                    face::private_label(ui);
                });
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::XS;
                    role_chips(ui, a);
                });
                ui.add_space(space::XS);
                if let Some(t) = now_line(ui, a, current, state, status, short) {
                    ask = Some(Ask::Task(t));
                }
            });
        });
    });
    ask
}

/// What it is doing, in one line: its now line and the task, or whose move
/// it is. Returns a task to open.
fn now_line(
    ui: &mut egui::Ui,
    a: &Value,
    current: Option<&Value>,
    state: &str,
    status: &str,
    short: &str,
) -> Option<String> {
    let mut open = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::SM;
        match (status, current) {
            ("revoked", _) => w::muted(ui, "Revoked \u{2014} its token no longer works."),
            ("waiting", _) => {
                ui.label(
                    RichText::new("Waiting for first contact")
                        .size(text::SMALL)
                        .color(colour::WARN),
                );
            }
            (_, Some(t)) => {
                let working = matches!(state, "working" | "acknowledged");
                if working {
                    face::spinner(ui, text::BODY);
                }
                let now = str_of(t, "now").map(str::trim).filter(|n| !n.is_empty());
                let words = match now {
                    Some(n) if working => n.to_owned(),
                    _ => state_words(state).to_owned(),
                };
                let ink = if matches!(state, "needs_input" | "plan_review") {
                    colour::WARN
                } else {
                    colour::TEXT_2
                };
                ui.label(RichText::new(words).size(text::BODY).color(ink));
                ui.label(
                    RichText::new("on")
                        .size(text::SMALL)
                        .color(colour::TEXT_MUTED),
                );
                if task_link(ui, str_of(t, "title").unwrap_or("Untitled")) {
                    open = str_of(t, "id").map(str::to_owned);
                }
            }
            _ => {
                let seen = str_of(a, "lastSeenAt")
                    .map(|s| format!(" \u{00B7} seen {}", ago(s)))
                    .unwrap_or_default();
                let next = if has(a, Role::Work) {
                    format!(" Hand {short} one from any task\u{2019}s page.")
                } else {
                    String::new()
                };
                w::muted(ui, &format!("No task right now{seen}.{next}"));
            }
        }
    });
    open
}

// -------------------------------------------------------------------- rail

fn rail(ui: &mut egui::Ui, a: &Value, owned: bool) -> Option<CardAsk> {
    let status = str_of(a, "status").unwrap_or("waiting");
    let intake = a.get("canIntake").and_then(Value::as_bool).unwrap_or(false);
    let mut ask = None;
    shell::property(ui, "Status", |ui| {
        let (words, tone) = status_words(status);
        c::chip(ui, words, tone, true);
    });
    shell::property(ui, "Last seen", |ui| match str_of(a, "lastSeenAt") {
        Some(at) => {
            ui.label(
                RichText::new(ago(at))
                    .size(text::SMALL)
                    .color(colour::TEXT_2),
            )
            .on_hover_text(exact(at));
        }
        None => w::muted(ui, "Never"),
    });
    shell::property(ui, "Connected", |ui| match str_of(a, "connectedAt") {
        Some(at) => {
            ui.label(
                RichText::new(day_label(at))
                    .size(text::SMALL)
                    .color(colour::TEXT_2),
            )
            .on_hover_text(exact(at));
        }
        None => w::muted(ui, "Not yet"),
    });
    shell::property(ui, "Runtime", |ui| {
        ui.label(
            RichText::new(runtime_label(str_of(a, "runtime").unwrap_or("other")))
                .size(text::SMALL)
                .color(colour::TEXT_2),
        );
    });
    shell::property(ui, "Handle", |ui| {
        ui.label(
            RichText::new(str_of(a, "handle").unwrap_or_default())
                .monospace()
                .size(text::SMALL)
                .color(colour::TEXT_2),
        );
    });
    shell::property(ui, "Intake", |ui| {
        let words = if intake {
            "Creates tasks for you"
        } else {
            "Off"
        };
        ui.label(RichText::new(words).size(text::SMALL).color(if intake {
            colour::TEXT_2
        } else {
            colour::TEXT_MUTED
        }));
    });

    ui.add_space(space::MD);
    let (rule, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(
        rule.x_range(),
        rule.center().y,
        egui::Stroke::new(1.0, colour::LINE),
    );
    ui.add_space(space::MD);
    ui.label(
        RichText::new("Setup")
            .size(text::SMALL)
            .family(egui::FontFamily::Name(theme::MEDIUM.into()))
            .color(colour::TEXT_MUTED),
    );
    ui.add_space(space::XS);
    match a
        .get("setup")
        .filter(|s| s.as_object().is_some_and(|o| !o.is_empty()))
    {
        Some(setup) => {
            ui.spacing_mut().item_spacing.y = space::XS;
            setup_checks(ui, setup, false);
        }
        None => w::caption(
            ui,
            "What it installed shows here once it says hello: its skill, MCP server and watcher.",
        ),
    }
    if status == "waiting" && owned {
        ui.add_space(space::MD);
        if w::secondary(ui, "Continue setup", true).clicked() {
            ask = Some(CardAsk::Continue);
        }
    }
    if intake {
        ui.add_space(space::MD);
        w::caption(
            ui,
            "Files what it reads into your Triage, each task labelled Slack.",
        );
    }
    ask
}

/// The rail's facts on one wrapped line, for a narrow window: status, last
/// seen, runtime, setup.
fn facts(ui: &mut egui::Ui, a: &Value, owned: bool) -> Option<CardAsk> {
    let status = str_of(a, "status").unwrap_or("waiting");
    let mut ask = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(space::MD, space::SM);
        let (words, tone) = status_words(status);
        c::chip(ui, words, tone, true);
        if let Some(at) = str_of(a, "lastSeenAt") {
            ui.label(
                RichText::new(format!("seen {}", ago(at)))
                    .size(text::SMALL)
                    .color(colour::TEXT_MUTED),
            )
            .on_hover_text(exact(at));
        }
        ui.label(
            RichText::new(runtime_label(str_of(a, "runtime").unwrap_or("other")))
                .size(text::SMALL)
                .color(colour::TEXT_MUTED),
        );
        if let Some(setup) = a
            .get("setup")
            .filter(|s| s.as_object().is_some_and(|o| !o.is_empty()))
        {
            setup_checks(ui, setup, true);
        }
        if status == "waiting" && owned && w::secondary(ui, "Continue setup", true).clicked() {
            ask = Some(CardAsk::Continue);
        }
    });
    ask
}

// -------------------------------------------------------------------- body

fn body(
    ui: &mut egui::Ui,
    o: &Value,
    a: &Value,
    tab: &mut usize,
    owned: bool,
    seed: &str,
    short: &str,
) -> Option<Ask> {
    let intake = a.get("canIntake").and_then(Value::as_bool).unwrap_or(false);
    let stats = o.get("stats").cloned().unwrap_or(Value::Null);
    let arr = |k: &str| {
        o.get(k)
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice)
    };
    let active = o
        .get("tasks")
        .and_then(|t| t.get("active"))
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let recent_tasks = o
        .get("tasks")
        .and_then(|t| t.get("recent"))
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let mut ask = None;

    ui.add_space(space::XL);
    figures(ui, &stats, intake);
    ui.add_space(space::MD);
    chart(ui, arr("activity"));

    ui.add_space(space::XL);
    let n = |k: &str| arr(k).len();
    let labels = [
        TABS[0].to_owned(),
        count_label(TABS[1], active.len() + recent_tasks.len()),
        count_label(TABS[2], n("runs")),
        count_label(TABS[3], n("logs")),
    ];
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    if let Some(i) = c::tabs(ui, &refs, *tab) {
        *tab = i;
    }
    match *tab {
        1 => ask = tasks(ui, active, recent_tasks, short),
        2 => runs(ui, arr("runs"), intake, short),
        3 => ask = logs(ui, arr("logs"), short),
        _ => ask = activity(ui, arr("recent"), owned, seed, short),
    }
    ask
}

fn count_label(label: &str, n: usize) -> String {
    if n == 0 {
        label.to_owned()
    } else {
        format!("{label} {n}")
    }
}

/// Where the body will be, while the overview loads.
fn skeleton(ui: &mut egui::Ui) {
    ui.add_space(space::XL);
    face::skeleton(ui, ui.available_width(), 52.0);
    ui.add_space(space::MD);
    face::skeleton(ui, ui.available_width(), CHART_H + 48.0);
    ui.add_space(space::XL);
    for wdt in [0.6, 0.45, 0.5] {
        face::skeleton(ui, ui.available_width() * wdt, text::BODY);
        ui.add_space(space::MD);
    }
}

// ------------------------------------------------------------ the figures

/// A row of figures in one band, divided by hairlines — facts to read in a
/// line, not tiles to admire. Which figures depends on what the agent does:
/// work handed to it, filing for its owner, or both.
fn figures(ui: &mut egui::Ui, s: &Value, intake: bool) {
    let n = |k: &str| s.get(k).and_then(Value::as_i64).unwrap_or(0);
    let worker = n("tasksHandled") > 0 || !intake;
    let filer = intake || n("filed") > 0;
    let mut cells: Vec<(String, &str, &str)> = Vec::new();
    if worker {
        cells.push((
            n("tasksHandled").to_string(),
            "Handed to it",
            "Tasks ever handed to it.",
        ));
        cells.push((
            n("tasksDone").to_string(),
            "Approved",
            "Tasks you approved after its report.",
        ));
        cells.push((
            n("inReviewNow").to_string(),
            "In review",
            "Reports waiting on your review now.",
        ));
        if !filer {
            cells.push((
                n("questionsAsked").to_string(),
                "Questions",
                "Questions it asked you.",
            ));
        }
        let ack = s
            .get("medianAckMinutes")
            .and_then(Value::as_f64)
            .map_or_else(|| "\u{2014}".to_owned(), minutes);
        cells.push((
            ack,
            "First reply",
            "Median time from hand-off to its first update or log line.",
        ));
    }
    if filer {
        cells.push((
            n("filed").to_string(),
            "Filed",
            "Tasks it filed into your Triage.",
        ));
        if !worker {
            cells.push((
                n("accepted").to_string(),
                "Accepted",
                "Filed tasks you accepted.",
            ));
            cells.push((
                n("dismissed").to_string(),
                "Dismissed",
                "Filed tasks you dismissed.",
            ));
        }
        let rate = s
            .get("acceptRate")
            .and_then(Value::as_f64)
            .map_or_else(|| "\u{2014}".to_owned(), |r| format!("{:.0}%", r * 100.0));
        cells.push((
            rate,
            "Accept rate",
            "Of the filed tasks you decided on, how many you accepted.",
        ));
    }

    egui::Frame::new()
        .fill(colour::SURFACE)
        .stroke(egui::Stroke::new(1.0, colour::LINE))
        .corner_radius(radius::LG)
        .inner_margin(egui::Margin::symmetric(0, space::MD as i8))
        .show(ui, |ui| {
            let width = ui.available_width();
            let cell_w = width / cells.len().max(1) as f32;
            let value_font =
                egui::FontId::new(text::TITLE, egui::FontFamily::Name(theme::SEMIBOLD.into()));
            let label_font = egui::FontId::proportional(text::SMALL);
            let (rect, _) = ui.allocate_exact_size(
                vec2(width, text::TITLE + text::SMALL + space::SM),
                egui::Sense::hover(),
            );
            for (i, (value, label, hint)) in cells.iter().enumerate() {
                let cell = egui::Rect::from_min_size(
                    pos2(rect.left() + i as f32 * cell_w, rect.top()),
                    vec2(cell_w, rect.height()),
                );
                let r = ui.interact(cell, ui.id().with(("figure", i)), egui::Sense::hover());
                let spoken = format!("{label}: {value}");
                r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &spoken));
                r.on_hover_text(*hint);
                let p = ui.painter();
                let x = cell.left() + pad::CARD.0;
                p.text(
                    pos2(x, cell.top()),
                    egui::Align2::LEFT_TOP,
                    value,
                    value_font.clone(),
                    colour::TEXT,
                );
                let label_g = w::truncated(
                    ui,
                    label,
                    label_font.clone(),
                    colour::TEXT_MUTED,
                    cell_w - pad::CARD.0 * 2.0,
                );
                ui.painter().galley(
                    pos2(x, cell.bottom() - label_g.size().y),
                    label_g,
                    colour::TEXT_MUTED,
                );
                if i > 0 {
                    ui.painter().vline(
                        cell.left(),
                        cell.y_range().expand(space::XXS),
                        egui::Stroke::new(1.0, colour::LINE),
                    );
                }
            }
        });
}

/// "4m", "1.5h", "2d".
fn minutes(m: f64) -> String {
    let trim = |v: f64| {
        let s = format!("{v:.1}");
        s.strip_suffix(".0").map(str::to_owned).unwrap_or(s)
    };
    if m < 1.0 {
        "<1m".into()
    } else if m < 60.0 {
        format!("{:.0}m", m)
    } else if m < 1440.0 {
        format!("{}h", trim(m / 60.0))
    } else {
        format!("{}d", trim(m / 1440.0))
    }
}

// --------------------------------------------------------------- the chart

/// Thirty days as stacked bars: what the agent wrote (its updates), what it
/// filed, and what reached it from the dashboard. An empty day is a stub, so
/// a quiet month still reads as a month.
fn chart(ui: &mut egui::Ui, days: &[Value]) {
    let n = |d: &Value, k: &str| d.get(k).and_then(Value::as_i64).unwrap_or(0) as f32;
    let totals =
        ["notes", "filed", "events"].map(|k| days.iter().map(|d| n(d, k)).sum::<f32>() as i64);
    let series = [
        ("Its updates", colour::AGENT, "notes"),
        ("Filed", colour::INFO, "filed"),
        ("Sent to it", colour::IDLE, "events"),
    ];

    c::surface(ui, false, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Last 30 days")
                    .size(text::SMALL)
                    .family(egui::FontFamily::Name(theme::MEDIUM.into()))
                    .color(colour::TEXT_MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = space::XS;
                let mut shown: Vec<usize> = (0..series.len()).filter(|i| totals[*i] > 0).collect();
                if shown.is_empty() {
                    shown.push(0);
                }
                for (j, &i) in shown.iter().enumerate().rev() {
                    let (label, ink, _) = &series[i];
                    if j < shown.len() - 1 {
                        ui.add_space(space::MD);
                    }
                    ui.label(
                        RichText::new(totals[i].to_string())
                            .size(text::SMALL)
                            .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                            .color(colour::TEXT),
                    );
                    ui.label(
                        RichText::new(*label)
                            .size(text::SMALL)
                            .color(colour::TEXT_MUTED),
                    );
                    let (sw, _) = ui
                        .allocate_exact_size(egui::Vec2::splat(text::SMALL), egui::Sense::hover());
                    ui.painter().rect_filled(
                        egui::Rect::from_center_size(sw.center(), egui::Vec2::splat(8.0)),
                        2.0,
                        *ink,
                    );
                }
            });
        });
        ui.add_space(space::MD);

        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), CHART_H), egui::Sense::hover());
        let spoken = format!(
            "Activity, last 30 days: {} updates, {} filed, {} sent to it",
            totals[0], totals[1], totals[2]
        );
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, &spoken));
        let count = days.len().max(1);
        let bar = ((rect.width() - BAR_GAP * (count - 1) as f32) / count as f32).max(1.0);
        let peak = days
            .iter()
            .map(|d| series.iter().map(|(_, _, k)| n(d, k)).sum::<f32>())
            .fold(1.0, f32::max);
        let hovered = response
            .hover_pos()
            .map(|p| (((p.x - rect.left()) / (bar + BAR_GAP)) as usize).min(count - 1));
        let p = ui.painter();
        p.hline(
            rect.x_range(),
            rect.bottom() + 0.5,
            egui::Stroke::new(1.0, colour::LINE),
        );
        for (i, d) in days.iter().enumerate() {
            let x = rect.left() + i as f32 * (bar + BAR_GAP);
            if hovered == Some(i) {
                p.rect_filled(
                    egui::Rect::from_min_max(
                        pos2(x - 1.0, rect.top()),
                        pos2(x + bar + 1.0, rect.bottom()),
                    ),
                    2.0,
                    colour::SURFACE_HOVER,
                );
            }
            let total: f32 = series.iter().map(|(_, _, k)| n(d, k)).sum();
            if total == 0.0 {
                p.rect_filled(
                    egui::Rect::from_min_size(pos2(x, rect.bottom() - 2.0), vec2(bar, 2.0)),
                    1.0,
                    colour::LINE_STRONG,
                );
                continue;
            }
            let mut y = rect.bottom();
            for (_, ink, k) in &series {
                let v = n(d, k);
                if v == 0.0 {
                    continue;
                }
                let h = (v / peak * CHART_H).max(2.0);
                p.rect_filled(
                    egui::Rect::from_min_max(pos2(x, y - h), pos2(x + bar, y)),
                    1.0,
                    *ink,
                );
                y -= h;
            }
        }
        if let Some(d) = hovered.and_then(|i| days.get(i)) {
            let day = str_of(d, "day").map(date_label).unwrap_or_default();
            let words = format!(
                "{day}: {} updates \u{00B7} {} filed \u{00B7} {} sent to it",
                n(d, "notes") as i64,
                n(d, "filed") as i64,
                n(d, "events") as i64
            );
            response.on_hover_text_at_pointer(words);
        }
        ui.add_space(space::XS);
        ui.horizontal(|ui| {
            w::caption(
                ui,
                &days
                    .first()
                    .and_then(|d| str_of(d, "day"))
                    .map(date_label)
                    .unwrap_or_default(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                w::caption(ui, "Today")
            });
        });
    });
}

// ---------------------------------------------------------------- activity

/// The last fifty things between you and the agent, newest first, grouped by
/// day, in the task page's timeline language: a node per entry on a rail.
fn activity(
    ui: &mut egui::Ui,
    recent: &[Value],
    owned: bool,
    seed: &str,
    short: &str,
) -> Option<Ask> {
    if recent.is_empty() {
        empty(
            ui,
            "Nothing yet",
            &format!(
                "When you hand {short} a task, its updates, questions and reports land here, newest first \u{2014} and anything it files for you."
            ),
        );
        return None;
    }
    let you = if owned { "You" } else { "Its owner" };
    let mut open = None;
    let mut day = String::new();
    let mut nodes: Vec<egui::Rect> = Vec::new();
    let mut rails: Vec<egui::layers::ShapeIdx> = Vec::new();
    let mut groups: Vec<Vec<egui::Rect>> = Vec::new();
    for r in recent {
        let at = str_of(r, "at").unwrap_or_default();
        let this_day = day_label(at);
        if this_day != day {
            if !nodes.is_empty() {
                groups.push(std::mem::take(&mut nodes));
            }
            if !day.is_empty() {
                ui.add_space(space::MD);
            }
            day = this_day;
            ui.label(
                RichText::new(&day)
                    .size(text::CAPTION)
                    .family(egui::FontFamily::Name(theme::MEDIUM.into()))
                    .color(colour::TEXT_MUTED),
            );
            ui.add_space(space::SM);
            rails.push(ui.painter().add(egui::Shape::Noop));
        }
        let kind = str_of(r, "kind").unwrap_or("note");
        let text_body = str_of(r, "text").unwrap_or_default();
        let filed_from = format!("filed it from {text_body}");
        let told = format!("told {short}");
        let (node, who, verb) = match kind {
            "handed_off" => (Node::Person(seed), you, "handed it off"),
            "taken_back" => (Node::Person(seed), you, "took it back"),
            "answer" => (Node::Person(seed), you, "answered"),
            "instruction" => (Node::Person(seed), you, told.as_str()),
            "approved" => (Node::Mark(Mark::Approved), you, "approved it"),
            "changes_requested" => (Node::Mark(Mark::Changes), you, "asked for changes"),
            "question" => (Node::Mark(Mark::Question), short, "asked"),
            "submission" => (
                Node::Mark(Mark::Submitted),
                short,
                "submitted it for review",
            ),
            "filed" => (Node::Mark(Mark::Filed), short, filed_from.as_str()),
            "progress" => (Node::Mark(Mark::Progress), short, "posted an update"),
            _ => (Node::Mark(Mark::Progress), short, "left a note"),
        };
        let shows_text =
            !matches!(kind, "filed" | "handed_off" | "taken_back") && !text_body.trim().is_empty();
        ui.spacing_mut().item_spacing.y = space::XS;
        nodes.push(entry(ui, node, who, verb, Some(at), false, |ui| {
            if task_link(ui, str_of(r, "taskTitle").unwrap_or("Untitled")) {
                open = str_of(r, "taskId").map(str::to_owned);
            }
            if shows_text {
                prose(ui, &one_line(text_body, 280), colour::TEXT_2);
            }
        }));
        ui.add_space(space::MD);
    }
    groups.push(nodes);
    // The hairline joining each day's nodes, drawn under them.
    for (slot, group) in rails.into_iter().zip(groups) {
        let segments: Vec<egui::Shape> = group
            .windows(2)
            .map(|w| {
                let x = w[0].center().x;
                egui::Shape::line_segment(
                    [
                        pos2(x, w[0].bottom() + space::XS),
                        pos2(x, w[1].top() - space::XS),
                    ],
                    egui::Stroke::new(1.0, colour::LINE),
                )
            })
            .collect();
        ui.painter().set(slot, egui::Shape::Vec(segments));
    }
    open.map(Ask::Task)
}

// ------------------------------------------------------------------- tasks

const TASK_COLS: [Col; 4] = [
    Col::fill("Task", 180.0),
    Col::left("Project", 140.0).rank(2),
    Col::left("State", 112.0),
    Col::right("Updated", 72.0).rank(1),
];

fn tasks(ui: &mut egui::Ui, active: &[Value], recent: &[Value], short: &str) -> Option<Ask> {
    if active.is_empty() && recent.is_empty() {
        empty(
            ui,
            "No tasks yet",
            &format!("Hand {short} a task from its page and it is listed here while it works, then under Recent once it is done or taken back."),
        );
        return None;
    }
    let mut open = None;
    for (label, rows, id) in [
        ("Active", active, "agent-page:active"),
        ("Recent", recent, "agent-page:recent"),
    ] {
        if rows.is_empty() {
            continue;
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::XS;
            ui.label(
                RichText::new(label)
                    .size(text::SMALL)
                    .family(egui::FontFamily::Name(theme::MEDIUM.into()))
                    .color(colour::TEXT_MUTED),
            );
            ui.label(
                RichText::new(rows.len().to_string())
                    .size(text::SMALL)
                    .color(colour::TEXT_FAINT),
            );
        });
        ui.add_space(space::SM);
        let clicked = table::show(ui, id, &TASK_COLS, rows.len(), |cells, i| {
            let t = &rows[i];
            cells.at(0, |ui| {
                ui.spacing_mut().item_spacing.x = space::SM;
                if t.get("filed").and_then(Value::as_bool).unwrap_or(false) {
                    c::chip(ui, "Filed", c::Tone::Info, false);
                }
                table::strong_label(ui, str_of(t, "title").unwrap_or("Untitled"), colour::TEXT);
            });
            cells.muted(1, str_of(t, "projectName").unwrap_or("No project"));
            cells.at(2, |ui| match str_of(t, "agentState") {
                Some(s) => {
                    c::chip(ui, state_words(s), state_tone(s), true);
                }
                None => {
                    let s = str_of(t, "status").unwrap_or("open");
                    c::chip(ui, status_label(s), c::status_tone(s), true);
                }
            });
            cells.muted(3, &str_of(t, "updatedAt").map(ago).unwrap_or_default());
        });
        if let Some(i) = clicked {
            open = str_of(&rows[i], "id").map(str::to_owned);
        }
        ui.add_space(space::LG);
    }
    open.map(Ask::Task)
}

// -------------------------------------------------------------------- runs

/// Each pass it reported: a strip of the last fifty at a glance, then one
/// row each — when, how it went, what it found — with the error folded
/// under a failed one.
fn runs(ui: &mut egui::Ui, runs: &[Value], intake: bool, short: &str) {
    if runs.is_empty() {
        let detail = if intake {
            format!("Each time {short} reads Slack it reports a run \u{2014} when, what it filed and skipped, and any error \u{2014} and it lands here.")
        } else {
            format!("A run is one pass an agent makes on a schedule, like an intake sweep of Slack. {short} has not reported any.")
        };
        empty(ui, "No runs reported yet", &detail);
        return;
    }

    // ---- the strip, oldest to newest, and the rhythm in words
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::MD;
        let width = runs.len() as f32 * (TICK_W + 2.0) - 2.0;
        let (rect, response) = ui.allocate_exact_size(vec2(width, TICK_H), egui::Sense::hover());
        let failed = runs
            .iter()
            .filter(|r| str_of(r, "status") == Some("failed"))
            .count();
        let spoken = format!("Last {} runs, {failed} failed", runs.len());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, &spoken));
        for (i, r) in runs.iter().rev().enumerate() {
            let x = rect.left() + i as f32 * (TICK_W + 2.0);
            ui.painter().rect_filled(
                egui::Rect::from_min_size(pos2(x, rect.top()), vec2(TICK_W, TICK_H)),
                1.0,
                run_ink(str_of(r, "status").unwrap_or("ok")),
            );
        }
        let mut words = str_of(&runs[0], "finishedAt")
            .map(|at| format!("Last run {}", ago(at)))
            .unwrap_or_default();
        if let Some(every) = cadence(runs) {
            words += &format!(" \u{00B7} about every {every}");
        }
        if failed > 0 {
            words += &format!(" \u{00B7} {failed} failed");
        }
        w::muted(ui, &words);
    });
    ui.add_space(space::MD);

    w::card_list(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 0.0;
        for (i, r) in runs.iter().enumerate() {
            if i > 0 {
                let (rule, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 1.0), egui::Sense::hover());
                ui.painter().hline(
                    rule.x_range(),
                    rule.center().y,
                    egui::Stroke::new(1.0, colour::LINE_SOFT),
                );
            }
            run_row(ui, r);
        }
    });
}

fn run_ink(status: &str) -> egui::Color32 {
    match status {
        "failed" => colour::DANGER,
        "partial" => colour::WARN,
        _ => colour::OK,
    }
}

fn run_row(ui: &mut egui::Ui, r: &Value) {
    let status = str_of(r, "status").unwrap_or("ok");
    let (words, tone) = match status {
        "failed" => ("Failed", c::Tone::Blocked),
        "partial" => ("Partial", c::Tone::Running),
        _ => ("OK", c::Tone::Ok),
    };
    let finished = str_of(r, "finishedAt").unwrap_or_default();
    let took = match (str_of(r, "startedAt").and_then(parse), parse(finished)) {
        (Some(a), Some(b)) => duration((b - a).num_seconds()),
        _ => String::new(),
    };
    ui.horizontal(|ui| {
        ui.set_min_height(table::ROW_H);
        ui.spacing_mut().item_spacing.x = space::SM;
        ui.add_space(space::SM);
        fixed(ui, 64.0, |ui| {
            c::chip(ui, words, tone, true);
        });
        fixed(ui, 96.0, |ui| {
            ui.label(
                RichText::new(ago(finished))
                    .size(text::SMALL)
                    .color(colour::TEXT_2),
            )
            .on_hover_text(exact(finished));
            if !took.is_empty() {
                ui.label(
                    RichText::new(took)
                        .size(text::SMALL)
                        .color(colour::TEXT_FAINT),
                );
            }
        });
        counts(ui, r.get("counts").unwrap_or(&Value::Null));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(space::SM);
            if let Some(s) = str_of(r, "summary").filter(|s| !s.is_empty()) {
                ui.add(
                    egui::Label::new(RichText::new(s).size(text::SMALL).color(colour::TEXT_MUTED))
                        .truncate(),
                );
            }
        });
    });
    if let Some(err) = str_of(r, "error") {
        let id = egui::Id::new((
            "agent-page:run-error",
            r.get("id").and_then(Value::as_i64).unwrap_or(0),
        ));
        let mut open: bool = ui.ctx().data(|d| d.get_temp(id)).unwrap_or(false);
        ui.horizontal(|ui| {
            ui.add_space(space::SM + 64.0 + space::SM - space::XS);
            face::disclosure(ui, id, "Error", None, &mut open);
            ui.ctx().data_mut(|d| d.insert_temp(id, open));
        });
        if open {
            ui.horizontal(|ui| {
                ui.add_space(space::SM + 64.0 + space::SM);
                log_well(ui, |ui| {
                    ui.label(
                        RichText::new(err)
                            .monospace()
                            .size(text::SMALL)
                            .color(colour::LOG_TEXT),
                    );
                });
            });
            ui.add_space(space::SM);
        }
    }
}

/// "filed 2 · appended 1 · skipped 5", the numbers in white. What it found
/// nothing new in says so.
fn counts(ui: &mut egui::Ui, c: &Value) {
    let parts: Vec<(&str, i64)> = [
        ("filed", "filed"),
        ("appended", "appended"),
        ("alreadyFiled", "already filed"),
        ("skipped", "skipped"),
    ]
    .iter()
    .map(|(k, word)| (*word, c.get(*k).and_then(Value::as_i64).unwrap_or(0)))
    .filter(|(_, n)| *n > 0)
    .collect();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::XS;
        if parts.is_empty() {
            ui.label(
                RichText::new("nothing new")
                    .size(text::SMALL)
                    .color(colour::TEXT_MUTED),
            );
        }
        for (i, (word, n)) in parts.iter().enumerate() {
            if i > 0 {
                ui.label(
                    RichText::new("\u{00B7}")
                        .size(text::SMALL)
                        .color(colour::TEXT_FAINT),
                );
            }
            ui.label(
                RichText::new(*word)
                    .size(text::SMALL)
                    .color(colour::TEXT_MUTED),
            );
            ui.label(
                RichText::new(n.to_string())
                    .size(text::SMALL)
                    .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                    .color(colour::TEXT),
            );
        }
    });
}

/// The typical gap between runs, if there are enough to say.
fn cadence(runs: &[Value]) -> Option<String> {
    let times: Vec<DateTime<chrono::Utc>> = runs
        .iter()
        .filter_map(|r| str_of(r, "startedAt").and_then(parse))
        .collect();
    if times.len() < 3 {
        return None;
    }
    let mut gaps: Vec<i64> = times
        .windows(2)
        .map(|w| (w[0] - w[1]).num_seconds().abs())
        .collect();
    gaps.sort_unstable();
    Some(duration(gaps[gaps.len() / 2]))
}

fn duration(secs: i64) -> String {
    match secs.max(0) {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{} min", (s + 30) / 60),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

// -------------------------------------------------------------------- logs

/// The newest step-log lines across its tasks, grouped by task, each group
/// in order — the same well and gutter as the task page's log.
fn logs(ui: &mut egui::Ui, lines: &[Value], short: &str) -> Option<Ask> {
    if lines.is_empty() {
        empty(
            ui,
            "No step log yet",
            &format!("While {short} works a task it logs the commands it runs and what they print. The latest hundred lines show here, grouped by task."),
        );
        return None;
    }
    let mut open = None;
    // Consecutive lines of one task are a group; the order is the server's.
    let mut groups: Vec<(&str, &str, Vec<&Value>)> = Vec::new();
    for l in lines {
        let task = str_of(l, "taskId").unwrap_or_default();
        match groups.last_mut() {
            Some((t, _, rows)) if *t == task => rows.push(l),
            _ => groups.push((task, str_of(l, "taskTitle").unwrap_or("Untitled"), vec![l])),
        }
    }
    ui.horizontal(|ui| {
        w::muted(
            ui,
            &format!(
                "The latest {} lines across its tasks, oldest first.",
                lines.len()
            ),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if w::link(ui, "Copy all").clicked() {
                let all: Vec<String> = groups
                    .iter()
                    .map(|(_, title, rows)| {
                        format!(
                            "# {title}\n{}",
                            rows.iter()
                                .map(|l| str_of(l, "text").unwrap_or_default())
                                .collect::<Vec<_>>()
                                .join("\n")
                        )
                    })
                    .collect();
                ui.ctx().copy_text(all.join("\n\n"));
                w::toast(ui.ctx(), "Log copied.", false);
            }
        });
    });
    ui.add_space(space::MD);
    for (task, title, rows) in &groups {
        ui.horizontal(|ui| {
            if task_link(ui, title) {
                open = Some((*task).to_owned());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let copy = w::link(ui, "Copy");
                if copy.clicked() {
                    let text: Vec<&str> = rows
                        .iter()
                        .map(|l| str_of(l, "text").unwrap_or_default())
                        .collect();
                    ui.ctx().copy_text(text.join("\n"));
                    w::toast(ui.ctx(), "Log copied.", false);
                }
            });
        });
        ui.add_space(space::XS);
        let gutter = rows
            .iter()
            .filter_map(|l| l.get("seq").and_then(Value::as_i64))
            .max()
            .unwrap_or(0)
            .to_string()
            .len()
            .max(3);
        log_well(ui, |ui| {
            ui.spacing_mut().item_spacing.y = space::XXS;
            for l in rows {
                ui.horizontal_top(|ui| {
                    let seq = l.get("seq").and_then(Value::as_i64).unwrap_or(0);
                    ui.label(
                        RichText::new(format!("{seq:>gutter$}"))
                            .monospace()
                            .size(text::SMALL)
                            .color(colour::LOG_SEQ),
                    );
                    ui.add_space(space::SM);
                    ui.label(
                        RichText::new(str_of(l, "text").unwrap_or_default())
                            .monospace()
                            .size(text::SMALL)
                            .color(colour::LOG_TEXT),
                    );
                });
            }
        });
        ui.add_space(space::LG);
    }
    open.map(Ask::Task)
}

fn log_well(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(colour::LOG_BG)
        .stroke(egui::Stroke::new(1.0, colour::LINE_SOFT))
        .corner_radius(radius::MD)
        .inner_margin(egui::Margin::symmetric(
            pad::CARD.0 as i8,
            pad::CARD.1 as i8,
        ))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

// ------------------------------------------------------------------ pieces

/// An empty section: what is missing, and what will appear and how.
fn empty(ui: &mut egui::Ui, title: &str, detail: &str) {
    c::surface(ui, false, |ui| {
        ui.set_width(ui.available_width());
        ui.add_space(space::SM);
        ui.label(
            RichText::new(title)
                .size(text::BODY)
                .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                .color(colour::TEXT),
        );
        ui.add_space(space::XXS);
        ui.label(
            RichText::new(detail)
                .size(text::SMALL)
                .color(colour::TEXT_MUTED),
        );
        ui.add_space(space::SM);
    });
}

/// A task's title that opens it: muted, brightening with an underline on
/// hover. True when clicked.
fn task_link(ui: &mut egui::Ui, title: &str) -> bool {
    let r = ui
        .add(
            egui::Label::new(
                RichText::new(title)
                    .size(text::SMALL)
                    .color(colour::TEXT_MUTED),
            )
            .truncate()
            .sense(egui::Sense::click()),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Link, true, title));
    if r.hovered() || r.has_focus() {
        let y = r.rect.bottom() - 0.5;
        ui.painter().hline(
            r.rect.x_range(),
            y,
            egui::Stroke::new(1.0, colour::TEXT_MUTED),
        );
    }
    r.clicked()
}

fn fixed(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(
        vec2(width, table::ROW_H),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.x = space::XS;
            add(ui);
        },
    );
}

fn one_line(s: &str, max: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > max {
        format!("{}\u{2026}", flat.chars().take(max).collect::<String>())
    } else {
        flat
    }
}

fn parse(s: &str) -> Option<DateTime<chrono::Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|t| t.with_timezone(&chrono::Utc))
}

/// "Today", "Yesterday", or "Mon 22 Sep", in local time.
fn day_label(at: &str) -> String {
    let Some(t) = parse(at) else {
        return String::new();
    };
    let day = t.with_timezone(&Local).date_naive();
    let today = Local::now().date_naive();
    match (today - day).num_days() {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        _ => day.format("%a %-d %b").to_string(),
    }
}

/// "27 Aug", from the chart's `YYYY-MM-DD`.
fn date_label(day: &str) -> String {
    NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .map(|d| d.format("%-d %b").to_string())
        .unwrap_or_default()
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn figures_read_as_words() {
        assert_eq!(minutes(0.4), "<1m");
        assert_eq!(minutes(12.4), "12m");
        assert_eq!(minutes(90.0), "1.5h");
        assert_eq!(minutes(120.0), "2h");
        assert_eq!(minutes(2880.0), "2d");
        assert_eq!(duration(42), "42s");
        assert_eq!(duration(900), "15 min");
    }

    #[test]
    fn cadence_is_the_median_gap() {
        let runs: Vec<Value> = [0, 15, 30, 50]
            .iter()
            .map(|m| json!({ "startedAt": (chrono::Utc::now() - chrono::Duration::minutes(*m)).to_rfc3339() }))
            .collect();
        assert_eq!(cadence(&runs).as_deref(), Some("15 min"));
        assert_eq!(cadence(&runs[..2]), None);
    }
}
