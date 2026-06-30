use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;

use crate::auth::{AccountStore, TokenIssuer};
use crate::cas::Cas;
use crate::manifest::ManifestStore;

#[derive(Clone)]
pub struct AppState {
    pub cas: Arc<Cas>,
    pub manifests: Arc<ManifestStore>,
    pub accounts: Arc<dyn AccountStore>,
    pub tokens: Arc<TokenIssuer>,
    /// Dev escape hatch: when true, skip all credential/token checks so tools can
    /// pull assets without the EQEmu login flow. NEVER enable in production.
    pub no_auth: bool,
}

#[derive(Deserialize)]
struct AuthReq {
    username: String,
    password: String,
}

/// True when the client's `If-None-Match` (quotes optional) equals the set's current digest, i.e.
/// the client already has this exact content and the server should answer `304 Not Modified`.
fn etag_matches(if_none_match: Option<&str>, digest: &str) -> bool {
    if_none_match.is_some_and(|inm| inm.trim().trim_matches('"') == digest)
}

fn bearer(headers: &HeaderMap, tokens: &TokenIssuer) -> Option<String> {
    let raw = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let token = raw.strip_prefix("Bearer ")?;
    tokens.verify(token)
}

async fn post_auth(State(st): State<AppState>, Json(req): Json<AuthReq>) -> Response {
    if st.no_auth || st.accounts.verify(&req.username, &req.password) {
        let token = st.tokens.issue(&req.username);
        Json(serde_json::json!({ "token": token })).into_response()
    } else {
        (StatusCode::UNAUTHORIZED, "invalid credentials").into_response()
    }
}

async fn get_manifest(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(set): Path<String>,
) -> Response {
    if !st.no_auth && bearer(&headers, &st.tokens).is_none() {
        return (StatusCode::UNAUTHORIZED, "missing/invalid token").into_response();
    }
    let Some(digest) = st.manifests.latest_digest(&set) else {
        return (StatusCode::NOT_FOUND, "no such manifest").into_response();
    };
    // Conditional GET: identical content the client already has → 304, no body.
    let inm = headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok());
    if etag_matches(inm, &digest) {
        return StatusCode::NOT_MODIFIED.into_response();
    }
    match st.manifests.load_latest(&set) {
        Ok(m) => ([(header::ETAG, format!("\"{digest}\""))], Json(m)).into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "no such manifest").into_response(),
    }
}

async fn get_chunk(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(hash): Path<String>,
) -> Response {
    if !st.no_auth && bearer(&headers, &st.tokens).is_none() {
        return (StatusCode::UNAUTHORIZED, "missing/invalid token").into_response();
    }
    match st.cas.get(&hash) {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, "application/octet-stream"),
                (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
            ],
            Body::from(bytes),
        )
            .into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "no such chunk").into_response(),
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/auth", post(post_auth))
        .route("/manifest/*set", get(get_manifest))
        .route("/chunk/:hash", get(get_chunk))
        .with_state(state)
}

pub async fn serve(state: AppState, addr: SocketAddr) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("asset server listening on {addr}");
    axum::serve(listener, router(state)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::etag_matches;

    #[test]
    fn etag_matches_with_and_without_quotes() {
        assert!(etag_matches(Some("\"abc\""), "abc"));
        assert!(etag_matches(Some("abc"), "abc"));
        assert!(etag_matches(Some("  \"abc\" "), "abc"));
    }

    #[test]
    fn etag_no_match_when_stale_or_absent() {
        assert!(!etag_matches(Some("\"stale\""), "abc"));
        assert!(!etag_matches(None, "abc"));
    }
}
