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
//! /v1/mcp is closed), FUJIN_VERSION, and the optional OPENAI_* fallback.

use axum::http::StatusCode;
use fujin::{ActionPacket, LinkedItem, Maturity};
use layer_kit::ai::extract_ai_config;
use layer_kit::auth::Claims;
use layer_kit::openai::{AiConfig, OpenAiProvider};
use layer_kit::serve::{serve, McpHandler, ServeConfig};
use serde_json::json;

const TOOL: &str = "fujin";

/// Dispatches fujin's MCP methods and owns the optional env fallback provider.
struct Handler {
    ai: Option<OpenAiProvider>,
}

impl McpHandler for Handler {
    async fn dispatch(
        &self,
        _claims: &Claims,
        method: &str,
        mut params: serde_json::Value,
    ) -> Result<serde_json::Value, (StatusCode, serde_json::Value)> {
        if let Some(cfg) = extract_ai_config(&mut params) {
            let provider = OpenAiProvider::new(cfg);
            dispatch(Some(&provider), true, method, params).await
        } else {
            dispatch(self.ai.as_ref(), false, method, params).await
        }
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
        Handler {
            ai: AiConfig::from_env().map(OpenAiProvider::new),
        },
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
async fn dispatch<P: fujin::AiProvider>(
    ai: Option<&P>,
    request_ai: bool,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, (StatusCode, serde_json::Value)> {
    match method {
        "fujin.pack" => {
            let context = params.clone();
            let p: PackParams = serde_json::from_value(params).map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    json!({"error": "invalid_params", "detail": e.to_string()}),
                )
            })?;
            if request_ai {
                let provider = ai.ok_or_else(|| {
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        json!({"error": "ai_not_configured", "detail": "AI provider is not configured"}),
                    )
                })?;
                let packet = fujin::pack_ai(provider, &context, &p.source_ref)
                    .await
                    .map_err(|e| {
                        (
                            StatusCode::BAD_GATEWAY,
                            json!({"error": "ai_error", "detail": e.to_string()}),
                        )
                    })?;
                if let Maturity::NotReady { missing } = fujin::assess(&packet) {
                    return Err((
                        StatusCode::UNPROCESSABLE_ENTITY,
                        json!({"error": "not_ready", "missing": missing}),
                    ));
                }
                return Ok(json!({ "method": "fujin.pack", "action_packet": packet }));
            }
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
    use fujin::{AiError, AiOutput, AiRequest, ToolCall};

    struct Fake(Result<Vec<AiOutput>, AiError>);

    impl fujin::AiProvider for Fake {
        async fn respond(&self, _req: AiRequest) -> Result<Vec<AiOutput>, AiError> {
            self.0.clone()
        }
    }

    fn packet_args(goal: &str) -> serde_json::Value {
        json!({
            "goal": goal,
            "context": "Plan context",
            "do_items": ["Implement change"],
            "why": "Required by plan",
            "do_not": ["Change wire names"],
            "completion_criteria": ["Tests pass"],
            "constraints": ["No new dependencies"],
            "risks": ["Regression"],
            "dependencies": ["Decision approved"],
            "required_documents": [{"title": "Plan", "uri": "plan://1"}],
            "linked_decisions": [{"id": "ignored", "label": "ignored"}],
            "linked_knowledge": [{"id": "knowledge_1", "label": "Context"}],
            "linked_rejected": [{"id": "rejected_1", "label": "Alternative"}],
            "expected_artifacts": ["Patch"],
            "before_start": [{"rule": "Read plan"}],
            "before_complete": [{"rule": "Run tests"}]
        })
    }

    #[tokio::test]
    async fn pack_builds_action_packet_with_decision_provenance() {
        let out = dispatch(
            None::<&OpenAiProvider>,
            false,
            "fujin.pack",
            json!({
                "source_ref": "dec_abc"
            }),
        )
        .await
        .expect("pack must succeed");
        let packet = &out["action_packet"];
        assert_eq!(out["method"], "fujin.pack");
        assert_eq!(packet["linked_decisions"][0]["id"], "dec_abc");
    }

    #[tokio::test]
    async fn read_methods_unsupported_and_unknown_method_rejected() {
        let (code, _) = dispatch(None::<&OpenAiProvider>, false, "fujin.list", json!({}))
            .await
            .unwrap_err();
        assert_eq!(code, StatusCode::NOT_IMPLEMENTED);
        let (code, _) = dispatch(None::<&OpenAiProvider>, false, "fujin.nope", json!({}))
            .await
            .unwrap_err();
        assert_eq!(code, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn tools_list_names_are_all_dispatchable() {
        for tool in tools() {
            let name = tool["name"].as_str().unwrap();
            let method = name.replacen('_', ".", 1);
            let (_, body) = dispatch(None::<&OpenAiProvider>, false, &method, json!({}))
                .await
                .expect_err("empty params must not satisfy any real method");
            assert_ne!(
                body["error"], "unknown_method",
                "{method} must be a real dispatch method"
            );
        }
    }

    #[tokio::test]
    async fn pack_rejects_bad_params() {
        let (code, _) = dispatch(None::<&OpenAiProvider>, false, "fujin.pack", json!({}))
            .await
            .unwrap_err();
        assert_eq!(code, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn request_ai_builds_mature_packet_without_leaking_secret() {
        let fake = Fake(Ok(vec![AiOutput::ToolCall(ToolCall {
            name: "build_action_packet".into(),
            arguments: packet_args("Ship auth").to_string(),
        })]));
        let mut params = json!({
            "source_ref": "dec_abc",
            "plan_brief": {"goal": "Ship auth"},
            "ai": {"api_key": "sk-secret", "base_url": "https://ai.test/v1", "model": "test"}
        });
        assert!(extract_ai_config(&mut params).is_some());
        let out = dispatch(Some(&fake), true, "fujin.pack", params)
            .await
            .unwrap();
        assert_eq!(out["action_packet"]["goal"], "Ship auth");
        assert_eq!(out["action_packet"]["linked_decisions"][0]["id"], "dec_abc");
        assert!(!out.to_string().contains("sk-secret"));
    }

    #[tokio::test]
    async fn request_ai_rejects_immature_packet_with_missing() {
        let fake = Fake(Ok(vec![AiOutput::ToolCall(ToolCall {
            name: "build_action_packet".into(),
            arguments: packet_args("").to_string(),
        })]));
        let (code, body) = dispatch(
            Some(&fake),
            true,
            "fujin.pack",
            json!({"source_ref": "dec_abc"}),
        )
        .await
        .unwrap_err();
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["error"], "not_ready");
        assert!(body["missing"].as_array().unwrap().contains(&json!("goal")));
    }

    #[tokio::test]
    async fn request_ai_failure_is_ai_error() {
        let (code, body) = dispatch(
            Some(&Fake(Err(AiError::new("boom")))),
            true,
            "fujin.pack",
            json!({"source_ref": "dec_abc"}),
        )
        .await
        .unwrap_err();
        assert_eq!(code, StatusCode::BAD_GATEWAY);
        assert_eq!(body["error"], "ai_error");
    }
}
