//! Slack's mrkdwn, drawn: `*bold*`, `_italic_`, `~strike~`, `` `code` ``,
//! fenced blocks, `<url|label>` and bare links (clickable), mentions,
//! bullets and line breaks. The agent sends names already resolved; a
//! leftover `<@U…>` reads as a quiet "@someone" rather than an id.
//!
//! A paragraph is one label — one accessible name, the plain text — and a
//! click on it is hit-tested against the link spans it holds.
//!
//! A line that carries a slack.com link is the one exception: it prints as a
//! "View conversation" button rather than a wall of `archives/…` text, and the
//! common "— Name in #channel: <url>" attribution collapses to one quiet line
//! plus the button. A long link's visible text is elided either way, with the
//! full address on hover.

use egui::text::{LayoutJob, TextFormat};
use egui::{FontFamily, FontId, RichText, Sense, Stroke};

use crate::desktop::design::{colour, radius, shell, space, text, theme, widgets as w};

#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    /// A mention the agent could not resolve: muted.
    pub quiet: bool,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Span {
    pub text: String,
    pub style: Style,
    pub link: Option<String>,
}

#[derive(Clone, PartialEq, Debug)]
pub enum Block {
    /// Consecutive lines, their breaks kept.
    Para(Vec<Span>),
    Bullet(Vec<Span>),
    /// A `>` line.
    Quote(Vec<Span>),
    Code(String),
    Gap,
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

/// The blocks of a message.
pub fn parse(src: &str) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    let mut lines = src.lines().peekable();
    let mut gap = false;
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            gap |= !out.is_empty();
            continue;
        }
        if std::mem::take(&mut gap) {
            out.push(Block::Gap);
        }
        if let Some(rest) = trimmed.strip_prefix("```") {
            // Everything to the closing fence, which may share a line with code.
            let mut code: Vec<String> = Vec::new();
            let mut tail = rest.to_owned();
            loop {
                if let Some(end) = tail.find("```") {
                    code.push(tail[..end].to_owned());
                    break;
                }
                code.push(tail);
                match lines.next() {
                    Some(l) => tail = l.to_owned(),
                    None => break,
                }
            }
            let body = code.join("\n");
            out.push(Block::Code(unescape(body.trim_matches('\n'))));
            continue;
        }
        let bullet = ["\u{2022} ", "- ", "* ", "\u{25E6} "].iter().find_map(|m| trimmed.strip_prefix(m));
        if let Some(item) = bullet {
            out.push(Block::Bullet(inline(item)));
            continue;
        }
        if let Some(q) = trimmed.strip_prefix("&gt;").or_else(|| trimmed.strip_prefix('>')) {
            out.push(Block::Quote(inline(q.trim_start())));
            continue;
        }
        let spans = inline(line.trim_end());
        match out.last_mut() {
            // A line straight after a paragraph line continues it.
            Some(Block::Para(prev)) if !prev_blank(src, line) => {
                prev.push(Span { text: "\n".into(), style: Style::default(), link: None });
                prev.extend(spans);
            }
            _ => out.push(Block::Para(spans)),
        }
    }
    out
}

/// Whether a blank line comes right before `line` — a paragraph break
/// rather than a line break. Found by address, so a repeated line is fine.
fn prev_blank(src: &str, line: &str) -> bool {
    let at = line.as_ptr() as usize - src.as_ptr() as usize;
    let before = src[..at].trim_end_matches([' ', '\t']);
    before.ends_with("\n\n") || before.ends_with("\n\r\n")
}

fn opens(prev: Option<char>, next: Option<char>) -> bool {
    prev.is_none_or(|c| c.is_whitespace() || "([{\"'".contains(c)) && next.is_some_and(|c| !c.is_whitespace())
}

/// The inline spans of one line.
pub fn inline(s: &str) -> Vec<Span> {
    let mut out = Vec::new();
    spans(s, Style::default(), &mut out);
    out
}

