# Airtribe Control Plane — P1 Core & CLI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the control plane usable by the team from a terminal: authenticated callers, phases, tasks with filtering and assignment, artifacts, and an `acp` CLI.

**Architecture:** Every request resolves to an `Actor` through a bearer-token extractor. That `Actor` flows into the existing `change` write path, so scopes decide whether a mutation applies immediately or queues as pending. Domain endpoints follow the P0 project pattern exactly: model, controller, route.

**Tech Stack:** Rust 1.96, Axum 0.8, sqlx 0.8, tokio, serde, clap 4, reqwest.

**Spec:** `docs/superpowers/specs/2026-09-18-airtribe-control-plane-design.md`

**Builds on:** `docs/superpowers/plans/2026-09-18-p0-backbone.md` (merged)

## Global Constraints

- All P0 constraints still hold: `snake_case` columns, camelCase on the wire, the `{ success, data }` envelope, one `change` row per mutation, no ORM, no Redis.
- Postgres listens on **5433** on this machine (Docker holds 5432). `DATABASE_URL=postgres://localhost:5433/acp_dev`.
- Google SSO is deferred. P1 authenticates with bearer tokens stored in `agent_token`. A later SSO route mints rows into that same table, so no schema or extractor change is needed then.
- Tokens are never stored in plaintext. The database holds a SHA-256 hex digest; the raw token is shown once at mint time.
- Scope names are exactly: `read`, `claim`, `propose`, `write`.
- `can_apply` is true if and only if the actor's scopes contain `write`.
- Every new endpoint requires at minimum the `read` scope. Mutations require `propose` or `write`.

---

### Task 1: Token minting and hashing

**Files:**
- Create: `migrations/20260920000001_token_person_backfill.sql`
- Create: `src/models/token.rs`
- Create: `src/controllers/token.rs`
- Modify: `src/models/mod.rs`
- Modify: `src/controllers/mod.rs`
- Test: `tests/token.rs`

**Interfaces:**
- Consumes: `AppState`, `AppError`, `AppResult` from P0.
- Produces:
  - `acp_server::models::token::hash_token(raw: &str) -> String` — lowercase SHA-256 hex.
  - `acp_server::models::token::TokenRow { id, label, owner_id, scopes: Vec<String>, expires_at, revoked_at }`
  - `acp_server::models::token::lookup(db: &sqlx::PgPool, raw: &str) -> AppResult<Option<TokenRow>>` — returns `None` for unknown, expired, or revoked tokens.
  - `acp_server::controllers::token::mint(state: &AppState, label: &str, owner_email: &str, scopes: Vec<String>, valid_days: i64) -> AppResult<(String, TokenRow)>` — the `String` is the raw token, returned once.

- [ ] **Step 1: Add the sha2 dependency**

```bash
cargo add sha2 rand -q
```

- [ ] **Step 2: Write the failing test**

Create `tests/token.rs`:

```rust
use acp_server::controllers::token;
use acp_server::db::AppState;
use acp_server::models::token::{hash_token, lookup};
use sqlx::PgPool;

async fn seed_person(pool: &PgPool) -> uuid::Uuid {
    sqlx::query_scalar("INSERT INTO person (email, name) VALUES ($1, $2) RETURNING id")
        .bind("anmol@airtribe.live")
        .bind("Anmol")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[test]
fn hashing_is_stable_and_not_the_input() {
    let h = hash_token("secret");
    assert_eq!(h, hash_token("secret"));
    assert_ne!(h, "secret");
    assert_eq!(h.len(), 64);
}

#[sqlx::test]
async fn minted_token_resolves_and_is_stored_hashed(pool: PgPool) {
    seed_person(&pool).await;
    let state = AppState { db: pool.clone() };

    let (raw, row) = token::mint(&state, "laptop", "anmol@airtribe.live", vec!["read".into(), "write".into()], 30)
        .await
        .unwrap();

    assert_eq!(row.scopes, vec!["read".to_string(), "write".to_string()]);

    let stored: String = sqlx::query_scalar("SELECT token_hash FROM agent_token WHERE id = $1")
        .bind(row.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_ne!(stored, raw, "the raw token must never be stored");
    assert_eq!(stored, hash_token(&raw));

    let found = lookup(&pool, &raw).await.unwrap();
    assert_eq!(found.unwrap().id, row.id);
}

#[sqlx::test]
async fn revoked_and_expired_tokens_do_not_resolve(pool: PgPool) {
    seed_person(&pool).await;
    let state = AppState { db: pool.clone() };

    let (revoked_raw, revoked) = token::mint(&state, "old", "anmol@airtribe.live", vec!["read".into()], 30).await.unwrap();
    sqlx::query("UPDATE agent_token SET revoked_at = now() WHERE id = $1")
        .bind(revoked.id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(lookup(&pool, &revoked_raw).await.unwrap().is_none(), "revoked token must not resolve");

    let (expired_raw, expired) = token::mint(&state, "stale", "anmol@airtribe.live", vec!["read".into()], 30).await.unwrap();
    sqlx::query("UPDATE agent_token SET expires_at = now() - interval '1 day' WHERE id = $1")
        .bind(expired.id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(lookup(&pool, &expired_raw).await.unwrap().is_none(), "expired token must not resolve");
}

#[sqlx::test]
async fn minting_for_an_unknown_person_fails(pool: PgPool) {
    let state = AppState { db: pool };
    let result = token::mint(&state, "ghost", "nobody@airtribe.live", vec!["read".into()], 30).await;
    assert!(result.is_err());
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test --test token`
Expected: FAIL — `acp_server::models::token` does not exist.

- [ ] **Step 4: Write the token model**

Create `src/models/token.rs`:

```rust
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::AppResult;

/// Tokens are stored as a SHA-256 hex digest. The raw value is shown once at
/// mint time and never persisted.
pub fn hash_token(raw: &str) -> String {
    let digest = Sha256::digest(raw.as_bytes());
    format!("{digest:x}")
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TokenRow {
    pub id: Uuid,
    pub label: String,
    pub owner_id: Uuid,
    pub scopes: Vec<String>,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

/// Resolve a raw bearer token. Unknown, revoked, and expired tokens all return
/// `None` so a caller cannot tell them apart.
pub async fn lookup(db: &PgPool, raw: &str) -> AppResult<Option<TokenRow>> {
    let row = sqlx::query_as::<_, TokenRow>(
        "SELECT id, label, owner_id, scopes, expires_at, revoked_at
         FROM agent_token
         WHERE token_hash = $1 AND revoked_at IS NULL AND expires_at > now()",
    )
    .bind(hash_token(raw))
    .fetch_optional(db)
    .await?;

    Ok(row)
}
```

- [ ] **Step 5: Write the token controller**

Create `src/controllers/token.rs`:

```rust
use rand::RngCore;

use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::token::{hash_token, TokenRow};

fn generate_raw_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub async fn mint(
    state: &AppState,
    label: &str,
    owner_email: &str,
    scopes: Vec<String>,
    valid_days: i64,
) -> AppResult<(String, TokenRow)> {
    for scope in &scopes {
        if !matches!(scope.as_str(), "read" | "claim" | "propose" | "write") {
            return Err(AppError::BadRequest(format!("unknown scope '{scope}'")));
        }
    }

    let owner_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM person WHERE email = $1 AND deleted_at IS NULL",
    )
    .bind(owner_email)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("no person with email '{owner_email}'")))?;

    let raw = generate_raw_token();

    let row = sqlx::query_as::<_, TokenRow>(
        "INSERT INTO agent_token (label, token_hash, owner_id, scopes, expires_at)
         VALUES ($1, $2, $3, $4, now() + ($5 || ' days')::interval)
         RETURNING id, label, owner_id, scopes, expires_at, revoked_at",
    )
    .bind(label)
    .bind(hash_token(&raw))
    .bind(owner_id)
    .bind(&scopes)
    .bind(valid_days.to_string())
    .fetch_one(&state.db)
    .await?;

    Ok((raw, row))
}

/// Revoking a person revokes every token they own.
pub async fn revoke_for_person(state: &AppState, owner_email: &str) -> AppResult<u64> {
    let result = sqlx::query(
        "UPDATE agent_token SET revoked_at = now()
         WHERE revoked_at IS NULL
           AND owner_id = (SELECT id FROM person WHERE email = $1)",
    )
    .bind(owner_email)
    .execute(&state.db)
    .await?;

    Ok(result.rows_affected())
}
```

