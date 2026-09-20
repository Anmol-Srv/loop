use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};

use crate::db::AppState;
use crate::middleware::auth::{resolve, Caller};

pub const SESSION_COOKIE: &str = "acp_session";

/// The same `Caller` the API uses, found through a cookie instead of a header.
/// Resolution goes through `auth::resolve`, so scope truth is identical.
pub struct WebCaller(pub Caller);

/// A browser needs a page, not a status code, so an unauthenticated page
/// request lands on the login form rather than a 401 body.
pub struct ToLogin;

impl IntoResponse for ToLogin {
    fn into_response(self) -> Response {
        Redirect::to("/login").into_response()
    }
}

/// The cookie holds a token, not an identity, so every request re-resolves it.
/// Revoking the token ends the session on the next click.
pub async fn session_caller(state: &AppState, jar: &CookieJar) -> Option<Caller> {
    let raw = jar.get(SESSION_COOKIE)?.value();
    resolve(&state.db, raw).await.ok()
}

impl FromRequestParts<AppState> for WebCaller {
    type Rejection = ToLogin;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_headers(&parts.headers);
        session_caller(state, &jar).await.map(WebCaller).ok_or(ToLogin)
    }
}

/// True when the request reached us over HTTPS. Behind a proxy that is the only
/// thing we can see; plain local http stays insecure so dev logins work.
pub fn is_https(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("https"))
}

fn base_cookie(value: String, secure: bool) -> Cookie<'static> {
    let mut c = Cookie::new(SESSION_COOKIE, value);
    c.set_http_only(true);
    c.set_same_site(SameSite::Strict);
    c.set_path("/");
    c.set_secure(secure);
    c
}

pub fn set_session(jar: CookieJar, token: String, secure: bool) -> CookieJar {
    jar.add(base_cookie(token, secure))
}

pub fn clear_session(jar: CookieJar, secure: bool) -> CookieJar {
    jar.remove(base_cookie(String::new(), secure))
}