fn spans(s: &str, style: Style, out: &mut Vec<Span>) {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let mut plain = String::new();
    let flush = |plain: &mut String, out: &mut Vec<Span>| {
        if !plain.is_empty() {
            out.push(Span { text: unescape(plain), style, link: None });
            plain.clear();
        }
    };
    let mut i = 0;
    while i < chars.len() {
        let (at, c) = chars[i];
        let prev = i.checked_sub(1).map(|p| chars[p].1);
        let next = chars.get(i + 1).map(|n| n.1);
        if prev.is_none_or(|p| p.is_whitespace() || p == '(') {
            if let Some((len, target)) = link_at(&s[at..]) {
                flush(&mut plain, out);
                out.push(Span { text: unescape(&s[at..at + len]), style, link: Some(unescape(&target)) });
                i = chars.partition_point(|(b, _)| *b < at + len);
                continue;
            }
        }
        match c {
            '<' => {
                if let Some(end) = s[at..].find('>') {
                    flush(&mut plain, out);
                    out.push(token(&s[at + 1..at + end], style));
                    i = chars.partition_point(|(b, _)| *b <= at + end);
                    continue;
                }
            }
            '`' => {
                if let Some(end) = s[at + 1..].find('`').filter(|e| *e > 0) {
                    flush(&mut plain, out);
                    let code = Style { code: true, ..style };
                    out.push(Span { text: unescape(&s[at + 1..at + 1 + end]), style: code, link: None });
                    i = chars.partition_point(|(b, _)| *b <= at + 1 + end);
                    continue;
                }
            }
            '*' | '_' | '~' if opens(prev, next) => {
                // The closing marker: after a non-space, before a boundary.
                let close = (i + 2..chars.len()).find(|&j| {
                    chars[j].1 == c
                        && !chars[j - 1].1.is_whitespace()
                        && chars.get(j + 1).is_none_or(|n| !n.1.is_alphanumeric())
                });
                if let Some(j) = close {
                    flush(&mut plain, out);
                    let inner = match c {
                        '*' => Style { bold: true, ..style },
                        '_' => Style { italic: true, ..style },
                        _ => Style { strike: true, ..style },
                    };
                    spans(&s[chars[i + 1].0..chars[j].0], inner, out);
                    i = j + 1;
                    continue;
                }
            }
            _ => {}
        }
        plain.push(c);
        i += 1;
    }
    flush(&mut plain, out);
}

const PATH_ROOTS: [&str; 7] = ["~/", "/Users/", "/Volumes/", "/tmp/", "/private/", "/Applications/", "/opt/"];
const BARE_SCHEMES: [&str; 4] = ["mailto:", "tel:", "sms:", "facetime:"];

fn link_at(s: &str) -> Option<(usize, String)> {
    let word = &s[..s.find(char::is_whitespace).unwrap_or(s.len())];
    let word = word.trim_end_matches(['.', ',', ')', '!', '?', ';', ':', '"', '\'']);
    let target = if let Some((scheme, rest)) = word.split_once("://") {
        if !is_scheme(scheme) || rest.is_empty() {
            return None;
        }
        word.to_owned()
    } else if BARE_SCHEMES.iter().any(|p| word.strip_prefix(p).is_some_and(|rest| !rest.is_empty())) {
        word.to_owned()
    } else if word.strip_prefix("www.").is_some_and(|rest| rest.contains('.')) {
        format!("https://{word}")
    } else if PATH_ROOTS.iter().any(|r| word.len() > r.len() && word.starts_with(r)) {
        word.to_owned()
    } else if is_email(word) {
        format!("mailto:{word}")
    } else {
        return None;
    };
    allowed(&target).then_some((word.len(), target))
}

