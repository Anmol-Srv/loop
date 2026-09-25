//! `/api/agent/*`: the routes a personal agent calls with its own token.
//!
//! Authorised by the agent and what is delegated to it, never by scope — an
//! agent credential holds none. A person's session is refused with a
//! sentence. Errors are read by models, so they say what to do next.

use axum::extract::{Path, Query, State};
use axum::http::header::CONTENT_TYPE;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::controllers::agent::{self, Event};
use crate::controllers::note::Note;
use crate::db::AppState;
use crate::errors::AppResult;
use crate::middleware::auth::Caller;
use crate::models::artifact::Artifact;
use crate::models::task::TaskRow;
use crate::response::ApiResponse;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/agent/hello", post(hello))
        .route("/api/agent/me", get(me))
        .route("/api/agent/tasks", get(tasks))
        .route("/api/agent/tasks/{id}", get(context))
        .route("/api/agent/tasks/{id}/ack", post(ack))
        .route("/api/agent/tasks/{id}/update", post(update))
        .route("/api/agent/tasks/{id}/ask", post(ask))
        .route("/api/agent/tasks/{id}/attach", post(attach))
        .route("/api/agent/tasks/{id}/note", post(note))
        .route("/api/agent/tasks/{id}/submit", post(submit))
        .route("/api/agent/tasks/{id}/now", post(now))
        .route("/api/agent/tasks/{id}/log", post(log))
        .route("/api/agent/events", get(events))
        .route("/api/agent/events/ack", post(ack_events))
        .route("/api/agent/inbox", get(inbox))
        .route("/api/agent/skill", get(skill))
        .route("/api/agent/onboarding", get(onboarding))
        .route("/api/agent/intake", post(intake))
        .route("/api/agent/intake/recent", get(intake_recent))
        .route("/api/agent/intake/{id}/append", post(intake_append))
}

/// Where agents reach this server: `PUBLIC_URL` when set, otherwise what the
/// request came in on. Behind a proxy that sets neither header correctly,
/// set `PUBLIC_URL`.
pub fn server_url(headers: &HeaderMap) -> String {
    if let Ok(url) = std::env::var("PUBLIC_URL") {
        if !url.trim().is_empty() {
            return url.trim().trim_end_matches('/').to_owned();
        }
    }
    let header = |k: &str| headers.get(k).and_then(|v| v.to_str().ok());
    let host = header("x-forwarded-host").or(header("host")).unwrap_or("localhost:8080");
    let proto = header("x-forwarded-proto").unwrap_or("http");
    format!("{proto}://{host}")
}

fn text(content_type: &'static str, body: String) -> Response {
    ([(CONTENT_TYPE, content_type)], body).into_response()
}

#[derive(Deserialize)]
pub struct HelloBody {
    #[serde(default)]
    pub runtime: Option<String>,
    /// What setup installed; shown to the owner as a checklist.
    #[serde(default)]
    pub setup: Option<agent::Setup>,
    // `version` is accepted and ignored, like any unknown field.
}

async fn hello(
    State(state): State<AppState>,
    caller: Caller,
    headers: HeaderMap,
    body: Option<Json<HelloBody>>,
) -> AppResult<ApiResponse<Value>> {
    let id = caller.agent()?;
    let (runtime, setup) = body.map(|Json(b)| (b.runtime, b.setup)).unwrap_or_default();
    Ok(ApiResponse::ok(agent::hello(&state, id, runtime.as_deref(), setup, &server_url(&headers)).await?))
}

async fn me(State(state): State<AppState>, caller: Caller, headers: HeaderMap) -> AppResult<ApiResponse<Value>> {
    Ok(ApiResponse::ok(agent::me(&state, caller.agent()?, &server_url(&headers)).await?))
}

async fn tasks(State(state): State<AppState>, caller: Caller) -> AppResult<ApiResponse<Vec<TaskRow>>> {
    Ok(ApiResponse::ok(agent::tasks(&state, caller.agent()?).await?))
}

async fn context(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
) -> AppResult<ApiResponse<Value>> {
    Ok(ApiResponse::ok(agent::context(&state, caller.agent()?, id).await?))
}

