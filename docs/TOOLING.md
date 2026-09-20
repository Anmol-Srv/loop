# Tooling

What to add to this repo, what to leave alone, and why. Written 2026-09-20
against egui 0.36.2, axum 0.8.9, sqlx 0.8.

Every version and compatibility claim below was checked on this machine — either
with `cargo add --dry-run`, by reading the crates.io index cache under
`~/.cargo/registry/index/.../.cache`, or by actually compiling the crates
together in a throwaway crate. The method is named where it matters.

---

## Adopt now

1. **`egui_extras` `datepicker` feature** — a date picker with no new dependency. Flip one feature flag.
2. **`egui_commonmark` 0.25** — renders task bodies and MCP tool docs as markdown instead of a wall of `Label`.
3. **`tower-http` 0.7.1** (`trace`, `timeout`, `request-id`, `catch-panic`, `limit`) — one dependency, five real gaps closed, ~15 lines in `app.rs`.
4. **`tracing_subscriber` JSON output when not a TTY** — zero new dependencies, makes logs greppable on a host.
5. **`just`** — replaces the five `scripts/*.sh` invocations and the long `cargo run --features app --bin acp-app` you type forty times a day.
6. **`sqlx-cli`** — `sqlx migrate add` instead of hand-naming timestamped files.
7. **Fix the Dockerfile.** It `COPY`s `templates` and `static`, neither of which exists in the repo. `docker build` cannot currently succeed. This is a deploy blocker, not a tooling nicety.
8. **Tailscale in front of the server** — deletes the rate-limiting question, the public-TLS question, and most of the threat model, for a 6-person internal tool.

Everything else in this document is LATER or NO.

---

## 1. egui ecosystem

### The surprise: nothing lags

The brief assumed egui crates trail a version or two. As of today they do not.
I read the index cache for eleven candidate crates and every single one's latest
release targets `egui ^0.36`:

| Crate | Latest | Requires |
|---|---|---|
| `egui_plot` | 0.37.0 | egui ^0.36.0 |
| `egui_table` | 0.10.0 | egui ^0.36.0 |
| `egui_commonmark` | 0.25.0 | egui ^0.36.0 |
| `egui_virtual_list` | 0.12.0 | egui ^0.36.0 |
| `egui_flex` | 0.8.0 | egui ^0.36.0 |
| `egui_taffy` | 0.14.0 | egui ^0.36.0 |
| `egui_inbox` | 0.13.0 | egui ^0.36.0 |
| `egui_infinite_scroll` | 0.12.0 | egui ^0.36.0 |
| `egui_json_tree` | 0.17.0 | egui ^0.36 |
| `egui_dock` | 0.21.1 | egui ^0.36 |
| `egui_tiles` | 0.17.1 | egui ^0.36.0 |

Watch out for `egui_plot`: its own version number runs one ahead of egui's, so
`egui_plot 0.37` is the egui-**0.36** release. `cargo add egui_plot` picks the
right thing; the number just looks wrong.

**How I verified compatibility, not just the metadata:** I built a scratch crate
at `/tmp/eguiprobe` pinning `egui = "=0.36.2"` alongside `egui_extras`
(datepicker), `egui_plot 0.37`, `egui_table 0.10`, `egui_commonmark 0.25`,
`egui_virtual_list 0.12` and `egui-phosphor 0.14`. `cargo generate-lockfile`
resolved **one** `egui` in the lock — 0.36.2, no duplicate — and `cargo check`
compiled all of them clean in 15s. So the whole shortlist genuinely co-exists.

The two crates already in the tree are also current: `egui-notify 0.23` and
`egui-phosphor 0.14` both require egui ^0.36. (`egui-notify` skipped 0.35 —
there is no egui-0.35 release of it — so it is the one to watch when egui 0.37
lands.)

### YES — `egui_extras` `datepicker` feature

**What it enables:** `DatePickerButton`. Due dates on tasks.
**Cost:** none worth counting. `egui_extras` is already a dependency; add
`"datepicker"` to its feature list. Verified compiling in the probe crate.
**Verdict: YES.** Do not reach for a date-picker crate. Rung 5 of the ladder —
the installed dependency already does it.

