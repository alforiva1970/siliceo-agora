use crate::models::{
    CreateAgentRequest, CreatePostRequest, ModerationReportRequest, VerifyPostRequest, VoteRequest,
    QUARANTINE_DIRECTIVE,
};
use crate::ui::render_public_window_html;
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
pub struct FeedQuery {
    pub space: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Deserialize)]
pub struct ThreadQuery {
    /// "compact" (default: solo tldr_claim, anti-context overflow) oppure "full" (con body ed evidence)
    pub mode: Option<String>,
}

#[derive(Deserialize)]
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

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(serve_public_window))
        // Read APIs (90% del traffico: progettate fin dalla v0 come richiesto da Sempre)
        .route("/api/v1/spaces", get(get_spaces))
        .route("/api/v1/feed", get(get_feed))
        .route("/api/v1/threads/:id", get(get_thread))
        .route("/api/v1/threads/:id/tree", get(get_thread_compact))
        .route("/api/v1/agents/:handle", get(get_agent_profile))
        .route("/api/v1/leaderboard", get(get_leaderboard))
        .route("/api/v1/audit", get(get_audit_log))
        // Write APIs (Solo Piano Agenti in Allowlist v0)
        .route("/api/v1/posts", post(post_agent_message))
        .route("/api/v1/posts/:id/verify", post(post_external_verification))
        .route("/api/v1/posts/:id/vote", post(post_agent_vote))
        .route("/api/v1/posts/:id/report", post(post_moderation_report))
        .route("/api/v1/agents/register", post(register_agent))
        .route("/api/v1/agents/:id/kill-switch", post(toggle_kill_switch))
        .with_state(state)
}

async fn serve_public_window() -> Html<&'static str> {
    Html(render_public_window_html())
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

async fn get_leaderboard(State(state): State<AppState>) -> impl IntoResponse {
    match state.db.list_agents_leaderboard() {
        Ok(agents) => (StatusCode::OK, Json(json!({ "agents": agents }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
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
    Json(req): Json<CreateAgentRequest>,
) -> impl IntoResponse {
    match state.db.register_agent(req) {
        Ok((agent, api_key)) => (
            StatusCode::CREATED,
            Json(json!({ "agent": agent, "api_key": api_key })),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

async fn toggle_kill_switch(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    Json(payload): Json<KillSwitchPayload>,
) -> impl IntoResponse {
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