fn is_scheme(s: &str) -> bool {
    s.len() >= 2
        && s.starts_with(|c: char| c.is_ascii_alphabetic())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

fn is_email(word: &str) -> bool {
    let Some((user, host)) = word.split_once('@') else { return false };
    let tld = host.rsplit('.').next().unwrap_or_default();
    !user.is_empty()
        && user.chars().all(|c| c.is_ascii_alphanumeric() || "._%+-".contains(c))
        && host.contains('.')
        && host.chars().all(|c| c.is_ascii_alphanumeric() || ".-".contains(c))
        && tld.len() >= 2
        && tld.chars().all(|c| c.is_ascii_alphabetic())
}

#[cfg(target_os = "macos")]
const OPENER: (&str, &[&str]) = ("open", &["--"]);
#[cfg(not(target_os = "macos"))]
const OPENER: (&str, &[&str]) = ("xdg-open", &[]);

const APP_SCHEMES: [&str; 5] = ["figma", "slack", "notion", "linear", "zoommtg"];
const DOCUMENTS: [&str; 30] = [
    "pdf", "png", "jpg", "jpeg", "gif", "webp", "heic", "tif", "tiff", "svg", "md", "markdown", "txt", "csv", "tsv",
    "json", "log", "rtf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "key", "numbers", "pages", "mov", "mp4", "mp3",
];

#[derive(Debug, PartialEq)]
enum Plan {
    Web,
    Open(String),
    Reveal(String),
    Missing(std::path::PathBuf),
    Refused,
}

fn is_web(target: &str) -> bool {
    let lower = target.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

fn allowed(target: &str) -> bool {
    let lower = target.to_ascii_lowercase();
    is_web(target)
        || lower.starts_with("file:")
        || target.starts_with('/')
        || target.starts_with("~/")
        || BARE_SCHEMES.iter().any(|p| lower.starts_with(p))
        || lower.split_once("://").is_some_and(|(scheme, _)| APP_SCHEMES.contains(&scheme))
}

fn plan(target: &str) -> Plan {
    if !allowed(target) {
        return Plan::Refused;
    }
    if is_web(target) {
        return Plan::Web;
    }
    match local_path(target) {
        Some(path) => match std::fs::canonicalize(&path) {
            Err(_) => Plan::Missing(path),
            Ok(real) if opens_in_place(&real) => Plan::Open(real.display().to_string()),
            Ok(real) => Plan::Reveal(real.display().to_string()),
        },
        None => Plan::Open(target.to_owned()),
    }
}

pub fn open(ctx: &egui::Context, target: &str) {
    if is_web(target) {
        ctx.open_url(egui::OpenUrl::new_tab(target));
        return;
    }
    let ctx = ctx.clone();
    let target = target.to_owned();
    std::thread::spawn(move || {
        let failed = match plan(&target) {
            Plan::Web | Plan::Refused => Some("Loop opens web, mail, file, Figma, Slack, Notion, Linear and Zoom links only.".to_owned()),
            Plan::Missing(path) => Some(format!("{} isn\u{2019}t on this Mac.", path.display())),
            Plan::Open(operand) => (!launch(false, &operand)).then(|| format!("Nothing on this Mac opens {target}.")),
            Plan::Reveal(operand) => (!launch(true, &operand)).then(|| format!("Nothing on this Mac opens {target}.")),
        };
        if let Some(message) = failed {
            w::toast(&ctx, message, true);
            ctx.request_repaint();
        }
    });
}

fn launch(reveal: bool, operand: &str) -> bool {
    let (opener, end) = OPENER;
    let args = reveal.then_some("-R").into_iter().chain(end.iter().copied());
    std::process::Command::new(opener).args(args).arg(operand).status().is_ok_and(|s| s.success())
}

fn local_path(target: &str) -> Option<std::path::PathBuf> {
    if target.get(..5).is_some_and(|p| p.eq_ignore_ascii_case("file:")) {
        return reqwest::Url::parse(target).ok()?.to_file_path().ok();
    }
    if let Some(rest) = target.strip_prefix("~/") {
        return std::env::home_dir().map(|home| home.join(rest));
    }
    target.starts_with('/').then(|| target.into())
}

fn opens_in_place(path: &std::path::Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    if path.is_dir() {
        return ext.is_none();
    }
    ext.is_some_and(|e| DOCUMENTS.contains(&e.as_str()))
}

/// One `<…>`: a link, a mention, a channel or a special.
fn token(inner: &str, style: Style) -> Span {
    let (target, label) = match inner.split_once('|') {
        Some((t, l)) => (t, Some(l)),
        None => (inner, None),
    };
    let plain = |text: String, style: Style| Span { text, style, link: None };
    match target.chars().next() {
        Some('@') => match label {
            Some(name) => plain(format!("@{name}"), style),
            None => plain("@someone".into(), Style { quiet: true, ..style }),
        },
        Some('#') => match label {
            Some(name) => plain(format!("#{name}"), style),
            None => plain("#channel".into(), Style { quiet: true, ..style }),
        },
        Some('!') => {
            let word = target[1..].split('^').next().unwrap_or_default();
            plain(format!("@{}", label.unwrap_or(word)), style)
        }
        _ => {
            let url = unescape(target);
            let shown = label.map(unescape).unwrap_or_else(|| url.trim_start_matches("mailto:").to_owned());
            Span { text: shown, style, link: allowed(&url).then_some(url) }
        }
    }
}

/// The plain text a list row can show in a line: markup gone.
pub fn plain(src: &str) -> String {
    parse(src)
        .into_iter()
        .map(|b| match b {
            Block::Para(s) | Block::Bullet(s) | Block::Quote(s) => s.into_iter().map(|s| s.text).collect(),
            Block::Code(c) => c,
            Block::Gap => String::new(),
        })
        .collect::<Vec<String>>()
        .join(" ")
}

// ------------------------------------------------------------------ drawing

/// How much of a link's own text shows before it is cut with an ellipsis —
/// long enough to still look like a URL, short enough that one doesn't run
/// the paragraph off the edge. The full address is always one hover away.
const LINK_CAP: usize = 60;

fn elide_link_text(text: &str) -> String {
    if text.chars().count() <= LINK_CAP {
        return text.to_owned();
    }
    let head: String = text.chars().take(LINK_CAP - 1).collect();
    format!("{head}\u{2026}")
}

fn job(spans: &[Span], ink: egui::Color32, wrap: f32) -> (LayoutJob, Vec<(std::ops::Range<usize>, String)>) {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    let mut links = Vec::new();
    for s in spans {
        let family = if s.style.code {
            FontFamily::Monospace
        } else if s.style.bold {
            FontFamily::Name(theme::SEMIBOLD.into())
        } else {
            FontFamily::Proportional
        };
        let size = if s.style.code { text::SMALL } else { text::BODY };
        let colour = match () {
            _ if s.link.is_some() => colour::ACCENT(),
            _ if s.style.quiet => colour::TEXT_MUTED(),
            _ if s.style.bold => colour::TEXT(),
            _ => ink,
        };
        let format = TextFormat {
            font_id: FontId::new(size, family),
            color: colour,
            italics: s.style.italic,
            background: if s.style.code { colour::INSET() } else { egui::Color32::TRANSPARENT },
            strikethrough: if s.style.strike { Stroke::new(1.0, colour) } else { Stroke::NONE },
            underline: if s.link.is_some() { Stroke::new(1.0, colour::ACCENT().gamma_multiply(0.5)) } else { Stroke::NONE },
            line_height: Some(text::BODY * 1.45),
            ..Default::default()
        };
        let start = job.text.len();
        let shown = if s.link.is_some() { elide_link_text(&s.text) } else { s.text.clone() };
        job.append(&shown, 0.0, format);
        if let Some(url) = &s.link {
            links.push((start..job.text.len(), url.clone()));
        }
    }
    (job, links)
}

/// One paragraph as a single label; a click on a link opens it.
fn paragraph(ui: &mut egui::Ui, spans: &[Span], ink: egui::Color32) {
    let (job, links) = job(spans, ink, ui.available_width());
    let sense = if links.is_empty() { Sense::hover() } else { Sense::click() };
    let (pos, galley, response) = shell::selectable(ui, egui::Label::new(job).sense(sense));
    let under = |p: egui::Pos2| {
        let at = galley.cursor_from_pos(p - pos).index.0;
        // A char index into the text; the ranges are byte offsets.
        let byte = galley.text().char_indices().nth(at).map_or(galley.text().len(), |(b, _)| b);
        links.iter().find(|(r, _)| r.contains(&byte)).map(|(_, u)| u.clone())
    };
    if let Some(url) = response.hover_pos().and_then(under) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        response.clone().on_hover_text_at_pointer(url.clone());
        if response.clicked() {
            open(ui.ctx(), &url);
        }
    }
}

// -------------------------------------------------------------- slack links

/// Any host under slack.com: an archive link, a file, a canvas.
fn is_slack_url(url: &str) -> bool {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest).to_ascii_lowercase();
    host == "slack.com" || host.ends_with(".slack.com")
}