### YES — `egui_commonmark` 0.25

**What it enables:** `task.body` and the `acp://docs/<tool>` MCP docs render as
markdown — headings, code blocks, links — instead of raw text. Every task body a
human or an agent writes will be markdown whether you support it or not.
**Cost:** pulls `pulldown-cmark`. Meaningful but modest; it compiled in ~2s in
the probe. Wants `egui_extras` and `eframe` at matching versions, which you have.
Add `"syntect"` to `egui_extras` only if you want highlighted code blocks —
that one is a heavy transitive dep, so skip it until someone complains.
**Verdict: YES**, gated behind the `app` feature like the rest of the GUI stack.

### LATER — `egui_json_tree` 0.17

**What it enables:** a collapsible viewer for `change.patch`. The task history
pane currently has to show a jsonb blob and there is exactly one good way to do
that. Small crate, no heavy deps.
**Cost:** one dependency for one screen.
**Verdict: LATER.** Do it the day someone squints at a patch blob in the history
pane. Not before.

### LATER — `egui_virtual_list` 0.12

**What it enables:** virtualisation for variable-height rows. Board cards are
variable height, so egui's built-in `ScrollArea::show_rows` (which assumes
uniform heights) does not apply to them directly.
**Cost:** a dependency and a restructure of the board's render loop.
**Verdict: LATER, with a trigger.** egui re-lays-out every frame, so a board with
a few hundred cards is fine on an M-series Mac. Add this when a real board drops
below 60fps, measured — not on principle. Until then, if a list gets long, use
`ScrollArea::show_rows` with a fixed row height; it is free and in egui already.

### NO — `egui_plot` 0.37

The design spec's non-goals say "no burndown charts". A task tracker for six
people has no chart that earns a plotting library. If you later want a sparkline
of tasks-closed-per-day, that is ~15 lines of `ui.painter().add(Shape::line(..))`
against the tokens you already have, and it will look more like the rest of the
app than a plot widget will.
**Verdict: NO.** Revisit only if someone asks for interactive, zoomable charts.

### NO — `egui_table` 0.10

`egui_extras::TableBuilder` is already in the tree and does striped, resizable,
scrollable tables. `egui_table` buys sticky column/row headers and cell-level
virtualisation for tables in the tens of thousands of rows. You do not have one.
**Verdict: NO.**

### NO — `egui_taffy` / `egui_flex`

Flexbox for egui. Attractive if you think in CSS. The cost is a second layout
engine running inside a codebase that already has a coherent 4pt spacing scale in
`design/tokens.rs` and views written against egui's own layout. Mixing two layout
models is how a design system stops being one.
**Verdict: NO.**

### NO — `egui_dock` / `egui_tiles`

Dockable, rearrangeable panels. The app has a sidebar, a board, an inbox and a
task detail. There is nothing to dock. This is the single most seductive
over-build available in the egui ecosystem.
**Verdict: NO.**

### NO — `egui_inbox`, `egui_infinite_scroll`, `egui_material_icons`

`egui_inbox` solves async-result-into-UI — `src/desktop/net.rs` already solves it
with a channel and `pump()`. `egui_infinite_scroll` is paging you do not have.
`egui_material_icons` is a second icon set next to `egui-phosphor`.
**Verdict: NO on all three.**

---

## 2. Server-side ergonomics

### YES — `tower-http` 0.7.1

Verified: built `axum 0.8.9` + `tower-http 0.7.1` with features
`trace,timeout,request-id,catch-panic,limit,cors` in `/tmp/srvprobe`; clean
compile, single `tower-http` in the lock.

Turn on exactly these four, in `app.rs`:

- **`TraceLayer`** — per-request method, path, status and latency. This is the
  single highest-value line in this document. Right now a request that 500s
  leaves one `tracing::error!` from `AppError` and no context about what was
  called.
- **`TimeoutLayer`** — a hung Postgres query currently hangs the connection
  forever. 30s.
