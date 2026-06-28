use axum::extract::Path;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

// ─── Types ──────────────────────────────────────────────────────

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SessionInfo {
    pub session_id: String,
    pub instructions: Vec<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SetInstructionsRequest {
    pub instructions: Vec<String>,
}

// ─── Handlers ───────────────────────────────────────────────────

/// GET /sessions/{session_id} — get session instructions
#[utoipa::path(
    get,
    path = "/sessions/{session_id}",
    params(
        ("session_id" = String, Path, description = "Session ID"),
    ),
    responses(
        (status = 200, description = "Session instructions", body = SessionInfo),
        (status = 404, description = "Session not found"),
    ),
    tag = "sessions"
)]
pub async fn get_session(
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    match crate::http::get_session_instructions(&session_id) {
        Some(instructions) => {
            let info = SessionInfo {
                session_id,
                instructions,
            };
            (axum::http::StatusCode::OK, Json(serde_json::json!({
                "status": "ok",
                "data": info,
            }))).into_response()
        }
        None => (axum::http::StatusCode::NOT_FOUND, Json(serde_json::json!({
            "status": "error",
            "error": "Session not found",
            "error_code": "session_not_found",
        }))).into_response(),
    }
}

/// PUT /sessions/{session_id} — set session instructions
#[utoipa::path(
    put,
    path = "/sessions/{session_id}",
    params(
        ("session_id" = String, Path, description = "Session ID"),
    ),
    request_body = SetInstructionsRequest,
    responses(
        (status = 200, description = "Instructions updated"),
        (status = 400, description = "Validation error"),
    ),
    tag = "sessions"
)]
pub async fn set_session(
    Path(session_id): Path<String>,
    Json(req): Json<SetInstructionsRequest>,
) -> impl IntoResponse {
    if req.instructions.len() > 100 {
        return (axum::http::StatusCode::BAD_REQUEST, Json(serde_json::json!({
            "status": "error",
            "error": "Too many system instructions (max 100)",
            "error_code": "validation_error",
        }))).into_response();
    }
    let total_len: usize = req.instructions.iter().map(|s| s.len()).sum();
    if total_len > 500_000 {
        return (axum::http::StatusCode::BAD_REQUEST, Json(serde_json::json!({
            "status": "error",
            "error": "System instructions too long (max 500KB)",
            "error_code": "validation_error",
        }))).into_response();
    }

    if req.instructions.is_empty() {
        crate::http::remove_session(&session_id);
    } else {
        crate::http::insert_session(session_id.clone(), req.instructions);
    }

    (axum::http::StatusCode::OK, Json(serde_json::json!({
        "status": "ok",
        "session_id": session_id,
    }))).into_response()
}

/// DELETE /sessions/{session_id} — delete session instructions
#[utoipa::path(
    delete,
    path = "/sessions/{session_id}",
    params(
        ("session_id" = String, Path, description = "Session ID"),
    ),
    responses(
        (status = 200, description = "Session deleted"),
    ),
    tag = "sessions"
)]
pub async fn delete_session(
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    crate::http::remove_session(&session_id);
    Json(serde_json::json!({
        "status": "ok",
        "session_id": session_id,
    }))
}
