//! fujin-server — thin, independently-deployed HTTP/MCP wrapper around the
//! `fujin` actions lib. Its own deploy unit (own systemd service, own port).
//! Boundary-clean: no mcpbox dependency; the platform→tool auth contract and
//! the axum/tokio scaffold live in `layer_kit::{auth,serve}`.
//!
//! Routes:
//!   GET  /healthz   — open; liveness + version for the platform registry.
//!   POST /v1/mcp    — requires a valid platform token; actions surface
//!                     (`fujin.pack` builds a typed ActionPacket via the lib).
//!
//! Env: FUJIN_PORT (default 8094), FUJIN_PLATFORM_SECRET (HMAC key; if unset,
//! /v1/mcp is closed), FUJIN_VERSION (defaults to the crate version).

use axum::http::StatusCode;
use fujin::{ActionPacket, LinkedItem};
use layer_kit::auth::Claims;
use layer_kit::serve::{serve, McpHandler, ServeConfig};
use serde_json::json;

const TOOL: &str = "fujin";

/// Dispatches fujin's MCP methods. Stateless — fujin has no AI provider.
struct Handler;

impl McpHandler for Handler {
    async fn dispatch(
        &self,
        _claims: &Claims,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, (StatusCode, serde_json::Value)> {
        dispatch(method, params)
    }

    fn tools(&self) -> Vec<serde_json::Value> {
        tools()
    }
}

/// Tool descriptors for `tools/list` — one per method actually handled by
/// [`dispatch`] (`fujin.list`/`fujin.get`/`fujin.list_packets`/
/// `fujin.get_packet` are NOT_IMPLEMENTED, so they are omitted).
fn tools() -> Vec<serde_json::Value> {
    vec![json!({
        "name": "fujin_pack",
        "description": "Build a typed ActionPacket linked to an upstream Decision.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "source_ref": {"type": "string"}
            },
            "required": ["source_ref"]
        }
    })]
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().json().init();

    serve(
        ServeConfig {
            tool: TOOL,
            default_port: 8094,
            default_version: env!("CARGO_PKG_VERSION"),
            git_sha: option_env!("GIT_SHA").unwrap_or("dev"),
        },
        Handler,
    )
    .await;
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
    fn tools_list_names_are_all_dispatchable() {
        for tool in tools() {
            let name = tool["name"].as_str().unwrap();
            let method = name.replacen('_', ".", 1);
            let (_, body) = dispatch(&method, json!({}))
                .expect_err("empty params must not satisfy any real method");
            assert_ne!(
                body["error"], "unknown_method",
                "{method} must be a real dispatch method"
            );
        }
    }

    #[test]
    fn pack_rejects_bad_params() {
        let (code, _) = dispatch("fujin.pack", json!({})).unwrap_err();
        assert_eq!(code, StatusCode::BAD_REQUEST);
    }
}