/// "— Shibu Singh in #issues-and-feedback:" into `(name, channel)`. Only the
/// exact attribution shape matches; anything else falls through to the
/// generic rendering.
fn parse_attribution(text: &str) -> Option<(&str, &str)> {
    let rest = text.trim().strip_suffix(':')?;
    let rest = rest.trim_start_matches(['\u{2014}', '-']).trim();
    let (name, tail) = rest.split_once(" in #")?;
    let (name, channel) = (name.trim(), tail.trim());
    (!name.is_empty() && !channel.is_empty() && !channel.contains(char::is_whitespace)).then_some((name, channel))
}

/// A `#channel` mentioned earlier on the same line, for the generic chip.
fn find_channel(text: &str) -> Option<&str> {
    let after = &text[text.rfind('#')? + 1..];
    let end = after.find(|c: char| c.is_whitespace() || c == ':').unwrap_or(after.len());
    (!after[..end].is_empty()).then_some(&after[..end])
}

/// The button that replaces a raw Slack URL: a small mark, "View
/// conversation", the channel when one is known, the full link on hover.
fn slack_chip(ui: &mut egui::Ui, url: &str, channel: Option<&str>) {
    let label = match channel {
        Some(c) => format!("View conversation \u{00B7} #{c}"),
        None => "View conversation".to_owned(),
    };
    let r = w::icon_button(ui, egui_phosphor::regular::SLACK_LOGO, &label, w::Emphasis::Secondary, true).on_hover_text(url);
    if r.clicked() {
        open(ui.ctx(), url);
    }
}

