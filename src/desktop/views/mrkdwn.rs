//! Slack's mrkdwn, drawn: `*bold*`, `_italic_`, `~strike~`, `` `code` ``,
//! fenced blocks, `<url|label>` and bare links (clickable), mentions,
//! bullets and line breaks. The agent sends names already resolved; a
//! leftover `<@U…>` reads as a quiet "@someone" rather than an id.
//!
//! A paragraph is one label — one accessible name, the plain text — and a
//! click on it is hit-tested against the link spans it holds.

use egui::text::{LayoutJob, TextFormat};
use egui::{FontFamily, FontId, RichText, Sense, Stroke};

use crate::desktop::design::{colour, radius, space, text, theme};

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
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

/// The blocks of a message.
pub fn parse(src: &str) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    let mut lines = src.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
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
        if trimmed.is_empty() {
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
            'h' if prev.is_none_or(char::is_whitespace) && (s[at..].starts_with("https://") || s[at..].starts_with("http://")) => {
                let len = s[at..].find(char::is_whitespace).unwrap_or(s.len() - at);
                let url = s[at..at + len].trim_end_matches(['.', ',', ')', '!', '?', ';', ':']);
                flush(&mut plain, out);
                out.push(Span { text: url.to_owned(), style, link: Some(unescape(url)) });
                i = chars.partition_point(|(b, _)| *b < at + url.len());
                continue;
            }
            _ => {}
        }
        plain.push(c);
        i += 1;
    }
    flush(&mut plain, out);
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
            Span { text: shown, style, link: Some(url) }
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
        })
        .collect::<Vec<String>>()
        .join(" ")
}

// ------------------------------------------------------------------ drawing

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
            _ if s.link.is_some() => colour::ACCENT,
            _ if s.style.quiet => colour::TEXT_MUTED,
            _ if s.style.bold => colour::TEXT,
            _ => ink,
        };
        let format = TextFormat {
            font_id: FontId::new(size, family),
            color: colour,
            italics: s.style.italic,
            background: if s.style.code { colour::INSET } else { egui::Color32::TRANSPARENT },
            strikethrough: if s.style.strike { Stroke::new(1.0, colour) } else { Stroke::NONE },
            underline: if s.link.is_some() { Stroke::new(1.0, colour::ACCENT.gamma_multiply(0.5)) } else { Stroke::NONE },
            line_height: Some(text::BODY * 1.45),
            ..Default::default()
        };
        let start = job.text.len();
        job.append(&s.text, 0.0, format);
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
    let (pos, galley, response) = egui::Label::new(job).sense(sense).selectable(false).layout_in_ui(ui);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, galley.text()));
    let under = |p: egui::Pos2| {
        let at = galley.cursor_from_pos(p - pos).index.0;
        // A char index into the text; the ranges are byte offsets.
        let byte = galley.text().char_indices().nth(at).map_or(galley.text().len(), |(b, _)| b);
        links.iter().find(|(r, _)| r.contains(&byte)).map(|(_, u)| u.clone())
    };
    if let Some(url) = response.hover_pos().and_then(under) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        if response.clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        }
    }
    ui.painter().galley(pos, galley, ink);
}

/// A message, drawn in `ink`.
pub fn show(ui: &mut egui::Ui, src: &str, ink: egui::Color32) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = space::XS;
        for block in parse(src) {
            match block {
                Block::Para(s) => paragraph(ui, &s, ink),
                Block::Quote(s) => {
                    ui.horizontal_top(|ui| {
                        ui.add_space(space::MD);
                        ui.vertical(|ui| paragraph(ui, &s, colour::TEXT_MUTED));
                    });
                }
                Block::Bullet(s) => {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = space::SM;
                        // A painted dot: the bullet sits on the first line's middle.
                        let (r, _) = ui.allocate_exact_size(egui::vec2(space::SM, text::BODY * 1.45), Sense::hover());
                        ui.painter().circle_filled(egui::pos2(r.center().x, r.center().y), 2.0, colour::TEXT_MUTED);
                        ui.vertical(|ui| paragraph(ui, &s, ink));
                    });
                }
                Block::Code(code) => {
                    egui::Frame::new()
                        .fill(colour::INSET)
                        .stroke(Stroke::new(1.0, colour::LINE_SOFT))
                        .corner_radius(radius::SM)
                        .inner_margin(egui::Margin::symmetric(space::MD as i8, space::SM as i8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new(code).monospace().size(text::SMALL).color(colour::TEXT_2));
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
    fn blocks_keep_breaks_bullets_and_fences() {
        let src = "First line\nsecond line\n\nNew para\n\u{2022} one\n- two\n```\nlet x = 1;\n```\n&gt; quoted";
        let blocks = parse(src);
        assert_eq!(blocks.len(), 6, "{blocks:?}");
        assert!(matches!(&blocks[0], Block::Para(s) if s.iter().map(|s| s.text.as_str()).collect::<String>() == "First line\nsecond line"));
        assert!(matches!(&blocks[1], Block::Para(s) if s[0].text == "New para"));
        assert!(matches!(&blocks[2], Block::Bullet(s) if s[0].text == "one"));
        assert!(matches!(&blocks[3], Block::Bullet(s) if s[0].text == "two"));
        assert_eq!(blocks[4], Block::Code("let x = 1;".into()));
        assert!(matches!(&blocks[5], Block::Quote(s) if s[0].text == "quoted"));
        assert_eq!(parse("```one line```"), vec![Block::Code("one line".into())]);
        assert_eq!(plain("*Checkout* fails for <@U1|Priya>"), "Checkout fails for @Priya");
    }
}
