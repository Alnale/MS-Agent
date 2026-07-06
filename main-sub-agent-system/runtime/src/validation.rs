use axum::extract::FromRequest;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use agent_core::processor::{detect_injection, sanitize_user_input, InjectionRisk};

use crate::http::ChatRequest;

// ─── Validation errors ──────────────────────────────────────────

#[derive(Debug)]
pub enum ValidationError {
    EmptyMessage,
    InjectionDetected { pattern: String, risk: InjectionRisk },
    TooManyInstructions(usize),
    InstructionsTooLarge(usize),
    TooManyHistoryEntries(usize),
}

impl IntoResponse for ValidationError {
    fn into_response(self) -> Response {
        let (code, msg) = match self {
            ValidationError::EmptyMessage => ("empty_message", "Message must not be empty"),
            ValidationError::InjectionDetected { risk, .. } => match risk {
                InjectionRisk::High => {
                    ("injection_detected", "Request rejected due to potential prompt injection")
                }
                InjectionRisk::Medium => {
                    ("injection_suspicious", "Suspicious input pattern detected")
                }
            },
            ValidationError::TooManyInstructions(_) => {
                ("validation_error", "Too many system instructions (max 100)")
            }
            ValidationError::InstructionsTooLarge(_) => {
                ("validation_error", "System instructions too long (max 500KB)")
            }
            ValidationError::TooManyHistoryEntries(_) => {
                ("validation_error", "Too many history entries (max 200)")
            }
        };

        let body = serde_json::json!({
            "status": "error",
            "error": msg,
            "error_code": code,
        });
        (StatusCode::BAD_REQUEST, axum::Json(body)).into_response()
    }
}

// ─── Validation logic ───────────────────────────────────────────

const MAX_MESSAGE_LENGTH: usize = 100_000;
const MAX_INSTRUCTIONS_COUNT: usize = 100;
const MAX_INSTRUCTIONS_SIZE: usize = 500_000;
const MAX_HISTORY_ENTRIES: usize = 200;

/// Validate and sanitize a ChatRequest in place.
/// Returns Ok(()) on success, Err(ValidationError) on failure.
pub fn validate_chat_request(req: &mut ChatRequest) -> Result<(), ValidationError> {
    // Empty message check
    if req.message.trim().is_empty() {
        return Err(ValidationError::EmptyMessage);
    }

    // Injection detection
    if let Some((pattern, risk)) = detect_injection(&req.message) {
        match risk {
            InjectionRisk::High => {
                return Err(ValidationError::InjectionDetected { pattern, risk });
            }
            InjectionRisk::Medium => {
                tracing::warn!(
                    "Suspicious prompt injection detected: pattern='{}', session={:?}",
                    pattern,
                    req.session_id
                );
            }
        }
    }

    // Sanitize message
    req.message = sanitize_user_input(&req.message, MAX_MESSAGE_LENGTH);

    // Validate system instructions
    if let Some(ref instructions) = req.system_instructions {
        if instructions.len() > MAX_INSTRUCTIONS_COUNT {
            return Err(ValidationError::TooManyInstructions(instructions.len()));
        }
        let total_len: usize = instructions.iter().map(|s| s.len()).sum();
        if total_len > MAX_INSTRUCTIONS_SIZE {
            return Err(ValidationError::InstructionsTooLarge(total_len));
        }
    }

    // Validate history
    if let Some(ref history) = req.recent_history {
        if history.len() > MAX_HISTORY_ENTRIES {
            return Err(ValidationError::TooManyHistoryEntries(history.len()));
        }
    }

    Ok(())
}

// ─── Typed extractor ────────────────────────────────────────────

/// Validated chat request — extracted via FromRequest with built-in validation.
/// Use this instead of `Json<ChatRequest>` to get automatic validation.
pub struct ValidatedChatRequest(pub ChatRequest);

impl<S: Send + Sync> FromRequest<S> for ValidatedChatRequest
where
    axum::Json<ChatRequest>: FromRequest<S>,
{
    type Rejection = Response;

    async fn from_request(
        req: axum::extract::Request,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let axum::Json(mut chat_req) = axum::Json::<ChatRequest>::from_request(req, state)
            .await
            .map_err(|e| e.into_response())?;

        validate_chat_request(&mut chat_req).map_err(|e| e.into_response())?;

        Ok(ValidatedChatRequest(chat_req))
    }
}
