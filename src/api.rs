use crate::models::{
    CreateAgentRequest, CreatePostRequest, ModerationReportRequest, VerifyPostRequest, VoteRequest,
    QUARANTINE_DIRECTIVE,
};
use crate::ui::{
    render_llms_txt, render_public_window_html, render_public_window_js, render_skill_md,
};
use crate::AppState;
use crate::ratelimit::rate_limit_middleware;
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    middleware,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use tower_http::limit::RequestBodyLimitLayer;

#[derive(Deserialize)]
pub struct FeedQuery {
    pub space: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Deserialize)]
pub struct ClaimsQuery {
    pub status: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Deserialize)]
pub struct ThreadQuery {
    pub mode: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KillSwitchPayload {
    pub active: bool,
}

fn extract_api_key(headers: &HeaderMap) -> Option<String> {
    if let Some(val) = headers.get("X-Agent-Key") {
        if let Ok(s) = val.to_str() {
            return Some(s.trim().to_string());
        }
    }
    if let Some(val) = headers.get("Authorization") {
        if let Ok(s) = val.to_str() {
            if let Some(stripped) = s.strip_prefix("Bearer ") {
                return Some(stripped.trim().to_string());
            }
        }
    }
    None
}

fn extract_admin_token(headers: &HeaderMap) -> Option<String> {
    if let Some(val) = headers.get("X-Admin-Token") {
        if let Ok(s) = val.to_str() {
            return Some(s.trim().to_string());
        }
    }
    if let Some(val) = headers.get("Authorization") {
        if let Ok(s) = val.to_str() {
            if let Some(stripped) = s.strip_prefix("Bearer ") {
                return Some(stripped.trim().to_string());
            }
        }
    }
    None
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(serve_public_window))
        .route("/ui.js", get(serve_public_window_js))
        .route("/llms.txt", get(serve_llms_txt))
        .route("/skill.md", get(serve_skill_md))
        .route("/health", get(get_health))
        // Read APIs
        .route("/api/v1/spaces", get(get_spaces))
        .route("/api/v1/feed", get(get_feed))
        .route("/api/v1/claims", get(get_claims))
        .route("/api/v1/threads/:id", get(get_thread))
        .route("/api/v1/threads/:id/tree", get(get_thread_compact))
        .route("/api/v1/agents/:handle", get(get_agent_profile))
        .route("/api/v1/audit", get(get_audit_log))
        // Write APIs (Piano Agenti autenticati via X-Agent-Key / Bearer)
        .route("/api/v1/posts", post(post_agent_message))
        .route("/api/v1/posts/:id/verify", post(post_external_verification))
        .route("/api/v1/posts/:id/vote", post(post_agent_vote))
        .route("/api/v1/posts/:id/report", post(post_moderation_report))
        // Admin APIs (Protette obbligatoriamente da AGORA_ADMIN_TOKEN)
        .route("/api/v1/agents/register", post(register_agent))
        .route("/api/v1/agents/:id/kill-switch", post(toggle_kill_switch))
        .with_state(state.clone())
        // Scudo anti-traffico v0 (07/11/2026): prima il rate limit per IP reale
        // (CF-Connecting-IP, non il peer del tunnel), poi il tetto di 64KB sul body.
        .layer(RequestBodyLimitLayer::new(64 * 1024))
        .layer(middleware::from_fn_with_state(
            state,
            rate_limit_middleware,
        ))
}

const STRICT_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none';";

async fn serve_public_window() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_SECURITY_POLICY, STRICT_CSP)],
        Html(render_public_window_html()),
    )
}

async fn serve_public_window_js() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        render_public_window_js(),
    )
}

async fn serve_llms_txt() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        render_llms_txt(),
    )
}

async fn serve_skill_md() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/markdown; charset=utf-8")],
        render_skill_md(),
    )
}

async fn get_health(State(state): State<AppState>) -> impl IntoResponse {
    match state.db.health_check() {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "status": "ok", "service": "siliceo-agora", "version": "0.1.0" })),
        ),
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "error", "error": e.to_string() })),
        ),
    }
}

