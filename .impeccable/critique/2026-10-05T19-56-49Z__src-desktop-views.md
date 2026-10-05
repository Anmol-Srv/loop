---
target: Loop desktop app UI (src/desktop/views)
total_score: 24
max_score: 40
na_heuristics: 
p0_count: 0
p1_count: 4
timestamp: 2026-10-05T19-56-49Z
slug: src-desktop-views
---
Method: dual-agent (A: design-review agent, B: detector + native-macOS code-audit agent), plus a research agent for online issues and libraries.

## Design health (24/40, Acceptable)

| # | Heuristic | Score | Key issue |
|---|---|---|---|
| 1 | Visibility of system status | 3 | No cue for content below the fold; "Approved." appears at page top, far from the card (task.rs:920) |
| 2 | Match system / real world | 3 | Raw host 127.0.0.1:55007 shown twice in sidebar (chrome.rs:99, :310) |
| 3 | User control and freedom | 2 | No undo for Approve/Take back/Hand off/status moves; Esc on New task drops the draft (new_task.rs:166); Esc in a Settings dialog closes all of Settings (settings.rs:205) |
| 4 | Consistency and standards | 2 | No Edit/Window menu; Cmd+W/Cmd+M dead in main window; two primaries on review; Hand off differs between menu and task page |
| 5 | Error prevention | 2 | One-click Take back mid-work; "No evidence attached" in faint text (agent_session.rs:1163) |
| 6 | Recognition over recall | 3 | Collapsed sidebar: My Tasks / All Tasks icons nearly identical (chrome.rs:67) |
| 7 | Flexibility and efficiency | 2 | No arrows or j/k in lists, palette can't run actions, no multi-select |
| 8 | Aesthetic and minimalist | 3 | Three text boxes visible on review |
| 9 | Error recovery | 2 | "Use Refresh in the sidebar" points at a removed button; w::error has no Retry |
| 10 | Help and documentation | 2 | No shortcut reference |

## Design specificity
Agent layer is specific to Loop (per-agent globes, Handed off / Working / Needs you / Done stepper, lock captions, plan-before-build promise). Everything else reads as a generic Linear clone, and Home opens on team charts (home.rs:205-223) rather than "Waiting on you" as in docs/design-mocks/home.png. detect.mjs scanned 0 files (.rs is not supported); a 21-item code audit of native macOS behaviors replaced it.

## Priority issues
- [P1] Selecting and copying text: drag past the edge doesn't scroll, and trackpad scrolling is blocked while the mouse button is held (egui scroll_area.rs:1215); agent text can't be selected (mrkdwn.rs:284 selectable(false)); table cells can't be selected; selecting a short ID copies only 8 chars (widgets.rs:63). Fix: ~15 lines in shell::content, scroll_with_delta while the pointer is outside the area + request_repaint; make mrkdwn selectable. /impeccable harden
- [P1] Forms invisible to macOS: label never linked to the field (widgets.rs:578), password fields unnamed (login.rs:266,273), AccessKit set-value ignored, no Secure Input, no right-click menu, dictation/emoji insert nothing (winit #3617/#4666). AutoFill needs real AppKit text fields with a content type + Developer ID signing + a fixed domain. Fix tiers: (a) labelled_by, set-value handling, Secure Input on/off, context menu; (b) real macOS text fields on login (~200-300 lines objc2); (c) signing + domain. /impeccable harden
- [P1] No Mac menu bar: winit default menu only; Cmd+W/M, Ctrl+Cmd+F dead in main window; no Edit menu means no emoji picker or dictation entry; titles likely read "acp-app" (check). Fix: muda 0.21, switch off winit's menu, pass Edit items to egui via raw_input_hook (egui #3411). /impeccable adapt
- [P1] "Your agent needs you" easy to miss: same amber as In progress/P1 (home.rs:1018, tokens.rs:431); no My Tasks badge; pinned card ~45% down the page (task.rs:536-571); Complete next to Approve skips review (task.rs:797); menu Hand off skips the brief dialog (menus.rs:383); Take back instant. /impeccable layout
- [P2] Scrolling, contrast, system settings: hidden scrollbar still leaves an invisible 10pt clickable strip (theme.rs:259-273); "Show scroll bars: Always" ignored; Reduce Motion only via env var (theme.rs:249); Increase Contrast and Reduce Transparency ignored; placeholder grey 4.27:1 on SURFACE (needs 4.5). Fix: show bar only while scrolling or hovering; read macOS settings via objc2-app-kit; grey to #858B94 dark / #5F6670 light. /impeccable audit

## Everything else
App-level quick fixes: Settings Esc/Cmd+W checked too early (settings.rs:205-210); Cmd+0 resets to 1.0 not 1.15 (mod.rs:45); window size/position not remembered; pasting a screenshot does nothing (arboard already in tree); no Devanagari/CJK font fallback (theme.rs:26-30); toasts not announced, confirm dialogs don't take focus; rows/palette rows/cards unnamed for VoiceOver; two buttons both named "Copy" (agents.rs:680/729); context menus right-click only (viz.rs:915); no autoscroll when dragging a board card (mytasks.rs:779); UI redraws 30x/sec while a working agent globe is visible, even unfocused (design/agent.rs:286); egui-notify unused; pointing-hand cursor on 48 buttons; URLs in descriptions not clickable (task.rs:1087).
Needs objc2: dragging the window by the top strip + double-click zoom (check by hand); native notifications + Dock badge; spell check via NSSpellChecker.
Only fixable in egui/winit: dictation/emoji (winit #3617), word-drag selection (egui #2550), rubber-band/nested scroll (egui #5422), color emoji (egui #2551), drag attachments out (winit 0.31), VoiceOver unanswered while minimized (egui #8671).

## Libraries
Adopt: muda (menu bar), objc2-app-kit as a direct dependency (settings, window position, Secure Input, Dock badge, native fields), arboard as a direct dependency (image paste), objc2-user-notifications (once signed). Already on: accesskit through eframe. Delete: egui-notify. Later: keyring (after Developer ID signing). Skip: egui_spellcheck (too new), dark-light, egui_commonmark, egui_dnd, window-vibrancy. No crate for drag-select autoscroll or real text fields inside egui; both are DIY. Switching frameworks (Tauri/Dioxus/Slint/Iced/GPUI) doesn't fix it cheaply; recommendation: stay on egui.

## Personas
Power user: no arrows/j-k in tables (table.rs:337), palette can't run actions, no bulk select, Esc dead on task page, Cmd+R reloads the whole app. VoiceOver user: unnamed password fields/rows/palette results, toasts not announced, status needs right-click or drag, Reduce Motion ignored. Engineer next to the editor: every launch opens on team charts (mod.rs:49), icon-only sidebar under 960pt with a Triage row that appears and disappears, C on a project page creates a task with no project (new_task.rs:52).

## Minor
Type-scale doc says six sizes, code has eight (tokens.rs:323-336); primary pressed = resting (widgets.rs:406); disabled secondary loses its outline; "Agent working" chip blue vs amber row dot; empty Team load card has no empty-state text; Sign out has no confirm.

## Questions
Who is Home for? Why is "needs you" the same amber as in-progress? What breaks if Complete is hidden on delegated tasks?