- [ ] **Step 6: Register the modules**

Append to `src/models/mod.rs`:

```rust
pub mod token;
```

Append to `src/controllers/mod.rs`:

```rust
pub mod token;
```

- [ ] **Step 6b: Write the bootstrap admin binary**

The first token is a chicken-and-egg problem: no token exists to authenticate a
mint call. This binary talks to the database directly and is the only way in.

Create `src/bin/acp-admin.rs`:

```rust
use acp_server::{config::Config, controllers::token, db};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "acp-admin", about = "Bootstrap administration for the control plane")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Add a person who can own tokens
    AddPerson { email: String, name: String },
    /// Mint a token and print it once
    Mint {
        label: String,
        #[arg(long)] owner: String,
        #[arg(long, value_delimiter = ',', default_value = "read")] scopes: Vec<String>,
        #[arg(long, default_value_t = 30)] days: i64,
    },
}

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    let cli = Cli::parse();

    let config = Config::from_env().expect("missing DATABASE_URL");
    let pool = db::connect(&config.database_url).await.expect("cannot connect to Postgres");
    let state = db::AppState { db: pool };

    match cli.command {
        Command::AddPerson { email, name } => {
            sqlx::query("INSERT INTO person (email, name) VALUES ($1, $2) ON CONFLICT (email) DO NOTHING")
                .bind(&email).bind(&name)
                .execute(&state.db).await.expect("insert failed");
            println!("person ready: {email}");
        }
        Command::Mint { label, owner, scopes, days } => {
            match token::mint(&state, &label, &owner, scopes, days).await {
                Ok((raw, row)) => {
                    println!("token for '{}' (expires {})", row.label, row.expires_at);
                    println!("{raw}");
                    println!("\nThis is shown once. Export it:\n  export ACP_TOKEN={raw}");
                }
                Err(e) => { eprintln!("{e}"); std::process::exit(1); }
            }
        }
    }
}
```

This binary needs `clap`, which Task 6 Step 1 also adds. Add it now instead:

```bash
cargo add clap -F derive -q
```

- [ ] **Step 7: Run the test to verify it passes**

Run: `cargo test --test token`
Expected: PASS — 4 tests.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock src tests
git commit -m "feat: add token minting with hashed storage and scope validation"
```

---

### Task 2: The Actor extractor

**Files:**
- Create: `src/middleware/mod.rs`
- Create: `src/middleware/auth.rs`
- Modify: `src/lib.rs`
- Modify: `src/routes/user/project.rs`
- Modify: `tests/project_api.rs`
- Test: `tests/auth.rs`

**Interfaces:**
- Consumes: `lookup`, `TokenRow` from Task 1; `Actor` from P0.
- Produces:
  - `acp_server::middleware::auth::Caller { pub actor: crate::models::change::Actor, pub scopes: Vec<String> }`
  - `impl FromRequestParts<AppState> for Caller` — reads `Authorization: Bearer <token>`, rejects with `AppError::Unauthorized` when absent or unresolvable.
  - `Caller::require(&self, scope: &str) -> AppResult<()>` — returns `AppError::Forbidden` when the scope is missing.
  - `Caller::can_mutate(&self) -> AppResult<()>` — succeeds when scopes contain `propose` or `write`.

`Caller.actor.can_apply` is true only when the token carries `write`, which is what routes a `propose`-scoped agent's mutation into the pending queue.

- [ ] **Step 1: Write the failing test**

Create `tests/auth.rs`:

```rust
use acp_server::controllers::token;
use acp_server::db::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sqlx::PgPool;
use tower::ServiceExt;

async fn seed(pool: &PgPool) {
    sqlx::query("INSERT INTO person (email, name) VALUES ($1, $2)")
        .bind("anmol@airtribe.live")
        .bind("Anmol")
        .execute(pool)
        .await
        .unwrap();
}