/// A literal `"\n"` sentinel span: the join `parse` inserts between two source
/// lines it folded into one paragraph (no blank line between them).
fn is_break(s: &Span) -> bool {
    s.text == "\n" && s.link.is_none()
}

/// One block's spans, split at its line breaks — each original source line on
/// its own, since only a whole line can be the Slack attribution shape.
fn lines(spans: &[Span]) -> Vec<&[Span]> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, s) in spans.iter().enumerate() {
        if is_break(s) {
            out.push(&spans[start..i]);
            start = i + 1;
        }
    }
    out.push(&spans[start..]);
    out
}

/// A block's spans, one source line at a time: a line holding a slack.com
/// link renders as a chip (and the attribution shape collapses first);
/// everything else prints exactly as `paragraph` always has.
fn body_lines(ui: &mut egui::Ui, spans: &[Span], ink: egui::Color32) {
    for (i, line) in lines(spans).into_iter().enumerate() {
        if i > 0 {
            ui.add_space(space::XXS);
        }
        match line.iter().position(|s| s.link.as_deref().is_some_and(is_slack_url)) {
            Some(at) => slack_line(ui, line, at, ink),
            None => paragraph(ui, line, ink),
        }
    }
}

fn slack_line(ui: &mut egui::Ui, line: &[Span], at: usize, ink: egui::Color32) {
    let url = line[at].link.clone().unwrap_or_default();
    let pre: String = line[..at].iter().map(|s| s.text.as_str()).collect();
    let post = &line[at + 1..];
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(space::XS, space::XXS);
        match parse_attribution(&pre) {
            Some((name, channel)) => {
                ui.label(RichText::new(format!("{name} \u{00B7} #{channel}")).size(text::SMALL).color(colour::TEXT_MUTED()));
                slack_chip(ui, &url, None);
            }
            None => {
                let trimmed = pre.trim();
                if !trimmed.is_empty() {
                    ui.label(RichText::new(trimmed.to_owned()).size(text::BODY).color(ink));
                }
                slack_chip(ui, &url, find_channel(&pre));
            }
        }
        if !post.is_empty() {
            let (job, _) = job(post, ink, ui.available_width());
            ui.add(egui::Label::new(job));
        }
    });
}

