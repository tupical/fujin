//! fujin-server — thin, independently-deployed HTTP/MCP wrapper around the
//! `fujin` actions lib. Its own deploy unit (own systemd service, own port).
//! Boundary-clean: no mcpbox dependency; the platform→tool auth contract is a
//! configured shared key (see `auth`).
//!
//! Routes:
//!   GET  /healthz   — open; liveness + version for the platform registry.
//!   POST /v1/mcp    — requires a valid platform token; actions surface
//!                     (`fujin.pack` builds a typed ActionPacket via the lib).
//!
//! Env: FUJIN_PORT (default 8094), FUJIN_PLATFORM_SECRET (HMAC key; if unset,
//! /v1/mcp is closed), FUJIN_VERSION (defaults to the crate version).

mod auth;

use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use fujin::{ActionPacket, LinkedItem};
use serde_json::json;

const TOOL: &str = "fujin";

struct AppState {
    version: String,
    platform_secret: Option<Vec<u8>>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().json().init();

    let version =
        std::env::var("FUJIN_VERSION").unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string());
    let platform_secret = std::env::var("FUJIN_PLATFORM_SECRET")
        .ok()
        .filter(|s| !s.is_empty())
        .map(String::into_bytes);
    if platform_secret.is_none() {
        tracing::warn!("FUJIN_PLATFORM_SECRET unset - /v1/mcp will reject all requests");
    }
    let state = Arc::new(AppState {
        version,
        platform_secret,
    });

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/mcp", post(mcp))
        .with_state(state);

    let port = std::env::var("FUJIN_PORT").unwrap_or_else(|_| "8094".to_string());
    // localhost-bound: only the co-located platform reaches it (C3 hardening).
    let addr = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("bind {addr}: {e}"));
    tracing::info!(%addr, tool = TOOL, "fujin-server listening");
    axum::serve(listener, app).await.expect("server error");
}

async fn healthz(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    Json(json!({ "service": TOOL, "status": "ok", "version": s.version, "git_sha": option_env!("GIT_SHA").unwrap_or("dev") }))
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

async fn mcp(State(s): State<Arc<AppState>>, headers: HeaderMap, body: Bytes) -> impl IntoResponse {
    let Some(secret) = &s.platform_secret else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"auth_disabled"})),
        )
            .into_response();
    };
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim);
    let Some(claims) = token.and_then(|t| auth::verify(secret, TOOL, now_secs(), t)) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"invalid_platform_token"})),
        )
            .into_response();
    };

    let req: McpRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "bad_request", "detail": e.to_string()})),
            )
                .into_response();
        }
    };
    match dispatch(&req.method, req.params) {
        Ok(mut result) => {
            result["tool"] = json!(TOOL);
            result["version"] = json!(s.version);
            result["workspace"] = json!(claims.workspace);
            result["project"] = json!(claims.project);
            Json(result).into_response()
        }
        Err((code, payload)) => (code, Json(payload)).into_response(),
    }
}

/// One MCP call: `{ "method": "fujin.pack", "params": { "source_ref": "..." } }`.
#[derive(serde::Deserialize)]
struct McpRequest {
    method: String,
    #[serde(default)]
    params: serde_json::Value,
}

/// Params for `fujin.pack`.
#[derive(serde::Deserialize)]
struct PackParams {
    /// Upstream Decision id, preserved as ActionPacket provenance.
    source_ref: String,
}

/// Pure MCP dispatch over the fujin actions lib — no auth, no HTTP, so it is
/// unit-testable directly. `fujin` is a stateless OSS skeleton: it builds typed
/// objects but stores nothing, so read methods are unsupported.
fn dispatch(
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, (StatusCode, serde_json::Value)> {
    match method {
        "fujin.pack" => {
            let p: PackParams = serde_json::from_value(params).map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    json!({"error": "invalid_params", "detail": e.to_string()}),
                )
            })?;
            let source = LinkedItem {
                id: p.source_ref.clone(),
                label: p.source_ref,
            };
            let packet = ActionPacket {
                linked_decisions: vec![source],
                ..ActionPacket::default()
            };
            Ok(json!({ "method": "fujin.pack", "action_packet": packet }))
        }
        "fujin.list" | "fujin.get" | "fujin.list_packets" | "fujin.get_packet" => Err((
            StatusCode::NOT_IMPLEMENTED,
            json!({"error": "unsupported", "detail": "fujin-server is stateless (OSS skeleton has no store); list/get need a storage adapter"}),
        )),
        other => Err((
            StatusCode::BAD_REQUEST,
            json!({"error": "unknown_method", "detail": other}),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_builds_action_packet_with_decision_provenance() {
        let out = dispatch(
            "fujin.pack",
            json!({
                "source_ref": "dec_abc"
            }),
        )
        .expect("pack must succeed");
        let packet = &out["action_packet"];
        assert_eq!(out["method"], "fujin.pack");
        assert_eq!(packet["linked_decisions"][0]["id"], "dec_abc");
    }

    #[test]
    fn read_methods_unsupported_and_unknown_method_rejected() {
        let (code, _) = dispatch("fujin.list", json!({})).unwrap_err();
        assert_eq!(code, StatusCode::NOT_IMPLEMENTED);
        let (code, _) = dispatch("fujin.nope", json!({})).unwrap_err();
        assert_eq!(code, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn pack_rejects_bad_params() {
        let (code, _) = dispatch("fujin.pack", json!({})).unwrap_err();
        assert_eq!(code, StatusCode::BAD_REQUEST);
    }
}
