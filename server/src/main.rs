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
use layer_kit::store::Store;
use serde_json::json;

const TOOL: &str = "fujin";

/// Dispatches fujin's MCP methods and owns the optional env fallback provider.
struct Handler {
    ai: Option<OpenAiProvider>,
    store: Store,
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
            dispatch(
                &self.store,
                Some(&provider),
                Some(provider.model()),
                method,
                params,
            )
            .await
        } else {
            dispatch(&self.store, self.ai.as_ref(), None, method, params).await
        }
    }

    fn tools(&self) -> Vec<serde_json::Value> {
        tools()
    }
}

/// Tool descriptors for `tools/list` — one per method actually handled by
/// [`dispatch`].
fn tools() -> Vec<serde_json::Value> {
    let mut tools = vec![json!({
        "name": "fujin_pack",
        "description": "Build a typed ActionPacket linked to an upstream Decision.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "source_ref": {"type": "string"},
                "plan_brief": {"type": "object"}
            },
            "required": ["source_ref"]
        }
    }), json!({
        "name": "fujin_assess",
        "description": "Run the §13 maturity gate on an ActionPacket. \
            Input: `action_packet` (packet JSON in the body; partial packets are accepted — \
            absent §13 keys are assessed as missing) or `id` — the source id the packet is \
            persisted under (e.g. `dec_1`), NOT `action_packet.id` (`ap_dec_1`); \
            if both are given, `action_packet` wins and `id` is ignored; with neither, \
            invalid_params is returned. \
            Output: {\"method\": \"fujin.assess\", \"ready\": bool, \"missing\": [field names]}. \
            A not-ready verdict is HTTP 200 with ready=false — assess is a query, not a gate \
            (unlike fujin_pack, which answers 422 not_ready); treat any non-2xx here as a \
            malformed request or server failure, never as \"not mature\". \
            On ready=false, `missing` names the §13 fields to fill; fill them and re-assess — \
            only a ready packet may go to handoff.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "action_packet": {"type": "object"},
                "id": {"type": "string"}
            },
            "anyOf": [{"required": ["action_packet"]}, {"required": ["id"]}]
        }
    })];
    for (name, description) in [
        ("fujin_list", "List persisted ActionPackets."),
        ("fujin_list_packets", "List persisted ActionPackets."),
        ("fujin_get", "Get a persisted ActionPacket by source id."),
        (
            "fujin_get_packet",
            "Get a persisted ActionPacket by source id.",
        ),
    ] {
        let get = name.contains("get");
        tools.push(json!({
            "name": name,
            "description": description,
            "inputSchema": if get {
                json!({"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"]})
            } else {
                json!({"type": "object", "properties": {"limit": {"type": "integer", "minimum": 1}}})
            }
        }));
    }
    tools
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().json().init();
    let store = Store::from_env(TOOL).await.unwrap_or_else(|e| {
        tracing::error!(error = %e, "failed to open fujin store");
        std::process::exit(1);
    });

    serve(
        ServeConfig {
            tool: TOOL,
            default_port: 8094,
            default_version: env!("CARGO_PKG_VERSION"),
            git_sha: option_env!("GIT_SHA").unwrap_or("dev"),
        },
        Handler {
            ai: AiConfig::from_env().map(OpenAiProvider::new),
            store,
        },
    )
    .await;
}

/// Params for `fujin.pack`.
#[derive(serde::Deserialize)]
struct PackParams {
    /// Upstream Decision id, preserved as ActionPacket provenance.
    source_ref: String,
    #[serde(default)]
    plan_brief: Option<serde_json::Value>,
}

#[derive(serde::Deserialize)]
struct ListParams {
    #[serde(default = "default_limit")]
    limit: i64,
}

#[derive(serde::Deserialize)]
struct GetParams {
    id: String,
}