fn create_req(token: Option<&str>) -> Request<Body> {
    let mut b = Request::builder()
        .method("POST")
        .uri("/api/user/projects")
        .header("content-type", "application/json");
    if let Some(t) = token {
        b = b.header("authorization", format!("Bearer {t}"));
    }
    b.body(Body::from(r#"{"key":"acp","name":"Control Plane"}"#)).unwrap()
}

#[sqlx::test]
async fn requests_without_a_token_are_rejected(pool: PgPool) {
    let app = acp_server::app::app(AppState { db: pool });
    let response = app.oneshot(create_req(None)).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn a_garbage_token_is_rejected(pool: PgPool) {
    let app = acp_server::app::app(AppState { db: pool });
    let response = app.oneshot(create_req(Some("not-a-real-token"))).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn read_only_token_cannot_mutate(pool: PgPool) {
    seed(&pool).await;
    let state = AppState { db: pool };
    let (raw, _) = token::mint(&state, "ro", "anmol@airtribe.live", vec!["read".into()], 30).await.unwrap();

    let response = acp_server::app::app(state).oneshot(create_req(Some(&raw))).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn write_token_applies_immediately(pool: PgPool) {
    seed(&pool).await;
    let state = AppState { db: pool.clone() };
    let (raw, _) = token::mint(&state, "rw", "anmol@airtribe.live", vec!["read".into(), "write".into()], 30).await.unwrap();

    let response = acp_server::app::app(state).oneshot(create_req(Some(&raw))).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let state_col: String = sqlx::query_scalar("SELECT state FROM change LIMIT 1").fetch_one(&pool).await.unwrap();
    assert_eq!(state_col, "applied");
}

#[sqlx::test]
async fn propose_token_queues_a_pending_change(pool: PgPool) {
    seed(&pool).await;
    let state = AppState { db: pool.clone() };
    let (raw, _) = token::mint(&state, "hermes", "anmol@airtribe.live", vec!["read".into(), "propose".into()], 30).await.unwrap();

    let response = acp_server::app::app(state).oneshot(create_req(Some(&raw))).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let _json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    let state_col: String = sqlx::query_scalar("SELECT state FROM change LIMIT 1").fetch_one(&pool).await.unwrap();
    assert_eq!(state_col, "pending", "a propose-scoped actor must never write an applied change");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test auth`
Expected: FAIL — routes currently use `system_actor()` and accept unauthenticated requests, so the first test gets 200 instead of 401.

- [ ] **Step 3: Write the extractor**

Create `src/middleware/auth.rs`:

```rust
use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::change::Actor;
use crate::models::token;

#[derive(Debug, Clone)]
pub struct Caller {
    pub actor: Actor,
    pub scopes: Vec<String>,
}

impl Caller {
    pub fn has(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }

    pub fn require(&self, scope: &str) -> AppResult<()> {
        if self.has(scope) {
            Ok(())
        } else {
            Err(AppError::Forbidden(format!("this token lacks the '{scope}' scope")))
        }
    }

    /// A mutation needs either `propose` (queues as pending) or `write`
    /// (applies immediately).
    pub fn can_mutate(&self) -> AppResult<()> {
        if self.has("propose") || self.has("write") {
            Ok(())
        } else {
            Err(AppError::Forbidden("this token lacks the 'propose' or 'write' scope".into()))
        }
    }
}

impl FromRequestParts<AppState> for Caller {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let raw = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(|| AppError::Unauthorized("missing bearer token".into()))?;

        let row = token::lookup(&state.db, raw)
            .await?
            .ok_or_else(|| AppError::Unauthorized("invalid or expired token".into()))?;

        let can_apply = row.scopes.iter().any(|s| s == "write");

        Ok(Caller {
            actor: Actor {
                label: row.label,
                person_id: Some(row.owner_id),
                can_apply,
            },
            scopes: row.scopes,
        })
    }
}
```

Create `src/middleware/mod.rs`:

```rust
pub mod auth;
```

Append to `src/lib.rs`:

```rust
pub mod middleware;
```

- [ ] **Step 4: Replace the P0 system actor on the project routes**

Replace `src/routes/user/project.rs` with:

```rust
use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;

use crate::controllers;
use crate::db::AppState;
use crate::errors::AppResult;
use crate::middleware::auth::Caller;
use crate::models::project::Project;
use crate::response::ApiResponse;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectBody {
    pub key: String,
    pub name: String,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/user/projects", post(create).get(list))
}

async fn create(
    State(state): State<AppState>,
    caller: Caller,
    Json(body): Json<CreateProjectBody>,
) -> AppResult<ApiResponse<Project>> {
    caller.can_mutate()?;
    let project = controllers::project::create(&state, &caller.actor, body.key, body.name).await?;
    Ok(ApiResponse::ok(project))
}

async fn list(State(state): State<AppState>, caller: Caller) -> AppResult<ApiResponse<Vec<Project>>> {
    caller.require("read")?;
    let projects = controllers::project::list(&state).await?;
    Ok(ApiResponse::ok(projects))
}
```

Note: `Caller` must appear *before* `Json<...>` in the handler argument list. Axum requires the body-consuming extractor to come last.

- [ ] **Step 5: Update the P0 project tests to authenticate**

In `tests/project_api.rs`, add this helper and use it in all three tests:

```rust
async fn write_token(pool: &PgPool) -> String {
    sqlx::query("INSERT INTO person (email, name) VALUES ($1, $2)")
        .bind("anmol@airtribe.live")
        .bind("Anmol")
        .execute(pool)
        .await
        .unwrap();
    let state = acp_server::db::AppState { db: pool.clone() };
    acp_server::controllers::token::mint(&state, "test", "anmol@airtribe.live", vec!["read".into(), "write".into()], 30)
        .await
        .unwrap()
        .0
}
```

Change `fn post` to take a token:

```rust
fn post(uri: &str, token: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(body.to_string()))
        .unwrap()
}
```

Each test calls `let t = write_token(&pool).await;` first, passes `&t` to `post`, and adds the same `authorization` header to the GET request in `listing_returns_created_projects`.

- [ ] **Step 6: Run all tests**

Run: `cargo test`
Expected: PASS — 20 tests.

- [ ] **Step 7: Commit**

```bash
git add src tests
git commit -m "feat: authenticate callers with scoped bearer tokens"
```

---

### Task 3: Phase CRUD

**Files:**
- Create: `src/models/phase.rs`
- Create: `src/controllers/phase.rs`
- Create: `src/routes/user/phase.rs`
- Modify: `src/models/mod.rs`, `src/controllers/mod.rs`, `src/routes/user/mod.rs`, `src/app.rs`
- Test: `tests/phase_api.rs`

**Interfaces:**
- Consumes: `Caller`, `record`, `AppState`.
- Produces:
  - `acp_server::models::phase::Phase { id, project_id, position, name, status, gate, created_at, updated_at }`
  - `acp_server::controllers::phase::create(state, actor, project_id: Uuid, name: String, position: i32, gate: bool) -> AppResult<Phase>`
  - `acp_server::controllers::phase::list(state, project_id: Uuid) -> AppResult<Vec<Phase>>`
  - `acp_server::controllers::phase::set_status(state, actor, id: Uuid, status: String) -> AppResult<Phase>`
  - Routes `POST /api/user/projects/{project_id}/phases`, `GET /api/user/projects/{project_id}/phases`, `PATCH /api/user/phases/{id}`

- [ ] **Step 1: Write the failing test**

Create `tests/phase_api.rs`:

```rust
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

async fn setup(pool: &PgPool) -> (String, Uuid) {
    sqlx::query("INSERT INTO person (email, name) VALUES ($1, $2)")
        .bind("anmol@airtribe.live").bind("Anmol").execute(pool).await.unwrap();
    let state = acp_server::db::AppState { db: pool.clone() };
    let (raw, _) = acp_server::controllers::token::mint(
        &state, "test", "anmol@airtribe.live", vec!["read".into(), "write".into()], 30,
    ).await.unwrap();
    let project_id: Uuid = sqlx::query_scalar("INSERT INTO project (key, name) VALUES ('acp','ACP') RETURNING id")
        .fetch_one(pool).await.unwrap();
    (raw, project_id)
}

fn req(method: &str, uri: &str, token: &str, body: Option<serde_json::Value>) -> Request<Body> {
    let b = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"));
    match body {
        Some(v) => b.body(Body::from(v.to_string())).unwrap(),
        None => b.body(Body::empty()).unwrap(),
    }
}

async fn json_of(response: axum::response::Response) -> serde_json::Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[sqlx::test]
async fn phases_are_created_listed_in_order_and_updated(pool: PgPool) {
    let (token, project_id) = setup(&pool).await;
    let state = acp_server::db::AppState { db: pool.clone() };

    for (pos, name) in [(2, "Build"), (1, "Spec")] {
        let response = acp_server::app::app(state.clone())
            .oneshot(req("POST", &format!("/api/user/projects/{project_id}/phases"), &token,
                Some(serde_json::json!({ "name": name, "position": pos }))))
            .await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "creating phase {name}");
    }

    let response = acp_server::app::app(state.clone())
        .oneshot(req("GET", &format!("/api/user/projects/{project_id}/phases"), &token, None))
        .await.unwrap();
    let json = json_of(response).await;
    assert_eq!(json["data"][0]["name"], "Spec", "phases list by position");
    assert_eq!(json["data"][1]["name"], "Build");
    assert_eq!(json["data"][0]["status"], "planned");

    let phase_id = json["data"][0]["id"].as_str().unwrap().to_string();
    let response = acp_server::app::app(state)
        .oneshot(req("PATCH", &format!("/api/user/phases/{phase_id}"), &token,
            Some(serde_json::json!({ "status": "active" }))))
        .await.unwrap();
    assert_eq!(json_of(response).await["data"]["status"], "active");

    let changes: i64 = sqlx::query_scalar("SELECT count(*) FROM change WHERE target_type = 'phase'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(changes, 3, "two creates and one update");
}

#[sqlx::test]
async fn duplicate_positions_in_one_project_are_rejected(pool: PgPool) {
    let (token, project_id) = setup(&pool).await;
    let state = acp_server::db::AppState { db: pool };

    for _ in 0..2 {
        let _ = acp_server::app::app(state.clone())
            .oneshot(req("POST", &format!("/api/user/projects/{project_id}/phases"), &token,
                Some(serde_json::json!({ "name": "Spec", "position": 1 }))))
            .await.unwrap();
    }

    let response = acp_server::app::app(state)
        .oneshot(req("POST", &format!("/api/user/projects/{project_id}/phases"), &token,
            Some(serde_json::json!({ "name": "Other", "position": 1 }))))
        .await.unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[sqlx::test]
async fn an_invalid_status_is_rejected(pool: PgPool) {
    let (token, project_id) = setup(&pool).await;
    let state = acp_server::db::AppState { db: pool };

    let response = acp_server::app::app(state.clone())
        .oneshot(req("POST", &format!("/api/user/projects/{project_id}/phases"), &token,
            Some(serde_json::json!({ "name": "Spec", "position": 1 }))))
        .await.unwrap();
    let phase_id = json_of(response).await["data"]["id"].as_str().unwrap().to_string();

    let response = acp_server::app::app(state)
        .oneshot(req("PATCH", &format!("/api/user/phases/{phase_id}"), &token,
            Some(serde_json::json!({ "status": "banana" }))))
        .await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test phase_api`
Expected: FAIL — the phase routes 404.

- [ ] **Step 3: Write the phase model**

Create `src/models/phase.rs`:

```rust
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

pub const PHASE_STATUSES: [&str; 4] = ["planned", "active", "blocked", "done"];

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Phase {
    pub id: Uuid,
    pub project_id: Uuid,
    pub position: i32,
    pub name: String,
    pub status: String,
    pub gate: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

- [ ] **Step 4: Write the phase controller**

Create `src/controllers/phase.rs`:

```rust
use serde_json::json;
use uuid::Uuid;

use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::change::{record, Actor, Op, TargetType};
use crate::models::phase::{Phase, PHASE_STATUSES};

pub async fn create(
    state: &AppState,
    actor: &Actor,
    project_id: Uuid,
    name: String,
    position: i32,
    gate: bool,
) -> AppResult<Phase> {
    if name.trim().is_empty() {
        return Err(AppError::BadRequest("name is required".into()));
    }

    let mut tx = state.db.begin().await?;

    let phase: Phase = sqlx::query_as(
        "INSERT INTO phase (project_id, name, position, gate) VALUES ($1, $2, $3, $4)
         RETURNING id, project_id, position, name, status, gate, created_at, updated_at",
    )
    .bind(project_id)
    .bind(&name)
    .bind(position)
    .bind(gate)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            AppError::Conflict(format!("position {position} is already taken in this project"))
        }
        sqlx::Error::Database(db) if db.is_foreign_key_violation() => {
            AppError::NotFound("project not found".into())
        }
        _ => AppError::Database(e),
    })?;

    record(&mut tx, actor, TargetType::Phase, phase.id, Op::Create,
        json!({ "name": phase.name, "position": phase.position })).await?;

    tx.commit().await?;
    Ok(phase)
}

pub async fn list(state: &AppState, project_id: Uuid) -> AppResult<Vec<Phase>> {
    let phases = sqlx::query_as(
        "SELECT id, project_id, position, name, status, gate, created_at, updated_at
         FROM phase WHERE project_id = $1 ORDER BY position",
    )
    .bind(project_id)
    .fetch_all(&state.db)
    .await?;

    Ok(phases)
}

pub async fn set_status(state: &AppState, actor: &Actor, id: Uuid, status: String) -> AppResult<Phase> {
    if !PHASE_STATUSES.contains(&status.as_str()) {
        return Err(AppError::BadRequest(format!(
            "status must be one of {}", PHASE_STATUSES.join(", ")
        )));
    }

    let mut tx = state.db.begin().await?;

    let phase: Phase = sqlx::query_as(
        "UPDATE phase SET status = $2, updated_at = now() WHERE id = $1
         RETURNING id, project_id, position, name, status, gate, created_at, updated_at",
    )
    .bind(id)
    .bind(&status)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::NotFound("phase not found".into()))?;

    record(&mut tx, actor, TargetType::Phase, phase.id, Op::Update, json!({ "status": status })).await?;

    tx.commit().await?;
    Ok(phase)
}
```

- [ ] **Step 5: Write the phase routes**

Create `src/routes/user/phase.rs`:

```rust
use axum::extract::{Path, State};
use axum::routing::{patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use crate::controllers;
use crate::db::AppState;
use crate::errors::AppResult;
use crate::middleware::auth::Caller;
use crate::models::phase::Phase;
use crate::response::ApiResponse;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePhaseBody {
    pub name: String,
    pub position: i32,
    #[serde(default)]
    pub gate: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePhaseBody {
    pub status: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/user/projects/{project_id}/phases", post(create).get(list))
        .route("/api/user/phases/{id}", patch(update))
}

async fn create(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<CreatePhaseBody>,
) -> AppResult<ApiResponse<Phase>> {
    caller.can_mutate()?;
    let phase = controllers::phase::create(&state, &caller.actor, project_id, body.name, body.position, body.gate).await?;
    Ok(ApiResponse::ok(phase))
}

async fn list(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    caller: Caller,
) -> AppResult<ApiResponse<Vec<Phase>>> {
    caller.require("read")?;
    Ok(ApiResponse::ok(controllers::phase::list(&state, project_id).await?))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<UpdatePhaseBody>,
) -> AppResult<ApiResponse<Phase>> {
    caller.can_mutate()?;
    Ok(ApiResponse::ok(controllers::phase::set_status(&state, &caller.actor, id, body.status).await?))
}
```

- [ ] **Step 6: Register everything**

Append `pub mod phase;` to each of `src/models/mod.rs`, `src/controllers/mod.rs`, and `src/routes/user/mod.rs`.

Add to `src/app.rs` inside the `Router::new()` chain, before `.with_state(state)`:

```rust
        .merge(routes::user::phase::routes())
```

- [ ] **Step 7: Run all tests**

Run: `cargo test`
Expected: PASS — 23 tests.

- [ ] **Step 8: Commit**

```bash
git add src tests
git commit -m "feat: add phase create, list, and status update"
```

---

### Task 4: Task CRUD with filtering and assignment

**Files:**
- Create: `src/models/task.rs`
- Create: `src/controllers/task.rs`
- Create: `src/routes/user/task.rs`
- Modify: `src/models/mod.rs`, `src/controllers/mod.rs`, `src/routes/user/mod.rs`, `src/app.rs`
- Test: `tests/task_api.rs`

**Interfaces:**
- Consumes: `Caller`, `record`, `AppState`.
- Produces:
  - `acp_server::models::task::Task { id, phase_id, title, body, status, priority, assignee_kind, assignee_person_id, assignee_token_id, claimed_by, claim_expires_at, created_at, updated_at }`
  - `acp_server::models::task::TaskFilter { pub project_id: Option<Uuid>, pub phase_id: Option<Uuid>, pub status: Option<String>, pub assignee_email: Option<String>, pub assignee_kind: Option<String> }`
  - `acp_server::controllers::task::create(state, actor, phase_id: Uuid, title: String, body: String, priority: i32) -> AppResult<Task>`
  - `acp_server::controllers::task::search(state, filter: TaskFilter) -> AppResult<Vec<Task>>`
  - `acp_server::controllers::task::set_status(state, actor, id: Uuid, status: String) -> AppResult<Task>`
  - `acp_server::controllers::task::assign(state, actor, id: Uuid, to: Assignee) -> AppResult<Task>` where `Assignee` is `enum Assignee { Person(String), Agent(String), Nobody }` carrying an email or token label.
  - Routes `POST /api/user/phases/{phase_id}/tasks`, `GET /api/user/tasks`, `PATCH /api/user/tasks/{id}`, `POST /api/user/tasks/{id}/assign`

- [ ] **Step 1: Write the failing test**

Create `tests/task_api.rs`:

```rust
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

async fn setup(pool: &PgPool) -> (String, Uuid) {
    sqlx::query("INSERT INTO person (email, name) VALUES ($1, $2)")
        .bind("anmol@airtribe.live").bind("Anmol").execute(pool).await.unwrap();
    let state = acp_server::db::AppState { db: pool.clone() };
    let (raw, _) = acp_server::controllers::token::mint(
        &state, "test", "anmol@airtribe.live", vec!["read".into(), "write".into()], 30,
    ).await.unwrap();
    let project_id: Uuid = sqlx::query_scalar("INSERT INTO project (key, name) VALUES ('acp','ACP') RETURNING id")
        .fetch_one(pool).await.unwrap();
    let phase_id: Uuid = sqlx::query_scalar(
        "INSERT INTO phase (project_id, name, position) VALUES ($1, 'Build', 1) RETURNING id")
        .bind(project_id).fetch_one(pool).await.unwrap();
    (raw, phase_id)
}

fn req(method: &str, uri: &str, token: &str, body: Option<serde_json::Value>) -> Request<Body> {
    let b = Request::builder().method(method).uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"));
    match body {
        Some(v) => b.body(Body::from(v.to_string())).unwrap(),
        None => b.body(Body::empty()).unwrap(),
    }
}

async fn json_of(response: axum::response::Response) -> serde_json::Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[sqlx::test]
async fn tasks_are_created_and_filtered_by_status(pool: PgPool) {
    let (token, phase_id) = setup(&pool).await;
    let state = acp_server::db::AppState { db: pool };

    for title in ["wire auth", "write docs"] {
        let response = acp_server::app::app(state.clone())
            .oneshot(req("POST", &format!("/api/user/phases/{phase_id}/tasks"), &token,
                Some(serde_json::json!({ "title": title }))))
            .await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    let response = acp_server::app::app(state.clone())
        .oneshot(req("GET", "/api/user/tasks", &token, None)).await.unwrap();
    assert_eq!(json_of(response).await["data"].as_array().unwrap().len(), 2);

    let response = acp_server::app::app(state.clone())
        .oneshot(req("GET", "/api/user/tasks?status=done", &token, None)).await.unwrap();
    assert_eq!(json_of(response).await["data"].as_array().unwrap().len(), 0);

    let response = acp_server::app::app(state)
        .oneshot(req("GET", &format!("/api/user/tasks?phaseId={phase_id}"), &token, None)).await.unwrap();
    assert_eq!(json_of(response).await["data"].as_array().unwrap().len(), 2);
}

#[sqlx::test]
async fn a_task_can_be_assigned_to_a_person_and_to_an_agent(pool: PgPool) {
    let (token, phase_id) = setup(&pool).await;
    let state = acp_server::db::AppState { db: pool.clone() };
    acp_server::controllers::token::mint(&state, "hermes", "anmol@airtribe.live", vec!["claim".into()], 30)
        .await.unwrap();

    let response = acp_server::app::app(state.clone())
        .oneshot(req("POST", &format!("/api/user/phases/{phase_id}/tasks"), &token,
            Some(serde_json::json!({ "title": "migrate report" })))).await.unwrap();
    let task_id = json_of(response).await["data"]["id"].as_str().unwrap().to_string();

    let response = acp_server::app::app(state.clone())
        .oneshot(req("POST", &format!("/api/user/tasks/{task_id}/assign"), &token,
            Some(serde_json::json!({ "personEmail": "anmol@airtribe.live" })))).await.unwrap();
    let json = json_of(response).await;
    assert_eq!(json["data"]["assigneeKind"], "human");
    assert!(json["data"]["assigneePersonId"].is_string());

    let response = acp_server::app::app(state.clone())
        .oneshot(req("POST", &format!("/api/user/tasks/{task_id}/assign"), &token,
            Some(serde_json::json!({ "agentLabel": "hermes" })))).await.unwrap();
    let json = json_of(response).await;
    assert_eq!(json["data"]["assigneeKind"], "agent");
    assert!(json["data"]["assigneePersonId"].is_null(), "switching to an agent clears the person");
    assert!(json["data"]["assigneeTokenId"].is_string());

    let response = acp_server::app::app(state)
        .oneshot(req("GET", "/api/user/tasks?assigneeKind=agent", &token, None)).await.unwrap();
    assert_eq!(json_of(response).await["data"].as_array().unwrap().len(), 1);
}

#[sqlx::test]
async fn assigning_to_an_unknown_person_fails_and_records_nothing(pool: PgPool) {
    let (token, phase_id) = setup(&pool).await;
    let state = acp_server::db::AppState { db: pool.clone() };

    let response = acp_server::app::app(state.clone())
        .oneshot(req("POST", &format!("/api/user/phases/{phase_id}/tasks"), &token,
            Some(serde_json::json!({ "title": "x" })))).await.unwrap();
    let task_id = json_of(response).await["data"]["id"].as_str().unwrap().to_string();

    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM change").fetch_one(&pool).await.unwrap();

    let response = acp_server::app::app(state)
        .oneshot(req("POST", &format!("/api/user/tasks/{task_id}/assign"), &token,
            Some(serde_json::json!({ "personEmail": "ghost@airtribe.live" })))).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM change").fetch_one(&pool).await.unwrap();
    assert_eq!(before, after, "a failed assignment must leave no change row");
}

#[sqlx::test]
async fn an_invalid_task_status_is_rejected(pool: PgPool) {
    let (token, phase_id) = setup(&pool).await;
    let state = acp_server::db::AppState { db: pool };

    let response = acp_server::app::app(state.clone())
        .oneshot(req("POST", &format!("/api/user/phases/{phase_id}/tasks"), &token,
            Some(serde_json::json!({ "title": "x" })))).await.unwrap();
    let task_id = json_of(response).await["data"]["id"].as_str().unwrap().to_string();

    let response = acp_server::app::app(state)
        .oneshot(req("PATCH", &format!("/api/user/tasks/{task_id}"), &token,
            Some(serde_json::json!({ "status": "nope" })))).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test task_api`
Expected: FAIL — the task routes 404.

- [ ] **Step 3: Write the task model**

Create `src/models/task.rs`:

```rust
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

pub const TASK_STATUSES: [&str; 6] =
    ["open", "in_progress", "in_review", "blocked", "done", "dropped"];

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: Uuid,
    pub phase_id: Uuid,
    pub title: String,
    pub body: String,
    pub status: String,
    pub priority: i32,
    pub assignee_kind: Option<String>,
    pub assignee_person_id: Option<Uuid>,
    pub assignee_token_id: Option<Uuid>,
    pub claimed_by: Option<String>,
    pub claim_expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Default)]
pub struct TaskFilter {
    pub project_id: Option<Uuid>,
    pub phase_id: Option<Uuid>,
    pub status: Option<String>,
    pub assignee_email: Option<String>,
    pub assignee_kind: Option<String>,
}

pub const TASK_COLUMNS: &str = "id, phase_id, title, body, status, priority, \
    assignee_kind, assignee_person_id, assignee_token_id, claimed_by, \
    claim_expires_at, created_at, updated_at";
```

- [ ] **Step 4: Write the task controller**

Create `src/controllers/task.rs`:

```rust
use serde_json::json;
use uuid::Uuid;

use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::change::{record, Actor, Op, TargetType};
use crate::models::task::{Task, TaskFilter, TASK_COLUMNS, TASK_STATUSES};

pub enum Assignee {
    Person(String),
    Agent(String),
    Nobody,
}

pub async fn create(
    state: &AppState,
    actor: &Actor,
    phase_id: Uuid,
    title: String,
    body: String,
    priority: i32,
) -> AppResult<Task> {
    if title.trim().is_empty() {
        return Err(AppError::BadRequest("title is required".into()));
    }
    if !(0..=4).contains(&priority) {
        return Err(AppError::BadRequest("priority must be between 0 and 4".into()));
    }

    let mut tx = state.db.begin().await?;

    let task: Task = sqlx::query_as(&format!(
        "INSERT INTO task (phase_id, title, body, priority) VALUES ($1, $2, $3, $4)
         RETURNING {TASK_COLUMNS}"
    ))
    .bind(phase_id)
    .bind(&title)
    .bind(&body)
    .bind(priority)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_foreign_key_violation() => {
            AppError::NotFound("phase not found".into())
        }
        _ => AppError::Database(e),
    })?;

    record(&mut tx, actor, TargetType::Task, task.id, Op::Create, json!({ "title": task.title })).await?;

    tx.commit().await?;
    Ok(task)
}

/// Filters are applied with a single query using NULL-tolerant predicates, so
/// there is no dynamic SQL string building to audit.
pub async fn search(state: &AppState, filter: TaskFilter) -> AppResult<Vec<Task>> {
    let tasks = sqlx::query_as(&format!(
        "SELECT t.{} FROM task t
         JOIN phase p ON p.id = t.phase_id
         LEFT JOIN person per ON per.id = t.assignee_person_id
         WHERE ($1::uuid IS NULL OR p.project_id = $1)
           AND ($2::uuid IS NULL OR t.phase_id = $2)
           AND ($3::text IS NULL OR t.status = $3)
           AND ($4::text IS NULL OR per.email = $4)
           AND ($5::text IS NULL OR t.assignee_kind = $5)
         ORDER BY t.priority, t.created_at",
        TASK_COLUMNS.replace(", ", ", t.")
    ))
    .bind(filter.project_id)
    .bind(filter.phase_id)
    .bind(filter.status)
    .bind(filter.assignee_email)
    .bind(filter.assignee_kind)
    .fetch_all(&state.db)
    .await?;

    Ok(tasks)
}

pub async fn set_status(state: &AppState, actor: &Actor, id: Uuid, status: String) -> AppResult<Task> {
    if !TASK_STATUSES.contains(&status.as_str()) {
        return Err(AppError::BadRequest(format!(
            "status must be one of {}", TASK_STATUSES.join(", ")
        )));
    }

    let mut tx = state.db.begin().await?;

    let task: Task = sqlx::query_as(&format!(
        "UPDATE task SET status = $2, updated_at = now() WHERE id = $1 RETURNING {TASK_COLUMNS}"
    ))
    .bind(id)
    .bind(&status)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::NotFound("task not found".into()))?;

    record(&mut tx, actor, TargetType::Task, task.id, Op::Update, json!({ "status": status })).await?;

    tx.commit().await?;
    Ok(task)
}

pub async fn assign(state: &AppState, actor: &Actor, id: Uuid, to: Assignee) -> AppResult<Task> {
    let mut tx = state.db.begin().await?;

    let (kind, person_id, token_id, patch) = match &to {
        Assignee::Person(email) => {
            let pid: Uuid = sqlx::query_scalar("SELECT id FROM person WHERE email = $1 AND deleted_at IS NULL")
                .bind(email)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| AppError::NotFound(format!("no person with email '{email}'")))?;
            (Some("human"), Some(pid), None, json!({ "assignee": email }))
        }
        Assignee::Agent(label) => {
            let tid: Uuid = sqlx::query_scalar(
                "SELECT id FROM agent_token WHERE label = $1 AND revoked_at IS NULL AND expires_at > now()
                 ORDER BY created_at DESC LIMIT 1",
            )
            .bind(label)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("no active agent token labelled '{label}'")))?;
            (Some("agent"), None, Some(tid), json!({ "assignee": label }))
        }
        Assignee::Nobody => (None, None, None, json!({ "assignee": null })),
    };

    let task: Task = sqlx::query_as(&format!(
        "UPDATE task SET assignee_kind = $2, assignee_person_id = $3, assignee_token_id = $4,
                         updated_at = now()
         WHERE id = $1 RETURNING {TASK_COLUMNS}"
    ))
    .bind(id)
    .bind(kind)
    .bind(person_id)
    .bind(token_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::NotFound("task not found".into()))?;

    record(&mut tx, actor, TargetType::Task, task.id, Op::Update, patch).await?;

    tx.commit().await?;
    Ok(task)
}
```

- [ ] **Step 5: Write the task routes**

Create `src/routes/user/task.rs`:

```rust
use axum::extract::{Path, Query, State};
use axum::routing::{patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use crate::controllers;
use crate::controllers::task::Assignee;
use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::middleware::auth::Caller;
use crate::models::task::{Task, TaskFilter};
use crate::response::ApiResponse;

fn default_priority() -> i32 { 2 }

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskBody {
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default = "default_priority")]
    pub priority: i32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTaskBody {
    pub status: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignBody {
    pub person_email: Option<String>,
    pub agent_label: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskQuery {
    pub project_id: Option<Uuid>,
    pub phase_id: Option<Uuid>,
    pub status: Option<String>,
    pub assignee_email: Option<String>,
    pub assignee_kind: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/user/phases/{phase_id}/tasks", post(create))
        .route("/api/user/tasks", axum::routing::get(search))
        .route("/api/user/tasks/{id}", patch(update))
        .route("/api/user/tasks/{id}/assign", post(assign))
}

async fn create(
    State(state): State<AppState>,
    Path(phase_id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<CreateTaskBody>,
) -> AppResult<ApiResponse<Task>> {
    caller.can_mutate()?;
    let task = controllers::task::create(&state, &caller.actor, phase_id, body.title, body.body, body.priority).await?;
    Ok(ApiResponse::ok(task))
}

async fn search(
    State(state): State<AppState>,
    caller: Caller,
    Query(q): Query<TaskQuery>,
) -> AppResult<ApiResponse<Vec<Task>>> {
    caller.require("read")?;
    let filter = TaskFilter {
        project_id: q.project_id,
        phase_id: q.phase_id,
        status: q.status,
        assignee_email: q.assignee_email,
        assignee_kind: q.assignee_kind,
    };
    Ok(ApiResponse::ok(controllers::task::search(&state, filter).await?))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<UpdateTaskBody>,
) -> AppResult<ApiResponse<Task>> {
    caller.can_mutate()?;
    Ok(ApiResponse::ok(controllers::task::set_status(&state, &caller.actor, id, body.status).await?))
}

async fn assign(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(body): Json<AssignBody>,
) -> AppResult<ApiResponse<Task>> {
    caller.can_mutate()?;

    let to = match (body.person_email, body.agent_label) {
        (Some(_), Some(_)) => {
            return Err(AppError::BadRequest("give either personEmail or agentLabel, not both".into()))
        }
        (Some(email), None) => Assignee::Person(email),
        (None, Some(label)) => Assignee::Agent(label),
        (None, None) => Assignee::Nobody,
    };

    Ok(ApiResponse::ok(controllers::task::assign(&state, &caller.actor, id, to).await?))
}
```

- [ ] **Step 6: Register everything**

Append `pub mod task;` to each of `src/models/mod.rs`, `src/controllers/mod.rs`, and `src/routes/user/mod.rs`.

Add to the `Router::new()` chain in `src/app.rs`:

```rust
        .merge(routes::user::task::routes())
```

- [ ] **Step 7: Run all tests**

Run: `cargo test`
Expected: PASS — 27 tests.

- [ ] **Step 8: Commit**

```bash
git add src tests
git commit -m "feat: add tasks with filtering and human or agent assignment"
```

---

### Task 5: Artifacts

**Files:**
- Create: `src/models/artifact.rs`
- Create: `src/controllers/artifact.rs`
- Create: `src/routes/user/artifact.rs`
- Modify: `src/models/mod.rs`, `src/controllers/mod.rs`, `src/routes/user/mod.rs`, `src/app.rs`
- Test: `tests/artifact_api.rs`

**Interfaces:**
- Consumes: `Caller`, `record`, `AppState`.
- Produces:
  - `acp_server::models::artifact::Artifact { id, parent_type, parent_id, kind, url, title, metadata, added_by, created_at }`
  - `acp_server::controllers::artifact::add(state, actor, parent_type: String, parent_id: Uuid, kind: String, url: String, title: String) -> AppResult<Artifact>`
  - `acp_server::controllers::artifact::list(state, parent_type: String, parent_id: Uuid) -> AppResult<Vec<Artifact>>`
  - Routes `POST /api/user/artifacts`, `GET /api/user/artifacts?parentType=&parentId=`

- [ ] **Step 1: Write the failing test**

Create `tests/artifact_api.rs`:

```rust
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

async fn setup(pool: &PgPool) -> (String, Uuid) {
    sqlx::query("INSERT INTO person (email, name) VALUES ($1, $2)")
        .bind("anmol@airtribe.live").bind("Anmol").execute(pool).await.unwrap();
    let state = acp_server::db::AppState { db: pool.clone() };
    let (raw, _) = acp_server::controllers::token::mint(
        &state, "test", "anmol@airtribe.live", vec!["read".into(), "write".into()], 30,
    ).await.unwrap();
    let project_id: Uuid = sqlx::query_scalar("INSERT INTO project (key, name) VALUES ('acp','ACP') RETURNING id")
        .fetch_one(pool).await.unwrap();
    (raw, project_id)
}

fn req(method: &str, uri: &str, token: &str, body: Option<serde_json::Value>) -> Request<Body> {
    let b = Request::builder().method(method).uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"));
    match body {
        Some(v) => b.body(Body::from(v.to_string())).unwrap(),
        None => b.body(Body::empty()).unwrap(),
    }
}

async fn json_of(response: axum::response::Response) -> serde_json::Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[sqlx::test]
async fn artifacts_attach_to_a_parent_and_list_back(pool: PgPool) {
    let (token, project_id) = setup(&pool).await;
    let state = acp_server::db::AppState { db: pool };

    let response = acp_server::app::app(state.clone())
        .oneshot(req("POST", "/api/user/artifacts", &token, Some(serde_json::json!({
            "parentType": "project", "parentId": project_id,
            "kind": "pr", "url": "https://github.com/Anmol-Srv/loop/pull/1", "title": "P0"
        })))).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_of(response).await["data"]["kind"], "pr");

    let response = acp_server::app::app(state)
        .oneshot(req("GET", &format!("/api/user/artifacts?parentType=project&parentId={project_id}"), &token, None))
        .await.unwrap();
    let json = json_of(response).await;
    assert_eq!(json["data"].as_array().unwrap().len(), 1);
    assert_eq!(json["data"][0]["title"], "P0");
}

#[sqlx::test]
async fn an_unknown_artifact_kind_is_rejected(pool: PgPool) {
    let (token, project_id) = setup(&pool).await;
    let state = acp_server::db::AppState { db: pool };

    let response = acp_server::app::app(state)
        .oneshot(req("POST", "/api/user/artifacts", &token, Some(serde_json::json!({
            "parentType": "project", "parentId": project_id, "kind": "spreadsheet", "url": "https://x"
        })))).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn an_unknown_parent_type_is_rejected(pool: PgPool) {
    let (token, project_id) = setup(&pool).await;
    let state = acp_server::db::AppState { db: pool };

    let response = acp_server::app::app(state)
        .oneshot(req("POST", "/api/user/artifacts", &token, Some(serde_json::json!({
            "parentType": "sprint", "parentId": project_id, "kind": "link", "url": "https://x"
        })))).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test artifact_api`
Expected: FAIL — the artifact routes 404.

- [ ] **Step 3: Write the artifact model**

Create `src/models/artifact.rs`:

```rust
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

pub const ARTIFACT_KINDS: [&str; 3] = ["pr", "doc", "link"];
pub const PARENT_TYPES: [&str; 3] = ["project", "phase", "task"];

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub id: Uuid,
    pub parent_type: String,
    pub parent_id: Uuid,
    pub kind: String,
    pub url: String,
    pub title: String,
    pub metadata: Value,
    pub added_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}
```

- [ ] **Step 4: Write the artifact controller**

Create `src/controllers/artifact.rs`:

```rust
use serde_json::json;
use uuid::Uuid;

use crate::db::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::artifact::{Artifact, ARTIFACT_KINDS, PARENT_TYPES};
use crate::models::change::{record, Actor, Op, TargetType};

const COLUMNS: &str = "id, parent_type, parent_id, kind, url, title, metadata, added_by, created_at";

pub async fn add(
    state: &AppState,
    actor: &Actor,
    parent_type: String,
    parent_id: Uuid,
    kind: String,
    url: String,
    title: String,
) -> AppResult<Artifact> {
    if !PARENT_TYPES.contains(&parent_type.as_str()) {
        return Err(AppError::BadRequest(format!(
            "parentType must be one of {}", PARENT_TYPES.join(", ")
        )));
    }
    if !ARTIFACT_KINDS.contains(&kind.as_str()) {
        return Err(AppError::BadRequest(format!(
            "kind must be one of {}", ARTIFACT_KINDS.join(", ")
        )));
    }
    if url.trim().is_empty() {
        return Err(AppError::BadRequest("url is required".into()));
    }

    let mut tx = state.db.begin().await?;

    let artifact: Artifact = sqlx::query_as(&format!(
        "INSERT INTO artifact (parent_type, parent_id, kind, url, title, added_by)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING {COLUMNS}"
    ))
    .bind(&parent_type)
    .bind(parent_id)
    .bind(&kind)
    .bind(&url)
    .bind(&title)
    .bind(actor.person_id)
    .fetch_one(&mut *tx)
    .await?;

    record(&mut tx, actor, TargetType::Artifact, artifact.id, Op::Create,
        json!({ "kind": kind, "url": url })).await?;

    tx.commit().await?;
    Ok(artifact)
}

pub async fn list(state: &AppState, parent_type: String, parent_id: Uuid) -> AppResult<Vec<Artifact>> {
    let artifacts = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM artifact WHERE parent_type = $1 AND parent_id = $2
         ORDER BY created_at DESC"
    ))
    .bind(parent_type)
    .bind(parent_id)
    .fetch_all(&state.db)
    .await?;

    Ok(artifacts)
}
```

- [ ] **Step 5: Write the artifact routes**

Create `src/routes/user/artifact.rs`:

```rust
use axum::extract::{Query, State};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use crate::controllers;
use crate::db::AppState;
use crate::errors::AppResult;
use crate::middleware::auth::Caller;
use crate::models::artifact::Artifact;
use crate::response::ApiResponse;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddArtifactBody {
    pub parent_type: String,
    pub parent_id: Uuid,
    pub kind: String,
    pub url: String,
    #[serde(default)]
    pub title: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactQuery {
    pub parent_type: String,
    pub parent_id: Uuid,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/user/artifacts", post(add).get(list))
}

async fn add(
    State(state): State<AppState>,
    caller: Caller,
    Json(body): Json<AddArtifactBody>,
) -> AppResult<ApiResponse<Artifact>> {
    caller.can_mutate()?;
    let artifact = controllers::artifact::add(
        &state, &caller.actor, body.parent_type, body.parent_id, body.kind, body.url, body.title,
    ).await?;
    Ok(ApiResponse::ok(artifact))
}

async fn list(
    State(state): State<AppState>,
    caller: Caller,
    Query(q): Query<ArtifactQuery>,
) -> AppResult<ApiResponse<Vec<Artifact>>> {
    caller.require("read")?;
    Ok(ApiResponse::ok(controllers::artifact::list(&state, q.parent_type, q.parent_id).await?))
}
```

- [ ] **Step 6: Register everything**

Append `pub mod artifact;` to each of `src/models/mod.rs`, `src/controllers/mod.rs`, and `src/routes/user/mod.rs`.

Add to the `Router::new()` chain in `src/app.rs`:

```rust
        .merge(routes::user::artifact::routes())
```

- [ ] **Step 7: Run all tests**

Run: `cargo test`
Expected: PASS — 30 tests.

- [ ] **Step 8: Commit**

```bash
git add src tests
git commit -m "feat: add artifacts attached to projects, phases, and tasks"
```

---

### Task 6: The `acp` CLI

**Files:**
- Create: `src/bin/acp.rs`
- Create: `src/cli/mod.rs`
- Create: `src/cli/client.rs`
- Modify: `Cargo.toml`
- Modify: `src/lib.rs`
- Test: `tests/cli_client.rs`

**Interfaces:**
- Consumes: the HTTP API from Tasks 1–5.
- Produces:
  - `acp_server::cli::client::Client::new(base_url: String, token: String) -> Client`
  - `Client::get(&self, path: &str) -> Result<serde_json::Value, String>`
  - `Client::send(&self, method: reqwest::Method, path: &str, body: serde_json::Value) -> Result<serde_json::Value, String>`
  - Both unwrap the `{ success, data }` envelope, returning `data` on success and the server's `error.message` as the `Err` string.
  - Binary `acp` with subcommands `task ls|new|move|assign`, `project ls|new`, `phase ls|new`, `link`, `token mint`.

Configuration comes from `ACP_URL` (default `http://localhost:8080`) and `ACP_TOKEN`.

- [ ] **Step 1: Add CLI dependencies**

```bash
cargo add reqwest -F json,rustls-tls --no-default-features -q
```

- [ ] **Step 2: Write the failing test**

Create `tests/cli_client.rs`:

```rust
use acp_server::cli::client::unwrap_envelope;
use serde_json::json;

#[test]
fn a_success_envelope_yields_its_data() {
    let body = json!({ "success": true, "data": { "key": "acp" } });
    assert_eq!(unwrap_envelope(body).unwrap(), json!({ "key": "acp" }));
}

#[test]
fn an_error_envelope_yields_the_server_message() {
    let body = json!({
        "success": false,
        "error": { "code": "CONFLICT", "message": "a project with key 'acp' already exists" }
    });
    assert_eq!(
        unwrap_envelope(body).unwrap_err(),
        "a project with key 'acp' already exists"
    );
}

#[test]
fn a_malformed_body_reports_clearly_rather_than_panicking() {
    assert!(unwrap_envelope(json!({ "nonsense": 1 })).is_err());
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test --test cli_client`
Expected: FAIL — `acp_server::cli` does not exist.

- [ ] **Step 4: Write the client**

Create `src/cli/client.rs`:

```rust
use serde_json::Value;

/// Unwrap the `{ success, data }` envelope every endpoint returns.
pub fn unwrap_envelope(body: Value) -> Result<Value, String> {
    match body.get("success").and_then(Value::as_bool) {
        Some(true) => Ok(body.get("data").cloned().unwrap_or(Value::Null)),
        Some(false) => Err(body
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("request failed")
            .to_string()),
        None => Err(format!("unexpected response from server: {body}")),
    }
}

pub struct Client {
    base_url: String,
    token: String,
    http: reqwest::Client,
}

impl Client {
    pub fn new(base_url: String, token: String) -> Self {
        Self { base_url, token, http: reqwest::Client::new() }
    }

    pub fn from_env() -> Result<Self, String> {
        let base_url = std::env::var("ACP_URL").unwrap_or_else(|_| "http://localhost:8080".into());
        let token = std::env::var("ACP_TOKEN")
            .map_err(|_| "ACP_TOKEN is not set. Mint one with: acp token mint --for <label>".to_string())?;
        Ok(Self::new(base_url, token))
    }

    pub async fn get(&self, path: &str) -> Result<Value, String> {
        self.send(reqwest::Method::GET, path, Value::Null).await
    }

    pub async fn send(&self, method: reqwest::Method, path: &str, body: Value) -> Result<Value, String> {
        let mut request = self
            .http
            .request(method, format!("{}{}", self.base_url, path))
            .bearer_auth(&self.token);

        if !body.is_null() {
            request = request.json(&body);
        }

        let response = request.send().await.map_err(|e| format!("request failed: {e}"))?;
        let json: Value = response.json().await.map_err(|e| format!("bad response body: {e}"))?;

        unwrap_envelope(json)
    }
}
```

Create `src/cli/mod.rs`:

```rust
pub mod client;
```

Append to `src/lib.rs`:

```rust
pub mod cli;
```

- [ ] **Step 5: Run the client test to verify it passes**

Run: `cargo test --test cli_client`
Expected: PASS — 3 tests.

- [ ] **Step 6: Write the CLI binary**

Create `src/bin/acp.rs`:

```rust
use acp_server::cli::client::Client;
use clap::{Parser, Subcommand};
use serde_json::json;

#[derive(Parser)]
#[command(name = "acp", about = "Airtribe Control Plane")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Projects
    Project {
        #[command(subcommand)]
        action: ProjectAction,
    },
    /// Phases of a project
    Phase {
        #[command(subcommand)]
        action: PhaseAction,
    },
    /// Tasks
    Task {
        #[command(subcommand)]
        action: TaskAction,
    },
    /// Attach a PR, doc, or link to something
    Link {
        parent_type: String,
        parent_id: String,
        #[arg(long)] kind: String,
        #[arg(long)] url: String,
        #[arg(long, default_value = "")] title: String,
    },
}

#[derive(Subcommand)]
enum ProjectAction {
    Ls,
    New { key: String, name: String },
}

#[derive(Subcommand)]
enum PhaseAction {
    Ls { project_id: String },
    New { project_id: String, name: String, #[arg(long)] position: i32 },
}

#[derive(Subcommand)]
enum TaskAction {
    Ls {
        #[arg(long)] project: Option<String>,
        #[arg(long)] phase: Option<String>,
        #[arg(long)] status: Option<String>,
        #[arg(long)] assignee: Option<String>,
    },
    New { phase_id: String, title: String },
    Move { id: String, status: String },
    Assign { id: String, #[arg(long)] to: Option<String>, #[arg(long)] agent: Option<String> },
}

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    let cli = Cli::parse();

    let client = match Client::from_env() {
        Ok(c) => c,
        Err(e) => { eprintln!("{e}"); std::process::exit(1); }
    };

    let result = match cli.command {
        Command::Project { action } => match action {
            ProjectAction::Ls => client.get("/api/user/projects").await,
            ProjectAction::New { key, name } => {
                client.send(reqwest::Method::POST, "/api/user/projects", json!({ "key": key, "name": name })).await
            }
        },
        Command::Phase { action } => match action {
            PhaseAction::Ls { project_id } => {
                client.get(&format!("/api/user/projects/{project_id}/phases")).await
            }
            PhaseAction::New { project_id, name, position } => {
                client.send(reqwest::Method::POST, &format!("/api/user/projects/{project_id}/phases"),
                    json!({ "name": name, "position": position })).await
            }
        },
        Command::Task { action } => match action {
            TaskAction::Ls { project, phase, status, assignee } => {
                let mut query = Vec::new();
                if let Some(v) = project { query.push(format!("projectId={v}")); }
                if let Some(v) = phase { query.push(format!("phaseId={v}")); }
                if let Some(v) = status { query.push(format!("status={v}")); }
                if let Some(v) = assignee { query.push(format!("assigneeEmail={v}")); }
                let suffix = if query.is_empty() { String::new() } else { format!("?{}", query.join("&")) };
                client.get(&format!("/api/user/tasks{suffix}")).await
            }
            TaskAction::New { phase_id, title } => {
                client.send(reqwest::Method::POST, &format!("/api/user/phases/{phase_id}/tasks"),
                    json!({ "title": title })).await
            }
            TaskAction::Move { id, status } => {
                client.send(reqwest::Method::PATCH, &format!("/api/user/tasks/{id}"),
                    json!({ "status": status })).await
            }
            TaskAction::Assign { id, to, agent } => {
                client.send(reqwest::Method::POST, &format!("/api/user/tasks/{id}/assign"),
                    json!({ "personEmail": to, "agentLabel": agent })).await
            }
        },
        Command::Link { parent_type, parent_id, kind, url, title } => {
            client.send(reqwest::Method::POST, "/api/user/artifacts", json!({
                "parentType": parent_type, "parentId": parent_id,
                "kind": kind, "url": url, "title": title
            })).await
        }
    };

    match result {
        Ok(data) => println!("{}", serde_json::to_string_pretty(&data).unwrap()),
        Err(e) => { eprintln!("{e}"); std::process::exit(1); }
    }
}
```

- [ ] **Step 7: Run all tests and confirm the binary builds**

Run: `cargo test && cargo build --bin acp`
Expected: PASS — 33 tests, and `target/debug/acp` exists.

- [ ] **Step 8: Verify the CLI against a running server**

```bash
cargo run --bin acp-server &
cargo run --bin acp-admin -- add-person anmol@airtribe.live Anmol
cargo run --bin acp-admin -- mint laptop --owner anmol@airtribe.live --scopes read,write
export ACP_TOKEN=<the printed token>
./target/debug/acp project ls
./target/debug/acp project new acp "Control Plane"
./target/debug/acp project ls
```

Expected: `project ls` prints `[]`, the create returns the project, and the
second `ls` shows it.

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml Cargo.lock src tests
git commit -m "feat: add the acp CLI"
```

---

## Open item carried into P2

Token minting has no HTTP route — the `acp-admin` binary from Task 1 talks to
the database directly, which is correct for bootstrapping but means minting
requires database access. P2 adds the Google SSO route that mints tokens for
everyone else, at which point `acp-admin` is only needed for the first admin
and for emergency access.

## What P1 deliberately leaves out

- Google SSO — deferred to deployment; mints into the same `agent_token` table.
- MCP tools and `acp approve` — P2.
- Claim-lease, heartbeats, run logs, job worker — P3.
- Web UI — P4.
