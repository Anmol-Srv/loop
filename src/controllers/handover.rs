//! A hand-over by hand: the context a hand-off gives an agent, written out
//! as a prompt a person pastes into their own Claude session.

use serde_json::Value;
use uuid::Uuid;

use crate::controllers::{agent, task};
use crate::db::AppState;
use crate::errors::{AppError, AppResult};

/// The prompt for `task_id`, for its assignee only: it carries the source
/// unmasked (a DM's text included), and the repo folders are the viewer's own.
pub async fn prompt(state: &AppState, viewer: Option<Uuid>, task_id: Uuid, server: &str) -> AppResult<String> {
    let me = viewer.ok_or_else(|| AppError::Forbidden("sign in to copy a hand-over".into()))?;
    let row = task::get(state, task_id, Some(me)).await?;
    if row.task.assignee_person_id != Some(me) {
        return Err(AppError::Forbidden(
            "Only the task's assignee can copy its hand-over \u{2014} assign it to yourself first.".into(),
        ));
    }
    let ctx = agent::context_for(state, row, me, server, true).await?;
    Ok(render(&ctx, server))
}

fn s<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

fn list(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}

fn render(ctx: &Value, server: &str) -> String {
    let t = &ctx["task"];
    let id = s(t, "id").unwrap_or_default();
    let mut out = String::new();
    let mut add = |line: String| {
        out.push_str(&line);
        out.push('\n');
    };

    add(format!(
        "You're picking up Loop task \"{}\" (id {id}), handed over by hand from the Loop app. \
         Read all of this before touching anything, then propose a short plan and wait for my go-ahead.",
        s(t, "title").unwrap_or("Untitled")
    ));
    add(String::new());

    add("## Task".into());
    let mut facts = vec![
        format!("Status: {}", s(t, "status").unwrap_or("open").replace('_', " ")),
        format!("Priority: P{}", t["priority"].as_i64().unwrap_or(2)),
    ];
    if let Some(c) = s(t, "category") {
        facts.push(format!("Category: {c}"));
    }
    if let Some(track) = s(ctx, "track") {
        let ends = match track {
            "design" => "finishes with a design hand-off",
            "eng" => "finishes with a PR that ships",
            _ => "no code change: finishes at completed, with a summary",
        };
        facts.push(format!("Track: {track} ({ends})"));
    }
    add(facts.join(" · "));
    add(String::new());
    add(s(t, "body").unwrap_or("(no description)").to_owned());
    add(String::new());

    if let Some(brief) = s(ctx, "brief") {
        add("## Owner's note".into());
        add(brief.to_owned());
        add(String::new());
    }
    if let Some(plan) = ctx.get("plan").filter(|p| p.is_object()) {
        add(format!("## Current plan ({})", s(plan, "decision").unwrap_or("not yet decided")));
        if let Some(sum) = s(plan, "summary") {
            add(sum.to_owned());
        }
        if let Some(body) = s(plan, "plan") {
            add(body.to_owned());
        }
        add(String::new());
    }

    // Where to work.
    add("## Where to work".into());
    let project = &ctx["project"];
    let repos = list(&project["repos"]);
    if repos.is_empty() {
        match ctx.get("folder").filter(|f| f.is_object()) {
            Some(f) => add(format!("- Folder: {} ({})", s(f, "path").unwrap_or_default(), s(f, "name").unwrap_or_default())),
            None => add("- No repo or folder is set for this task: ask me where to work.".into()),
        }
    }
    for r in repos {
        let place = s(r, "localPath").map_or_else(|| "not cloned on this Mac".to_owned(), str::to_owned);
        add(format!("- {} at {place} ({})", s(r, "name").unwrap_or("repo"), s(r, "url").unwrap_or_default()));
    }
    add(String::new());

    if project.is_object() {
        add(format!("## Project: {}", s(project, "name").unwrap_or("Untitled")));
        if let Some(d) = s(project, "description") {
            add(d.to_owned());
        }
        for r in list(&project["resources"]) {
            add(format!("- {}: {} {}", s(r, "kind").unwrap_or("link"), s(r, "title").unwrap_or_default(), s(r, "url").unwrap_or_default()));
        }
        add(String::new());
    }

    if let Some(src) = ctx.get("source").filter(|v| v.is_object()) {
        add("## Where it came from".into());
        add(format!(
            "{} {} \u{00B7} {} \u{00B7} {}",
            s(src, "kind").unwrap_or("message"),
            s(src, "channelName").unwrap_or_default(),
            s(src, "author").unwrap_or_default(),
            s(src, "receivedAt").unwrap_or_default()
        ));
        if let Some(url) = s(src, "url") {
            add(url.to_owned());
        }
        for m in list(&src["thread"]) {
            add(format!("> {}: {}", s(m, "author").unwrap_or_default(), s(m, "text").unwrap_or_default().replace('\n', "\n> ")));
        }
        if let Some(text) = s(src, "text") {
            add(format!("> {}", text.replace('\n', "\n> ")));
        }
        for f in list(&src["files"]) {
            add(format!("- File: {} ({}) {}", s(f, "name").unwrap_or_default(), s(f, "mime").unwrap_or_default(), s(f, "url").unwrap_or_default()));
        }
        add(String::new());
    }

    let files = list(&ctx["files"]);
    if !files.is_empty() {
        add("## Files attached to the task".into());
        for f in files {
            add(format!("- {} ({}) {}", s(f, "name").unwrap_or_default(), s(f, "mime").unwrap_or_default(), s(f, "url").unwrap_or_default()));
        }
        add(String::new());
    }

    let notes = list(&ctx["notes"]);
    if !notes.is_empty() {
        add("## Notes so far".into());
        for n in notes {
            let who = s(n, "authorName").unwrap_or("someone");
            add(format!("- {who} ({}): {}", s(n, "kind").unwrap_or("note"), s(n, "body").unwrap_or_default().replace('\n', " ")));
        }
        add(String::new());
    }

    if let Some(arts) = ctx["artifacts"].as_object().filter(|a| !a.is_empty()) {
        add("## Attached links".into());
        for (kind, items) in arts {
            for a in list(items) {
                add(format!("- {kind}: {} {}", s(a, "title").unwrap_or_default(), s(a, "url").unwrap_or_default()));
            }
        }
        add(String::new());
    }

    for (key, heading) in [("blockedBy", "Blocked by"), ("blocks", "Blocks")] {
        let items = list(&ctx["related"][key]);
        if !items.is_empty() {
            add(format!("## {heading}"));
            for r in items {
                add(format!("- {} ({}, {})", s(r, "title").unwrap_or_default(), s(r, "status").unwrap_or_default(), s(r, "assigneeName").unwrap_or("unassigned")));
            }
            add(String::new());
        }
    }

    add("## Keeping Loop updated".into());
    let next: Vec<&str> = list(&ctx["allowedNext"]).iter().filter_map(Value::as_str).collect();
    add(format!(
        "This is my task, not an agent's: post progress notes, attach the PR and move its status yourself at milestones \
         (statuses it can move to now: {}). Use my Loop sign-in from the desktop app:",
        if next.is_empty() { "none".to_owned() } else { next.join(", ") }
    ));
    add("```bash".into());
    add("read -r U T < <(python3 -c \"import json,os;w=[x for x in json.load(open(os.path.expanduser('~/Library/Application Support/airtribe-control-plane/workspaces.json'))) if not x['private']][0];print(w['server'],w['token'])\")".into());
    add(format!("ID={id}"));
    add("curl -fsS -X POST --oauth2-bearer \"$T\" -H 'content-type: application/json' -d '{\"body\":\"…\"}' \"$U/api/user/tasks/$ID/notes\"".into());
    add("curl -fsS -X POST --oauth2-bearer \"$T\" -H 'content-type: application/json' -d \"{\\\"parentType\\\":\\\"task\\\",\\\"parentId\\\":\\\"$ID\\\",\\\"kind\\\":\\\"pr\\\",\\\"url\\\":\\\"<pr url>\\\",\\\"title\\\":\\\"<title>\\\"}\" \"$U/api/user/artifacts\"".into());
    add(format!(
        "curl -fsS -X PATCH --oauth2-bearer \"$T\" -H 'content-type: application/json' -d '{{\"status\":\"<next>\",\"expectedStatus\":\"{}\"}}' \"$U/api/user/tasks/$ID\"",
        s(t, "status").unwrap_or("open")
    ));
    add("# a file listed above: curl -fsS --oauth2-bearer \"$T\" \"<its url>\" -o <name>".into());
    add("```".into());
    add(format!("Never print the token or put it in a commit. Server: {server}"));
    out
}