/// Params for `fujin.assess`. `action_packet` stays raw JSON here: a
/// partial packet is the main use case ("tell me what is missing"), so the
/// body is overlaid onto a default packet before parsing — missing §13 keys
/// are reported by `assess` itself, not by serde.
#[derive(serde::Deserialize)]
struct AssessParams {
    #[serde(default)]
    action_packet: Option<serde_json::Value>,
    #[serde(default)]
    id: Option<String>,
}

/// Parse an assess request body into an [`ActionPacket`], tolerating a
/// partial packet: absent §13 keys fall back to the empty defaults so
/// `fujin::assess` can name them in `missing`. Non-object bodies and type
/// errors on the keys that are present remain `invalid_params`.
fn parse_assess_packet(
    body: serde_json::Value,
) -> Result<ActionPacket, (StatusCode, serde_json::Value)> {
    let invalid = |detail: String| {
        (
            StatusCode::BAD_REQUEST,
            json!({"error": "invalid_params", "detail": detail}),
        )
    };
    let body = body
        .as_object()
        .ok_or_else(|| invalid("`action_packet` must be an object".into()))?;
    let mut base = serde_json::to_value(ActionPacket::default())
        .expect("default packet serializes")
        .as_object()
        .expect("packet serializes to an object")
        .clone();
    for (key, value) in body {
        base.insert(key.clone(), value.clone());
    }
    serde_json::from_value(serde_json::Value::Object(base))
        .map_err(|e| invalid(e.to_string()))
}

fn default_limit() -> i64 {
    100
}

fn storage_error(e: impl std::fmt::Display) -> (StatusCode, serde_json::Value) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        json!({"error": "storage_error", "detail": e.to_string()}),
    )
}

const METHODS: &[&str] = &[
    "fujin.pack",
    "fujin.list",
    "fujin.list_packets",
    "fujin.get",
    "fujin.get_packet",
    "fujin.assess",
];