- **`SetRequestIdLayer` + `PropagateRequestIdLayer`** — you have agents, a CLI
  and a Mac app all hitting the same endpoints. Correlating a report ("the claim
  failed") to a log line without a request id is guesswork.
- **`CatchPanicLayer`** — a panic in a handler currently kills the connection
  with no response and no log. Turns it into a 500 you can see.
- **`RequestBodyLimitLayer`** — cheap; `run_log_line` append is an unbounded text
  field written by agents.

**Cost:** one dependency, already in your tree transitively via axum's own
graph, ~15 lines of setup. **Verdict: YES.**

One footgun: `axum-extra` and `tower_governor` both still require `tower-http
^0.6`, so pulling either of them alongside `tower-http 0.7` gives you two copies
in the lock. Harmless, but if you ever add them, pin `tower-http = "0.6.11"`
instead.

### NO — compression and CORS

Compression: the clients are a native Rust app and a CLI, on a LAN or tailnet,
exchanging small JSON. Compression costs CPU per request to save bytes nobody is
paying for. CORS: there is no browser client and the README says there never will
be. **Verdict: NO to both**, and delete them from the feature list if they creep
in.

### NO — `garde` / `validator` / `axum-valid`

`garde 0.23` and `validator 0.21` are both current and both fine crates.
`axum-valid 0.25` wires them into extractors.

You do not need them. `AppError::BadRequest` already exists, is already used in
29 places, and the validation this API actually performs is "is this string
non-empty", "is this status one of five values" and "does this uuid exist" — the
third is a database check no derive macro can do. A derive-based validator
replaces a one-line guard with a dependency, an attribute DSL, and a custom
extractor you have to write anyway to keep the `{success, data}` envelope on
errors.

If the guards start repeating, write this and stop:

```rust
fn required(field: &str, v: &str) -> AppResult<String> { ... }
```

**Verdict: NO.** Revisit if a single request struct ever grows past ~8 validated
fields.

### NO — OpenAPI (`utoipa` 5.5 / `utoipa-axum` 0.2 / `aide` 0.15)

This is the recommendation the brief half-expected and it is the wrong one.

The argument for OpenAPI is "agents consume this API". They do not — agents
consume **MCP**, and the MCP surface already ships a hand-written companion
document per tool at `acp://docs/<tool>`, compiled into the binary. That is a
better artifact than a generated schema: it explains the claim-lease protocol,
which no OpenAPI document can.

The human clients are a CLI and a Mac app, both in this repo, both sharing types
with the server through `src/`. They get compile-time checking, which is
strictly stronger than a spec file.

So OpenAPI would generate a document with no reader, at the cost of annotating
every handler and every DTO, and it would rot the first time someone forgets an
attribute. **Verdict: NO.** If an outside consumer ever appears, revisit — and
even then, consider that `acp --help` and the MCP docs may still be the answer.

### LATER — rate limiting (`tower_governor` 0.8)

Verified current and axum-0.8 compatible. But: if the server sits behind
Tailscale (see §5) the caller set is six laptops and a handful of agent tokens
you minted yourself, and every one of them is already authenticated and
attributable. Rate limiting protects against an anonymous internet you will not
have.

The one real risk is an agent in a retry loop hammering `work_claim`. That is
better handled where it can be handled precisely — a claim lease already exists;
cap heartbeats server-side if it ever happens.
**Verdict: LATER**, and only if the server goes public.

---

## 3. Developer workflow

I measured before recommending, and the measurements contradict the brief's
premise that the loop is slow:

| Operation | Measured (warm, this machine) |
|---|---|
| `cargo check` (server) | 3.2s |
| `cargo build --bin acp-server` after touching `app.rs` | 1.4s |
| `cargo build --features app --bin acp-app` after touching a view | 2.6s |
| `cargo test` — whole suite, 20 binaries, 67 `sqlx::test` fns | **8.9s** |

The loop is not slow. It is **manual**: five shell scripts with different
invocations, plus `cargo run --features app --bin acp-app` typed from memory.
Fix the manual part, do not buy speed you already have.

### YES — `just`

**What it replaces:** remembering `--features app --bin acp-app`, remembering
that the rehearsal script needs a running server, and the README as the place
people look up commands.

```
just dev        # cargo run --bin acp-server
just app        # cargo run --features app --bin acp-app
just test       # cargo test
just rehearse   # scripts/rehearse-delegation.sh
just bundle     # scripts/bundle-mac.sh
```

**Cost:** one brew install, one ~20-line `justfile`, a five-minute syntax to
learn. It is a make-file that does not fight you about tabs.
**Verdict: YES.** This is the highest-leverage item in this section precisely
because it costs nothing.

### YES — `sqlx-cli`

**What it enables:** `sqlx migrate add <name>` generates the timestamped file
with the naming convention already in `migrations/`, so nobody hand-types a
timestamp and gets ordering wrong. Also `sqlx database reset` for a clean local
DB, and `cargo sqlx prepare` if you ever go to §3's macro option.
**Cost:** a `cargo install`, no change to the built artifact — migrations still
run through `sqlx::migrate!()` at startup.
**Verdict: YES.**

### LATER — `sqlx::query!` macros + `cargo sqlx prepare`

Worth naming because the design doc claims it and the code does not: I grepped
and there are **zero** uses of `query!`/`query_as!`/`query_scalar!` in `src/`.
Every query is runtime-checked. The spec's "compile-time checked queries" line
in §2 is aspirational, not descriptive — fix the doc or fix the code.

Switching buys you a typo in SQL failing `cargo build` instead of in production.
The cost is real: the macros need a live `DATABASE_URL` at compile time, or a
committed `.sqlx/` directory kept in sync via `cargo sqlx prepare` — and your
Docker build has neither today.
**Verdict: LATER.** Do it when a runtime SQL error actually bites. If you do,
`.sqlx/` committed + `SQLX_OFFLINE=true` in the Dockerfile is the path.

### NO — `cargo-nextest`

I wanted to recommend this and the numbers said no. The suite runs in 8.9s. The
wins nextest offers — per-test process isolation, parallel test *binaries*,
better output — are worth real money on a 5-minute suite. At 9 seconds they buy
you nothing, and `sqlx::test` already gives you the isolation (a throwaway
database per test) that is nextest's main correctness argument.
**Verdict: NO.** Reconsider above ~60s.

### NO — `bacon` / `cargo-watch`

A 1.4s rebuild does not need a file watcher; `just dev` and cmd-R is the same
latency with nothing running in the background. `cargo-watch` is also
effectively in maintenance mode with its author pointing people at `bacon`.
**Verdict: NO.** If you want one anyway, `bacon` is the live one — but it is
preference, not leverage.

### NO — `cargo-deny`

Licence and advisory auditing for a closed-source internal tool with a
six-person team and no compliance obligation. The output is a CI job that goes
red on a `RUSTSEC` advisory in a transitive dep you cannot fix that week.
**Verdict: NO.** `cargo audit` run by hand before a deploy covers the one case
you care about.

### NO — pre-commit hooks

`cargo fmt --check` and `clippy` in a hook means every commit waits on a build,
and the first time someone is mid-thought they will `--no-verify` and the hook is
dead. Put `fmt` and `clippy -D warnings` in CI where it blocks the merge instead
of the commit.
**Verdict: NO to hooks, YES to the same two commands in CI.**

### NO — `sccache`, `mold`/`lld`, `cranelift`

Build-time tooling for build times you do not have. See the table above.
**Verdict: NO.**

---

## 4. Observability

The minimum, in order, and note that the first two cost no new dependency at all.

### YES — structured logs, no new crate

`tracing_subscriber::fmt::init()` in `main.rs` gives pretty human output and
nothing machine-readable. You already have the `env-filter` feature enabled and
are not using it.

```rust
let json = !std::io::IsTerminal::is_terminal(&std::io::stdout());
let sub = tracing_subscriber::fmt()
    .with_env_filter(EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,acp_server=debug,sqlx=warn".into()));
if json { sub.json().init() } else { sub.init() }
```

Human output locally, JSON on the host, and `RUST_LOG` works. Roughly six lines.
**Verdict: YES.** Do this at the same time as `TraceLayer`; together they are the
whole of "we can debug this in production".

### YES — request ids in the span

Covered in §2. `TraceLayer` plus `PropagateRequestIdLayer` means a report from
the Mac app can carry an id that finds the exact server span. With three client
types this stops being optional quickly.

### LATER — `sentry` 0.49

Panic capture and error grouping with a `tower` layer, ~10 lines of setup, a free
tier that a six-person internal tool will never exhaust. The honest case against
it: the users are the developers, sitting nearby, and they will tell you. The
honest case for: `AppError::Internal` currently vanishes into a log nobody tails.
**Verdict: LATER.** Adopt the first time an error is reported verbally that you
cannot reproduce.

### NO — OpenTelemetry (`opentelemetry` 0.33, `tracing-opentelemetry`)

Distributed tracing for one binary and one database. There is nothing to
distribute. The setup is a collector, an exporter, a backend, and a version
matrix between `opentelemetry` and `tracing-opentelemetry` that breaks on a
schedule.
**Verdict: NO.**

### NO — Prometheus / `metrics` / a dashboard

You have `/health` and `/health/ready` and six users. The metric that matters —
"is the job worker draining" — is `SELECT count(*) FROM job WHERE locked_by IS
NULL`. Write it as `acp admin queue` if you want it; that is a subcommand, not a
metrics stack.
**Verdict: NO.**

---

## 5. Deployment

Pricing below is from general knowledge, not fetched — crates.io was reachable
from this machine but the open web was not. Treat the numbers as the right order
of magnitude and confirm before you sign up.

### Ranked

**1. Fly.io + a managed Postgres — recommended.**
You already have a working Dockerfile (once the `templates`/`static` bug is
fixed), which is exactly Fly's input; `fly launch` reads it and `fly deploy`
ships it. A Rust binary on Debian slim is the single best-fitting workload on
that platform — small image, fast boot, low idle memory. Expect a shared-cpu-1x
256–512MB machine in the ~$3–5/month range. Take **managed** Postgres (Fly's
own MPG offering, or Neon, which Fly integrates); do not run the old
unmanaged `fly pg` app — it is a Postgres you have to babysit, which is the
thing you are paying to avoid. Budget roughly $5 compute + $0–20 database.

**2. Railway — the easy answer if Fly's CLI annoys you.**
Push a repo, get a service and a managed Postgres in the same project with
`DATABASE_URL` injected. Marginally more expensive at rest, materially less to
think about. Usage-based, realistically $10–20/month for this. Choose it if the
team's tolerance for infrastructure is low; nothing about a Rust binary makes it
worse here.

**3. A Hetzner VPS (CX22, ~€4/month) with Postgres on the same box.**
Cheapest by a distance and genuinely defensible: one box, `docker compose up`,
Postgres in a container with a volume, `pg_dump` to object storage on a nightly
cron. The cost is that **you** are now the managed Postgres — upgrades, backups,
and the 2am restore you have never tested. For a system that is the team's source
of truth for all work, that is the wrong thing to own to save $10/month.
**Take this only if someone on the team already runs a VPS and wants to.**

**4. Render — no.**
It works, the Postgres is fine, but the free tier sleeps and the paid tier is
priced above Fly for the same thing, with slower deploys for container workloads.
No reason to pick it over 1 or 2.

### Put it behind Tailscale

The strongest deployment recommendation here is not a host. Six laptops and a
handful of agent processes, all of which you control — join them to a tailnet and
bind the server to the tailnet address. That deletes: public TLS and certificate
renewal, the rate-limiting decision in §2, the "what if a token leaks onto the
internet" branch of the threat model, and the need for any WAF. The bearer tokens
and scopes stay exactly as they are — this is defence in depth, not a replacement
for auth.

Fly and Railway both support this; on a VPS it is `tailscale up` and a firewall
rule. **Verdict: YES**, and do it before the first production deploy rather than
after.

### Distributing the Mac app without an Apple Developer account

Today `scripts/bundle-mac.sh` produces an unsigned `.app`. Anything unsigned that
arrives via a download gets a quarantine xattr, and Gatekeeper on current macOS
refuses to open it — the old right-click-Open workaround is increasingly
unreliable and is not something you want to explain six times.

**What to do, no account, no $99:**

1. Ad-hoc sign in the bundle script: `codesign --force --deep --sign - "Airtribe Control Plane.app"`. Ad-hoc signing does not satisfy Gatekeeper, but it does keep the bundle's own integrity checks happy and stops some of the stranger failure modes.
2. Distribute by an install script rather than a zip on Slack:
   ```sh
   curl -fsSL https://<host>/install-app.sh | sh
   ```
   which downloads the bundle, `xattr -dr com.apple.quarantine`s it, and moves it
   to `/Applications`. Quarantine is applied by the downloading *application*, so
   stripping it in the script you already trust is the honest, one-step path.
3. Better still for a six-person team: skip distribution. `just app` builds and
   runs it from source in 2.6s, and everyone on the team has the repo. Ship the
   `.app` only to people who will not have Rust installed.

**What changes with a $99 Developer ID:** you get `codesign` with a Developer ID
Application certificate plus `xcrun notarytool submit --wait` and `xcrun stapler
staple`, added to `bundle-mac.sh` as about ten lines. Then the app double-clicks
open with no warning, updates cleanly, and you can put the download behind a plain
URL. **Verdict: LATER.** Worth the $99 the moment a non-engineer needs the app, or
the moment you get tired of the xattr step. Not before.

---

## Considered and rejected

| Thing | Why not |
|---|---|
| OpenAPI (`utoipa`, `aide`) | Agents use MCP, which ships better hand-written docs. The other two clients share types with the server in-repo. A spec with no reader. |
| `garde` / `validator` / `axum-valid` | The validation is "non-empty string" and "row exists". `AppError::BadRequest` already does it in 29 places. |
| `cargo-nextest` | Suite is 8.9s, measured. `sqlx::test` already isolates. |
| `bacon` / `cargo-watch` | Rebuilds are 1.4–2.6s, measured. `cargo-watch` is also effectively unmaintained. |
| `cargo-deny` | No compliance obligation. Produces red CI you cannot act on. |
| Pre-commit hooks | People `--no-verify`. Same checks belong in CI. |
| `sccache` / `mold` / cranelift | Solving a build-time problem this repo does not have. |
| OpenTelemetry | One binary, one database. Nothing to distribute. |
| Prometheus / metrics stack | The one metric you want is a `SELECT count(*)`. |
| `tower-http` compression + CORS | Native clients, tiny JSON payloads, no browser — now or ever. |
| `tower_governor` rate limiting | Behind Tailscale the caller set is six authenticated laptops. |
| `egui_plot` | Non-goals rule out charts. A sparkline is 15 lines of `Shape::line`. |
| `egui_table` | `egui_extras::TableBuilder` is already in the tree and sufficient. |
| `egui_dock` / `egui_tiles` | Four fixed panes. Nothing to dock. |
| `egui_taffy` / `egui_flex` | A second layout model next to an already-coherent token system. |
| `egui_inbox` | `src/desktop/net.rs` already does channel-to-UI. |
| `egui_infinite_scroll` | No paging. |
| `egui_material_icons` | `egui-phosphor` is already the icon set. |
| SeaORM / any ORM | Already rejected in the design doc. Nothing has changed. |
| Redis | Already rejected in the design doc. `LISTEN/NOTIFY` is working. |
| `color-eyre` | `thiserror` + `AppError` is the error story and it is a good one. |

---

## Two things that are not tooling

Found while researching, both worth more than anything above:

- **`Dockerfile` cannot build.** It `COPY`s `templates ./templates` and
  `static ./static`; neither directory exists. Every deployment option in §5
  assumes this is fixed first.
- **The design doc claims compile-time checked SQL; there is none.** Zero
  `query!` macros in `src/`. Either adopt them (§3) or correct §2 of
  `docs/superpowers/specs/2026-09-18-airtribe-control-plane-design.md`, because
  right now the doc promises a safety property the code does not have.