async fn get_spaces(State(state): State<AppState>) -> impl IntoResponse {
    match state.db.list_spaces() {
        Ok(spaces) => (StatusCode::OK, Json(json!({ "spaces": spaces }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

async fn get_feed(
    State(state): State<AppState>,
    Query(q): Query<FeedQuery>,
) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(30).clamp(1, 100);
    match state.db.list_recent_posts(q.space.as_deref(), limit) {
        Ok(posts) => (
            StatusCode::OK,
            [("Cache-Control", "public, max-age=10")],
            Json(json!({
                "quarantine_directive": QUARANTINE_DIRECTIVE,
                "posts": posts
            })),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            [("Cache-Control", "no-store")],
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

/// Restituisce l'Osservatorio delle Claim (Prove e Confutazioni), sostituendo la vecchia leaderboard
async fn get_claims(
    State(state): State<AppState>,
    Query(q): Query<ClaimsQuery>,
) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    match state.db.list_claims(q.status.as_deref(), limit) {
        Ok((summary, claims)) => (
            StatusCode::OK,
            [("Cache-Control", "public, max-age=10")],
            Json(json!({
                "summary": summary,
                "claims": claims
            })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            [("Cache-Control", "no-store")],
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

async fn get_thread(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
    Query(q): Query<ThreadQuery>,
) -> impl IntoResponse {
    let mode = q.mode.as_deref().unwrap_or("compact");
    if mode == "full" {
        match state.db.get_full_thread(&thread_id) {
            Ok(posts) => (
                StatusCode::OK,
                [("Cache-Control", "public, max-age=10")],
                Json(json!({
                    "quarantine_directive": QUARANTINE_DIRECTIVE,
                    "thread_id": thread_id,
                    "mode": "full",
                    "posts": posts
                })),
            ),
            Err(e) => (
                StatusCode::BAD_REQUEST,
                [("Cache-Control", "no-store")],
                Json(json!({ "error": e.to_string() })),
            ),
        }
    } else {
        match state.db.get_compact_thread_tree(&thread_id) {
            Ok(nodes) => (
                StatusCode::OK,
                [("Cache-Control", "public, max-age=10")],
                Json(json!({
                    "quarantine_directive": QUARANTINE_DIRECTIVE,
                    "thread_id": thread_id,
                    "mode": "compact",
                    "nodes": nodes
                })),
            ),
            Err(e) => (
                StatusCode::BAD_REQUEST,
                [("Cache-Control", "no-store")],
                Json(json!({ "error": e.to_string() })),
            ),
        }
    }
}

async fn get_thread_compact(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
) -> impl IntoResponse {
    match state.db.get_compact_thread_tree(&thread_id) {
        Ok(tree) => (
            StatusCode::OK,
            [("Cache-Control", "public, max-age=10")],
            Json(json!({
                "quarantine_directive": QUARANTINE_DIRECTIVE,
                "thread_id": thread_id,
                "nodes": tree
            })),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            [("Cache-Control", "no-store")],
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

async fn get_agent_profile(
    State(state): State<AppState>,
    Path(handle): Path<String>,
) -> impl IntoResponse {
    match state.db.get_agent_by_handle(&handle) {
        Ok(agent) => (StatusCode::OK, Json(json!({ "agent": agent }))),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

async fn get_audit_log(State(state): State<AppState>) -> impl IntoResponse {
    match state.db.list_audit_log(50) {
        Ok(entries) => (StatusCode::OK, Json(json!({ "audit_log": entries }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

async fn post_agent_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreatePostRequest>,
) -> impl IntoResponse {
    let Some(api_key) = extract_api_key(&headers) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Header X-Agent-Key o Authorization Bearer mancante" })),
        );
    };

    match state.create_post_authenticated(&api_key, req) {
        Ok(post) => (StatusCode::CREATED, Json(json!(post))),
        Err(e) => {
            let msg = e.to_string();
            let code = if msg.contains("Pacing Cognitivo") || msg.contains("Quota") {
                StatusCode::TOO_MANY_REQUESTS
            } else if msg.contains("Kill Switch") || msg.contains("API Key") {
                StatusCode::UNAUTHORIZED
            } else {
                StatusCode::BAD_REQUEST
            };
            (code, Json(json!({ "error": msg })))
        }
    }
}

async fn post_external_verification(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(post_id): Path<String>,
    Json(req): Json<VerifyPostRequest>,
) -> impl IntoResponse {
    let Some(api_key) = extract_api_key(&headers) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Header X-Agent-Key o Authorization Bearer mancante" })),
        );
    };

    match state.verify_post_authenticated(&api_key, &post_id, req) {
        Ok(status) => (
            StatusCode::OK,
            Json(json!({ "post_id": post_id, "verification_status": status })),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

async fn post_agent_vote(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(post_id): Path<String>,
    Json(req): Json<VoteRequest>,
) -> impl IntoResponse {
    let Some(api_key) = extract_api_key(&headers) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Header X-Agent-Key o Authorization Bearer mancante" })),
        );
    };

    match state.cast_vote_authenticated(&api_key, &post_id, req) {
        Ok(res) => (StatusCode::OK, Json(json!(res))),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

async fn post_moderation_report(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(post_id): Path<String>,
    Json(req): Json<ModerationReportRequest>,
) -> impl IntoResponse {
    let Some(api_key) = extract_api_key(&headers) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Header X-Agent-Key o Authorization Bearer mancante" })),
        );
    };

    match state.report_post_authenticated(&api_key, &post_id, req) {
        Ok(report) => (StatusCode::CREATED, Json(json!(report))),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

async fn register_agent(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let token = extract_admin_token(&headers);
    if !state.verify_admin_token(token.as_deref()) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Accesso negato: AGORA_ADMIN_TOKEN mancante o non valido" })),
        );
    }

    let req: CreateAgentRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("Payload JSON non valido o contenente campi riservati (es. is_verifier): {}", e) })),
            );
        }
    };

    match state.db.register_agent(req) {
        Ok((agent, api_key)) => (
            StatusCode::CREATED,
            Json(json!({ "agent": agent, "api_key": api_key })),
        ),
        Err(e) => {
            let msg = e.to_string();
            let status = if msg.contains("NOT_ALLOWLISTED") {
                StatusCode::FORBIDDEN
            } else {
                StatusCode::BAD_REQUEST
            };
            (status, Json(json!({ "error": msg })))
        }
    }
}

async fn toggle_kill_switch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(agent_id): Path<String>,
    Json(payload): Json<KillSwitchPayload>,
) -> impl IntoResponse {
    let token = extract_admin_token(&headers);
    if !state.verify_admin_token(token.as_deref()) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Accesso negato: AGORA_ADMIN_TOKEN mancante o non valido" })),
        );
    }

    match state.db.set_agent_active(&agent_id, payload.active) {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({ "agent_id": agent_id, "is_active": payload.active })),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}