/// Pure MCP dispatch over the fujin actions lib — no auth, no HTTP, so it is
/// unit-testable directly. ActionPackets are persisted before success is returned.
async fn dispatch<P: fujin::AiProvider>(
    store: &Store,
    ai: Option<&P>,
    model: Option<&str>,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, (StatusCode, serde_json::Value)> {
    if !METHODS.contains(&method) {
        return Err((
            StatusCode::BAD_REQUEST,
            json!({"error": "unknown_method", "detail": method}),
        ));
    }
    match method {
        "fujin.pack" => {
            let context = params.clone();
            let p: PackParams = serde_json::from_value(params).map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    json!({"error": "invalid_params", "detail": e.to_string()}),
                )
            })?;
            if let Some(model) = model {
                let provider = ai.ok_or_else(|| {
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        json!({"error": "ai_not_configured", "detail": "AI provider is not configured"}),
                    )
                })?;
                let (packet, usage) = fujin::pack_ai(provider, &context, &p.source_ref)
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
                store
                    .put("action_packet", &p.source_ref, &packet)
                    .await
                    .map_err(storage_error)?;
                let mut meta = json!({"model": model});
                if let Some(usage) = usage {
                    meta["usage"] = json!(usage);
                }
                return Ok(json!({ "method": "fujin.pack", "action_packet": packet, "_meta": meta }));
            }
            let packet = if let Some(brief) = p.plan_brief {
                fujin::packet_from_brief(&brief)
            } else {
                let source = LinkedItem {
                    id: p.source_ref.clone(),
                    label: p.source_ref.clone(),
                };
                ActionPacket {
                    linked_decisions: vec![source],
                    ..ActionPacket::default()
                }
            };
            store
                .put("action_packet", &p.source_ref, &packet)
                .await
                .map_err(storage_error)?;
            Ok(json!({ "method": "fujin.pack", "action_packet": packet }))
        }
        "fujin.list" | "fujin.list_packets" => {
            let p: ListParams = serde_json::from_value(params).map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    json!({"error": "invalid_params", "detail": e.to_string()}),
                )
            })?;
            let packets: Vec<ActionPacket> = store
                .list("action_packet", p.limit)
                .await
                .map_err(storage_error)?;
            Ok(json!({"method": method, "action_packets": packets}))
        }
        "fujin.get" | "fujin.get_packet" => {
            let p: GetParams = serde_json::from_value(params).map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    json!({"error": "invalid_params", "detail": e.to_string()}),
                )
            })?;
            let packet: Option<ActionPacket> = store
                .get("action_packet", &p.id)
                .await
                .map_err(storage_error)?;
            packet
                .map(|packet| json!({"method": method, "action_packet": packet}))
                .ok_or_else(|| {
                    (
                        StatusCode::NOT_FOUND,
                        json!({"error": "not_found", "detail": p.id}),
                    )
                })
        }
        "fujin.assess" => {
            let p: AssessParams = serde_json::from_value(params).map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    json!({"error": "invalid_params", "detail": e.to_string()}),
                )
            })?;
            let packet = if let Some(body) = p.action_packet {
                parse_assess_packet(body)?
            } else if let Some(id) = p.id {
                store
                    .get("action_packet", &id)
                    .await
                    .map_err(storage_error)?
                    .ok_or_else(|| {
                        (
                            StatusCode::NOT_FOUND,
                            json!({"error": "not_found", "detail": id}),
                        )
                    })?
            } else {
                return Err((
                    StatusCode::BAD_REQUEST,
                    json!({"error": "invalid_params", "detail": "pass `action_packet` (packet JSON) or `id` of a persisted packet"}),
                ));
            };
            let (ready, missing) = match fujin::assess(&packet) {
                Maturity::Ready => (true, Vec::new()),
                Maturity::NotReady { missing } => (false, missing),
            };
            Ok(json!({"method": method, "ready": ready, "missing": missing}))
        }
        other => Err((
            StatusCode::BAD_REQUEST,
            json!({"error": "unknown_method", "detail": other}),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fujin::{AiError, AiOutput, AiRequest, AiUsage, ToolCall};
    use std::sync::atomic::{AtomicU64, Ordering};

    static DB_SEQ: AtomicU64 = AtomicU64::new(1);

    fn db_path() -> String {
        std::env::temp_dir()
            .join(format!(
                "fujin-server-{}-{}.db",
                std::process::id(),
                DB_SEQ.fetch_add(1, Ordering::Relaxed)
            ))
            .to_string_lossy()
            .into_owned()
    }

    async fn test_store() -> Store {
        Store::open(&db_path()).await.unwrap()
    }

    async fn dispatch<P: fujin::AiProvider>(
        ai: Option<&P>,
        request_ai: bool,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, (StatusCode, serde_json::Value)> {
        super::dispatch(
            &test_store().await,
            ai,
            request_ai.then_some("test"),
            method,
            params,
        )
        .await
    }

    struct Fake(Result<Vec<AiOutput>, AiError>);

    impl fujin::AiProvider for Fake {
        async fn respond(&self, _req: AiRequest) -> Result<Vec<AiOutput>, AiError> {
            self.0.clone()
        }

        async fn respond_with_usage(
            &self,
            _req: AiRequest,
        ) -> Result<(Vec<AiOutput>, Option<AiUsage>), AiError> {
            Ok((self.0.clone()?, Some(AiUsage {
                input_tokens: Some(123),
                output_tokens: Some(45),
                total_tokens: Some(168),
            })))
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
        assert!(out.get("_meta").is_none());
    }

    #[tokio::test]
    async fn read_methods_and_unknown_method_rejected() {
        let out = dispatch(None::<&OpenAiProvider>, false, "fujin.list", json!({}))
            .await
            .unwrap();
        assert_eq!(out["action_packets"], json!([]));
        let (code, _) = dispatch(None::<&OpenAiProvider>, false, "fujin.nope", json!({}))
            .await
            .unwrap_err();
        assert_eq!(code, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn packet_maps_brief_persists_across_restart_and_write_errors_surface() {
        let path = db_path();
        let store = Store::open(&path).await.unwrap();
        let created = super::dispatch(
            &store,
            None::<&OpenAiProvider>,
            None,
            "fujin.pack",
            json!({
                "source_ref": "decision_1",
                "plan_brief": {
                    "goal": "Ship storage",
                    "in_scope": ["persist packets"],
                    "why_now": "avoid data loss",
                    "decisions_made": ["decision_1"]
                }
            }),
        )
        .await
        .unwrap();
        assert_eq!(created["action_packet"]["goal"], "Ship storage");
        assert_eq!(
            created["action_packet"]["do_items"],
            json!(["persist packets"])
        );
        drop(store);

        let reopened = Store::open(&path).await.unwrap();
        let got = super::dispatch(
            &reopened,
            None::<&OpenAiProvider>,
            None,
            "fujin.get_packet",
            json!({"id": "decision_1"}),
        )
        .await
        .unwrap();
        assert_eq!(got["action_packet"]["goal"], "Ship storage");

        reopened.pool().close().await;
        let (code, body) = super::dispatch(
            &reopened,
            None::<&OpenAiProvider>,
            None,
            "fujin.pack",
            json!({"source_ref": "decision_2"}),
        )
        .await
        .unwrap_err();
        assert_eq!(code, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"], "storage_error");
    }

    #[tokio::test]
    async fn tools_list_names_are_all_dispatchable() {
        for tool in tools() {
            let name = tool["name"].as_str().unwrap();
            let method = name.replacen('_', ".", 1);
            if let Err((_, body)) =
                dispatch(None::<&OpenAiProvider>, false, &method, json!({})).await
            {
                assert_ne!(body["error"], "unknown_method", "{method} must be real");
            }
        }
    }

    #[test]
    fn tools_catalogue_matches_methods() {
        layer_kit::test_support::assert_catalogue_matches(&tools(), METHODS);
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
        let store = test_store().await;
        let out = super::dispatch(&store, Some(&fake), Some("test"), "fujin.pack", params)
            .await
            .unwrap();
        assert_eq!(out["action_packet"]["goal"], "Ship auth");
        assert_eq!(out["action_packet"]["linked_decisions"][0]["id"], "dec_abc");
        assert_eq!(out["_meta"]["model"], "test");
        assert_eq!(out["_meta"]["usage"]["total_tokens"], 168);
        let stored: serde_json::Value = store
            .get("action_packet", "dec_abc")
            .await
            .unwrap()
            .unwrap();
        assert!(stored.get("_meta").is_none());
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

    #[tokio::test]
    async fn assess_mature_packet_in_body_is_ready() {
        let out = dispatch(
            None::<&OpenAiProvider>,
            false,
            "fujin.assess",
            json!({"action_packet": packet_args("Ship auth")}),
        )
        .await
        .unwrap();
        assert_eq!(out["method"], "fujin.assess");
        assert_eq!(out["ready"], true);
        assert_eq!(out["missing"], json!([]));
    }

    #[tokio::test]
    async fn assess_immature_packet_names_missing_fields() {
        let mut args = packet_args("");
        args["do_items"] = json!([]);
        let out = dispatch(
            None::<&OpenAiProvider>,
            false,
            "fujin.assess",
            json!({"action_packet": args}),
        )
        .await
        .unwrap();
        assert_eq!(out["method"], "fujin.assess");
        assert_eq!(out["ready"], false);
        assert_eq!(out["missing"], json!(["goal", "do_items"]));
    }

    #[tokio::test]
    async fn assess_by_id_reads_persisted_packet() {
        let store = test_store().await;
        let packet: ActionPacket = serde_json::from_value(packet_args("Ship storage")).unwrap();
        store.put("action_packet", "pkt_1", &packet).await.unwrap();
        let out = super::dispatch(
            &store,
            None::<&OpenAiProvider>,
            None,
            "fujin.assess",
            json!({"id": "pkt_1"}),
        )
        .await
        .unwrap();
        assert_eq!(out["method"], "fujin.assess");
        assert_eq!(out["ready"], true);
        assert_eq!(out["missing"], json!([]));
    }

    #[tokio::test]
    async fn assess_body_wins_over_id() {
        // A mature packet in the body plus an id that does not exist in the
        // store must still succeed — `action_packet` takes priority.
        let out = dispatch(
            None::<&OpenAiProvider>,
            false,
            "fujin.assess",
            json!({"action_packet": packet_args("Ship auth"), "id": "no_such_packet"}),
        )
        .await
        .unwrap();
        assert_eq!(out["ready"], true);
    }

    #[tokio::test]
    async fn assess_requires_packet_or_id() {
        let (code, body) = dispatch(None::<&OpenAiProvider>, false, "fujin.assess", json!({}))
            .await
            .unwrap_err();
        assert_eq!(code, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_params");
    }

    #[tokio::test]
    async fn assess_unknown_id_is_not_found() {
        let (code, body) = dispatch(
            None::<&OpenAiProvider>,
            false,
            "fujin.assess",
            json!({"id": "no_such_packet"}),
        )
        .await
        .unwrap_err();
        assert_eq!(code, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], "not_found");
    }

    #[tokio::test]
    async fn assess_partial_packet_names_absent_keys_as_missing() {
        // The main use case: a packet that only carries some §13 keys must
        // get a 200 ready=false whose `missing` names the absent keys, on
        // par with empty ones — not a 400 from serde.
        let out = dispatch(
            None::<&OpenAiProvider>,
            false,
            "fujin.assess",
            json!({"action_packet": {"goal": "x", "do_items": ["y"]}}),
        )
        .await
        .unwrap();
        assert_eq!(out["method"], "fujin.assess");
        assert_eq!(out["ready"], false);
        // Strict full-list check, in §13 order: every absent key must be
        // named, and the two present keys (goal, do_items) must not be.
        assert_eq!(
            out["missing"],
            json!([
                "context",
                "why",
                "do_not",
                "completion_criteria",
                "constraints",
                "risks",
                "dependencies",
                "linked_decisions",
                "expected_artifacts",
                "before_start",
                "before_complete",
            ])
        );
    }

    #[tokio::test]
    async fn assess_tolerates_platform_id_inside_packet() {
        // mcpbox.ru injects an id like "ap_<decision_id>" into the packet it
        // sends back in `action_packet`. ActionPacket has no
        // deny_unknown_fields, so the extra key must be ignored — if the
        // struct ever gains it, the platform path breaks and this test
        // should be the one to say so.
        let mut args = packet_args("Ship auth");
        args["id"] = json!("ap_dec_1");
        let out = dispatch(
            None::<&OpenAiProvider>,
            false,
            "fujin.assess",
            json!({"action_packet": args}),
        )
        .await
        .unwrap();
        assert_eq!(out["method"], "fujin.assess");
        assert_eq!(out["ready"], true);
        assert_eq!(out["missing"], json!([]));
    }

    #[tokio::test]
    async fn assess_malformed_action_packet_is_invalid_params() {
        // Non-object body.
        let (code, body) = dispatch(
            None::<&OpenAiProvider>,
            false,
            "fujin.assess",
            json!({"action_packet": "not an object"}),
        )
        .await
        .unwrap_err();
        assert_eq!(code, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_params");

        // Object, but a present key has the wrong type.
        let (code, body) = dispatch(
            None::<&OpenAiProvider>,
            false,
            "fujin.assess",
            json!({"action_packet": {"goal": 5}}),
        )
        .await
        .unwrap_err();
        assert_eq!(code, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_params");
    }
}
