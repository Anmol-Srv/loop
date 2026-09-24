# Loop — product context

## Register
product

## Platform
Native macOS app (Rust, egui), not web. The design language is Loop's own dark
product system in `src/desktop/design/`; no HIG/Material rulebook applies.

## Users
Airtribe's in-house engineering team — five people across backend, frontend
and design (plus a manager). Each may run a personal AI agent (Hermes, Claude
Code, Codex) on their own Mac. They open Loop through the day to see what the
team is doing, move their own tasks, and hand work to their agent.

## Purpose
A Linear-like tracker built in-house: projects, tasks on per-department tracks
(eng: open → in progress → completed → shipped; design: … → handoff →
completed), evidence (PRs, commits, Figma) and full visibility of who is doing
what. Agents are first-class: a person hands a task to their agent, the agent
works locally and reports back, the owner reviews.

## Positioning
The team's tracker where people and their agents work side by side, with the
same visibility for both.

## Personality
Calm, precise, alive. Quiet surfaces and exact information; motion only where
work is actually happening. References: Linear (agent sessions, density,
restraint), Raycast (speed, crispness).

## Anti-references
- Chatbot UI: no chat bubbles or messenger layouts for agent activity.
- Neon "AI" glow: no purple gradients, sparkles, glowing orbs, "magic" motifs.
- Cropped or clipped content passed off as a fix; visible scrollbars; blue
  selection borders (removed app-wide).

## Visibility principles
- Team-visible: that a task is with an agent, whose, its state and "now" line,
  progress updates, the submission report, evidence, reviews, agents at work.
- Owner-only (plus admins where noted): the agent's step log, private
  instructions to the agent, questions and answers between owner and agent,
  agent management (tokens, rotate, revoke, connection health).
- Enforced by the server, never only hidden in the UI.

## Accessibility
Body text ≥ 4.5:1 on its surface; stat numbers in white. Every animation has a
reduced-motion path (`AIRTRIBE_REDUCE_MOTION`). Nunito lacks arrows, ticks and
boxes — those glyphs are painted, never drawn as text.