/// A message, drawn in `ink`.
pub fn show(ui: &mut egui::Ui, src: &str, ink: egui::Color32) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = space::XS;
        for block in parse(src) {
            match block {
                Block::Gap => ui.add_space(space::SM),
                Block::Para(s) => body_lines(ui, &s, ink),
                Block::Quote(s) => {
                    ui.horizontal_top(|ui| {
                        ui.add_space(space::MD);
                        ui.vertical(|ui| body_lines(ui, &s, colour::TEXT_MUTED()));
                    });
                }
                Block::Bullet(s) => {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = space::SM;
                        // A painted dot: the bullet sits on the first line's middle.
                        let (r, _) = ui.allocate_exact_size(egui::vec2(space::SM, text::BODY * 1.45), Sense::hover());
                        ui.painter().circle_filled(egui::pos2(r.center().x, r.center().y), 2.0, colour::TEXT_MUTED());
                        ui.vertical(|ui| body_lines(ui, &s, ink));
                    });
                }
                Block::Code(code) => {
                    egui::Frame::new()
                        .fill(colour::INSET())
                        .stroke(Stroke::new(1.0, colour::LINE_SOFT()))
                        .corner_radius(radius::SM)
                        .inner_margin(egui::Margin::symmetric(space::MD as i8, space::SM as i8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            shell::selectable(
                                ui,
                                egui::Label::new(RichText::new(code).monospace().size(text::SMALL).color(colour::TEXT_2())),
                            );
                        });
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(spans: &[Span]) -> Vec<(&str, Style, Option<&str>)> {
        spans.iter().map(|s| (s.text.as_str(), s.style, s.link.as_deref())).collect()
    }

    #[test]
    fn inline_styles_links_and_mentions() {
        let b = Style { bold: true, ..Default::default() };
        let i = Style { italic: true, ..Default::default() };
        let st = Style { strike: true, ..Default::default() };
        let c = Style { code: true, ..Default::default() };
        let q = Style { quiet: true, ..Default::default() };
        let n = Style::default();
        assert_eq!(
            texts(&inline("a *bold* _it_ ~gone~ `x*y*` end")),
            vec![("a ", n, None), ("bold", b, None), (" ", n, None), ("it", i, None), (" ", n, None),
                 ("gone", st, None), (" ", n, None), ("x*y*", c, None), (" end", n, None)]
        );
        // Nested: bold holding italic.
        assert_eq!(texts(&inline("*very _much_*")), vec![("very ", b, None), ("much", Style { italic: true, ..b }, None)]);
        // Not markup: a snake_case word, a lone star, 2*3*4.
        assert_eq!(texts(&inline("snake_case_name 2*3*4 * x")), vec![("snake_case_name 2*3*4 * x", n, None)]);
        assert_eq!(
            texts(&inline("see <https://x.test/a?b=1&amp;c=2|the doc> or https://y.test/p.")),
            vec![("see ", n, None), ("the doc", n, Some("https://x.test/a?b=1&c=2")), (" or ", n, None),
                 ("https://y.test/p", n, Some("https://y.test/p")), (".", n, None)]
        );
        assert_eq!(
            texts(&inline("<@U123|Priya> in <#C9|issues> ping <@U777> <#C8> <!here>")),
            vec![("@Priya", n, None), (" in ", n, None), ("#issues", n, None), (" ping ", n, None),
                 ("@someone", q, None), (" ", n, None), ("#channel", q, None), (" ", n, None), ("@here", n, None)]
        );
        assert_eq!(texts(&inline("a &lt;b&gt; &amp; c")), vec![("a <b> & c", n, None)]);
    }

    #[test]
    fn links_of_every_kind() {
        let n = Style::default();
        let link = |src: &str| -> Vec<(String, Option<String>)> {
            inline(src).into_iter().map(|s| (s.text, s.link)).collect()
        };
        for (src, target) in [
            ("figma://file/aR7x", "figma://file/aR7x"),
            ("slack://channel?team=T1&id=C2", "slack://channel?team=T1&id=C2"),
            ("file:///Users/a/spec%20v2.pdf", "file:///Users/a/spec%20v2.pdf"),
            ("~/Desktop/spec.pdf", "~/Desktop/spec.pdf"),
            ("/Users/a/notes.md", "/Users/a/notes.md"),
            ("mailto:dhaval@airtribe.live", "mailto:dhaval@airtribe.live"),
            ("tel:+919800000000", "tel:+919800000000"),
            ("dhaval@airtribe.live", "mailto:dhaval@airtribe.live"),
            ("www.notion.so/airtribe/Spec", "https://www.notion.so/airtribe/Spec"),
        ] {
            assert_eq!(link(&format!("see {src}.")), vec![
                ("see ".to_owned(), None),
                (src.to_owned(), Some(target.to_owned())),
                (".".to_owned(), None),
            ], "{src}");
        }
        assert_eq!(texts(&inline("(https://x.test/a)")), vec![("(", n, None), ("https://x.test/a", n, Some("https://x.test/a")), (")", n, None)]);
        for prose in ["GET /cart/totals", "std::fs::read", "localhost:8091", "a@b", "x/~/y", "www.", "http://", "vscode://file/x.rs", "shortcuts://run-shortcut?name=x"] {
            assert!(inline(prose).iter().all(|s| s.link.is_none()), "{prose}");
        }
    }

    #[test]
    fn local_targets_resolve_and_scripts_are_revealed_not_run() {
        assert_eq!(local_path("file:///Users/a/spec%20v2.pdf"), Some("/Users/a/spec v2.pdf".into()));
        assert_eq!(local_path("file://localhost/tmp/x"), Some("/tmp/x".into()));
        assert_eq!(local_path("FILE:///tmp/x"), Some("/tmp/x".into()), "the scheme is any case");
        assert_eq!(local_path("file:/tmp/x"), Some("/tmp/x".into()), "and one slash is still a file");
        assert_eq!(local_path("/Users/a/b"), Some("/Users/a/b".into()));
        assert!(local_path("~/Desktop").is_some_and(|p| p.ends_with("Desktop") && p.is_absolute()));
        assert_eq!(local_path("figma://file/x"), None);
        assert_eq!(local_path("mailto:a@b.co"), None);

        let dir = std::env::temp_dir().join(format!("loop-open-{}", std::process::id()));
        let app = dir.join("Thing.app");
        std::fs::create_dir_all(app.join("Contents")).unwrap();
        let doc = dir.join("spec.pdf");
        std::fs::write(&doc, "%PDF").unwrap();
        let script = dir.join("payload");
        std::fs::write(&script, "#!/bin/sh\n").unwrap();
        let disguised = dir.join("report.pdf");
        let _ = std::fs::remove_file(&disguised);
        std::os::unix::fs::symlink(&app, &disguised).unwrap();
        let real = |p: &std::path::Path| std::fs::canonicalize(p).unwrap().display().to_string();

        assert_eq!(plan(&doc.display().to_string()), Plan::Open(real(&doc)), "a document opens");
        assert_eq!(plan(&dir.display().to_string()), Plan::Open(real(&dir)), "a plain folder opens");
        for hostile in [
            app.display().to_string(),
            format!("{}/Contents/..", app.display()),
            format!("file://{}/Contents/%2E%2E", app.display()),
            format!("FILE://{}", app.display()),
            disguised.display().to_string(),
        ] {
            assert_eq!(plan(&hostile), Plan::Reveal(real(&app)), "{hostile} is shown in Finder, not run");
        }
        assert_eq!(plan(&script.display().to_string()), Plan::Reveal(real(&script)), "no document type, no run");
        assert!(matches!(plan("/no/such/file.pdf"), Plan::Missing(_)));

        assert_eq!(plan("HTTPS://example.com"), Plan::Web);
        assert_eq!(plan("mailto:a@b.co"), Plan::Open("mailto:a@b.co".into()));
        assert_eq!(plan("figma://file/x"), Plan::Open("figma://file/x".into()));
        for refused in ["shortcuts://run-shortcut?name=x", "x-man-page://ls", "smb://host/share", "-aCalculator", "vscode://ext"] {
            assert_eq!(plan(refused), Plan::Refused, "{refused}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn blocks_keep_breaks_bullets_and_fences() {
        let src = "First line\nsecond line\n\nNew para\n\u{2022} one\n- two\n```\nlet x = 1;\n```\n&gt; quoted";
        let blocks = parse(src);
        assert_eq!(blocks.len(), 7, "{blocks:?}");
        assert!(matches!(&blocks[0], Block::Para(s) if s.iter().map(|s| s.text.as_str()).collect::<String>() == "First line\nsecond line"));
        assert_eq!(blocks[1], Block::Gap, "a blank line is a paragraph break, not a line break");
        assert!(matches!(&blocks[2], Block::Para(s) if s[0].text == "New para"));
        assert!(matches!(&blocks[3], Block::Bullet(s) if s[0].text == "one"));
        assert!(matches!(&blocks[4], Block::Bullet(s) if s[0].text == "two"));
        assert_eq!(blocks[5], Block::Code("let x = 1;".into()));
        assert!(matches!(&blocks[6], Block::Quote(s) if s[0].text == "quoted"));
        assert!(!parse("\n\nlead\n\n").contains(&Block::Gap), "no gap before the first block or after the last");
        assert_eq!(parse("```one line```"), vec![Block::Code("one line".into())]);
        assert_eq!(plain("*Checkout* fails for <@U1|Priya>"), "Checkout fails for @Priya");
    }
}
