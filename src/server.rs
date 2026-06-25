use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path, Query, State},
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

#[derive(Deserialize)]
struct VersionQuery {
    version: Option<u64>,
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
    Query(q): Query<VersionQuery>,
) -> Response {
    if !st.no_auth && bearer(&headers, &st.tokens).is_none() {
        return (StatusCode::UNAUTHORIZED, "missing/invalid token").into_response();
    }
    let result = match q.version {
        Some(v) => st.manifests.load(&set, v),
        None => st.manifests.load_latest(&set),
    };
    match result {
        Ok(m) => Json(m).into_response(),
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