async fn ack(State(state): State<AppState>, Path(id): Path<Uuid>, caller: Caller) -> AppResult<ApiResponse<TaskRow>> {
    Ok(ApiResponse::ok(agent::ack(&state, caller.agent()?, id).await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBody {
    pub body: String,
    #[serde(default)]
    pub status: Option<String>,
    /// The status the agent last read. When it is no longer true the move is
    /// refused with a 409 naming who moved it.
    #[serde(default)]
    pub expected_status: Option<String>,
    /// A new now line, set with the update.
    #[serde(default)]
    pub now: Option<String>,
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(b): Json<UpdateBody>,
) -> AppResult<ApiResponse<TaskRow>> {
    Ok(ApiResponse::ok(
        agent::update(&state, caller.agent()?, id, &b.body, b.status, b.expected_status, b.now.as_deref()).await?,
    ))
}

#[derive(Deserialize)]
pub struct BodyOnly {
    pub body: String,
}

async fn ask(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(b): Json<BodyOnly>,
) -> AppResult<ApiResponse<TaskRow>> {
    Ok(ApiResponse::ok(agent::ask(&state, caller.agent()?, id, &b.body).await?))
}

async fn note(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(b): Json<BodyOnly>,
) -> AppResult<ApiResponse<Note>> {
    Ok(ApiResponse::ok(agent::note(&state, caller.agent()?, id, &b.body).await?))
}

#[derive(Deserialize)]
pub struct NowBody {
    pub text: String,
}

async fn now(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(b): Json<NowBody>,
) -> AppResult<ApiResponse<TaskRow>> {
    Ok(ApiResponse::ok(agent::now(&state, caller.agent()?, id, &b.text).await?))
}

#[derive(Deserialize)]
pub struct LogBody {
    pub lines: Vec<String>,
}

async fn log(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(b): Json<LogBody>,
) -> AppResult<ApiResponse<Value>> {
    Ok(ApiResponse::ok(agent::log(&state, caller.agent()?, id, &b.lines).await?))
}

#[derive(Deserialize)]
pub struct AttachBody {
    pub kind: String,
    pub url: String,
    #[serde(default)]
    pub title: String,
}

async fn attach(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(b): Json<AttachBody>,
) -> AppResult<ApiResponse<Artifact>> {
    Ok(ApiResponse::ok(agent::attach(&state, caller.agent()?, id, &b.kind, &b.url, &b.title).await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitBody {
    pub target: String,
    pub summary: String,
    #[serde(default)]
    pub manual_reason: Option<String>,
}

async fn submit(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(b): Json<SubmitBody>,
) -> AppResult<ApiResponse<TaskRow>> {
    Ok(ApiResponse::ok(
        agent::submit(&state, caller.agent()?, id, &b.target, &b.summary, b.manual_reason).await?,
    ))
}

#[derive(Deserialize)]
pub struct EventsQuery {
    pub after: Option<i64>,
    #[serde(default)]
    pub wait: u64,
}

async fn events(
    State(state): State<AppState>,
    caller: Caller,
    Query(q): Query<EventsQuery>,
) -> AppResult<ApiResponse<Vec<Event>>> {
    Ok(ApiResponse::ok(agent::events(&state, caller.agent()?, q.after, q.wait).await?))
}

#[derive(Deserialize)]
pub struct AckEventsBody {
    pub through: i64,
}

async fn ack_events(
    State(state): State<AppState>,
    caller: Caller,
    Json(b): Json<AckEventsBody>,
) -> AppResult<ApiResponse<Value>> {
    let cursor = agent::ack_events(&state, caller.agent()?, b.through).await?;
    Ok(ApiResponse::ok(serde_json::json!({ "eventCursor": cursor })))
}

async fn inbox(State(state): State<AppState>, caller: Caller) -> AppResult<Response> {
    Ok(text("text/plain; charset=utf-8", agent::inbox(&state, caller.agent()?).await?))
}

#[derive(Deserialize)]
pub struct SkillQuery {
    pub name: Option<String>,
}

async fn skill(
    State(state): State<AppState>,
    caller: Caller,
    headers: HeaderMap,
    Query(q): Query<SkillQuery>,
) -> AppResult<Response> {
    let body = agent::skill(&state, caller.agent()?, q.name.as_deref(), &server_url(&headers)).await?;
    Ok(text("text/markdown; charset=utf-8", body))
}

#[derive(Deserialize)]
pub struct OnboardingQuery {
    pub runtime: Option<String>,
}

async fn onboarding(
    State(state): State<AppState>,
    caller: Caller,
    headers: HeaderMap,
    Query(q): Query<OnboardingQuery>,
) -> AppResult<Response> {
    let body =
        agent::onboarding(&state, caller.agent()?, q.runtime.as_deref(), &server_url(&headers)).await?;
    Ok(text("text/markdown; charset=utf-8", body))
}

async fn intake(
    State(state): State<AppState>,
    caller: Caller,
    Json(b): Json<agent::Intake>,
) -> AppResult<ApiResponse<TaskRow>> {
    Ok(ApiResponse::ok(agent::intake(&state, caller.agent()?, b).await?))
}

#[derive(Deserialize)]
pub struct AppendBody {
    pub source: agent::Source,
    /// What the new message adds; the source text when empty.
    #[serde(default)]
    pub text: String,
}

async fn intake_append(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    caller: Caller,
    Json(b): Json<AppendBody>,
) -> AppResult<ApiResponse<Note>> {
    Ok(ApiResponse::ok(agent::intake_append(&state, caller.agent()?, id, b.source, &b.text).await?))
}

#[derive(Deserialize)]
pub struct RecentQuery {
    #[serde(default = "fourteen")]
    pub days: i64,
}

fn fourteen() -> i64 {
    14
}

async fn intake_recent(
    State(state): State<AppState>,
    caller: Caller,
    Query(q): Query<RecentQuery>,
) -> AppResult<ApiResponse<Vec<agent::Filed>>> {
    Ok(ApiResponse::ok(agent::intake_recent(&state, caller.agent()?, q.days).await?))
}
