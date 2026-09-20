use askama::Template;
use axum::extract::{Path, State};
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Form, Router};
use axum_extra::extract::CookieJar;
use serde::Deserialize;

use crate::db::AppState;
use crate::middleware::auth::resolve;
use crate::middleware::session::{clear_session, is_https, session_caller, set_session};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/login", get(login_form).post(login))
        .route("/logout", post(logout))
        .route("/static/{file}", get(static_file))
}

/// Every web handler renders through here. A template that fails to render is a
/// bug in the template, not something a user can provoke.
pub fn page(template: &impl Template) -> Response {
    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "template render failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "template error").into_response()
        }
    }
}

#[derive(Template)]
#[template(path = "login.html")]
struct LoginTemplate {
    error: Option<String>,
}

#[derive(Deserialize)]
pub struct LoginForm {
    token: String,
}

async fn login_form(State(state): State<AppState>, jar: CookieJar) -> Response {
    if session_caller(&state, &jar).await.is_some() {
        return Redirect::to("/").into_response();
    }
    page(&LoginTemplate { error: None })
}

async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
    Form(form): Form<LoginForm>,
) -> Response {
    match resolve(&state.db, form.token.trim()).await {
        Ok(_) => {
            let jar = set_session(jar, form.token.trim().to_string(), is_https(&headers));
            (jar, Redirect::to("/")).into_response()
        }
        Err(_) => page(&LoginTemplate {
            error: Some("That token is not valid.".into()),
        }),
    }
}

async fn logout(headers: HeaderMap, jar: CookieJar) -> Response {
    (clear_session(jar, is_https(&headers)), Redirect::to("/login")).into_response()
}

async fn static_file(Path(file): Path<String>) -> Response {
    // ponytail: two assets, embedded so the binary ships alone. A third one
    // means reaching for tower-http's ServeDir.
    let (body, mime) = match file.as_str() {
        "htmx.min.js" => (include_str!("../../../static/htmx.min.js"), "application/javascript"),
        "app.css" => (include_str!("../../../static/app.css"), "text/css"),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    ([(CONTENT_TYPE, mime)], body).into_response()
}
