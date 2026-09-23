# Performance pass — the contract

Measured with `scripts/perf.sh` (`FRAMES=1` for frame times) against
`tests/fixtures/scale-seed.sql`: base = a year for five people (1,500 tasks),
stress = ×5. Baseline on the current code is in `/tmp/perf-before.txt`.

## Budgets

| Budget | Base today | Stress today |
| --- | --- | --- |
| Frame p95 ≤ 8 ms base, ≤ 16 ms stress | pass (Home 3.2) | **fail — Home 20.8** |
| Endpoint p95 ≤ 50 ms at base | pass, `/home` 42.7 | `/home` **1,036** |
| Page load (first data drawn) | ~110 ms | **~1,000 ms every page** — they all wait on `/home` |
| Idle frames ≈ 0/s | pass | pass |
| Requests on first load | Projects **35** | Projects **155** |

## Owners — disjoint files

Never kill a process you did not start (track yours with `$!`/its port). If a
build fails in a file you do not own, wait a minute and retry.

| Owner | Files |
| --- | --- |
| **Server** | `src/controllers/**`, `src/routes/**`, `src/models/**`, `src/middleware/**`, `src/app.rs`, `tests/*.rs` (new tests) |
| **Client** | `src/desktop/**`, `src/cli/client.rs`, `src/cli/creds.rs` |
| **Parent** | `Cargo.toml`, `src/main.rs`, `scripts/`, measurement |

## Server

- **S1** `team_capacity` (`controllers/home.rs`): replace the per-task
  `EXISTS … = ANY(blocked_by)` scan with a CTE of waiting ids, e.g.
  `WITH waiting AS (SELECT DISTINCT unnest(blocked_by) AS id FROM task WHERE done_at IS NULL AND status <> 'dropped')`
  and `t.id IN (SELECT id FROM waiting)`. Same results — prove it with a test
  that compares the counts on a seeded graph.
- **S2** `GET /api/user/counts` (read scope) →
  `{"myOpen": <my tasks with doneAt null and status ≠ dropped>, "activeProjects": <projects with status active>}`.
  One cheap query each.
- **S3** `GET /api/user/projects` rows gain `"done"` and `"total"` — the same
  numbers `/flow` gives (done = `done_at` set, total excludes dropped), from
  one grouped query, not a query per project.
- **S4** Conditional GETs: every `200` JSON response to a `GET` under
  `/api/user/` carries a weak `ETag` (hash of the body — `std`'s `DefaultHasher`
  is fine, no new crate); a request whose `If-None-Match` matches gets `304`
  with an empty body. One middleware layer in `src/app.rs`, not per route.
- **S5** `middleware/auth.rs`: the `UPDATE credential SET last_used_at…` on
  every request runs only when `last_used_at` is older than 5 minutes (the
  90-day cap is on `created_at`, so this does not weaken it).
- **S6** Small ones: `progress()`'s per-project lookup into a `HashMap`;
  `label::by_project` filtered by project for a single-project `get()`; the
  labels fold in `list()` via a `HashMap`, not a nested scan.

## Client

- **C1** `Net` stores replies as `Arc<Value>`; add `net.shared(key) -> Option<Arc<Value>>`
  and stop cloning whole payloads every frame (`home.rs:150-151`,
  `chrome.rs:31`, `board.rs`, `mytasks.rs`, `projects.rs`, `task.rs` — every
  `net.data(..).cloned()`).
- **C2** `net.generation(key) -> u64`, bumped on each new `200` reply (not on a
  `304`). Views cache what they derive — sorted rows, id maps, Needs-attention
  alerts, card counts, filter menus, lowercased search keys — and recompute only
  when a generation they depend on changes. Home first; then My Tasks, the
  project page and the Projects list where the same pattern appears.
- **C3** The sidebar counts come from `GET /api/user/counts` (key
  `sidebar:counts` — not `__`-prefixed, so the 30 s refresh covers it); nothing
  outside Home fetches `/api/user/home`. Invalidate it wherever `home` is
  invalidated today.
- **C4** The Projects list reads `done`/`total` from the `/projects` rows and
  stops fetching `/flow` per project. The project page keeps its own `/flow`.
- **C5** Conditional GETs: store each key's `ETag`, send `If-None-Match` on
  refetch; on `304` keep the data and do not bump the generation.
- **C6** Task page: fetch people on the first frame alongside the task, not
  after it arrives.

## Verification

`cargo build --all-targets --features app` clean; `DATABASE_URL=postgres://localhost:5433/acp_dev cargo test --features app` green; each owner's claimed gain shown with `scripts/perf.sh` / `FRAMES=1`. The parent re-runs the whole
measurement at the end and compares against the baseline.
