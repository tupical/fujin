use serde_json::{json, Value};

use crate::{ActionPacket, ActionsError, AiOutput, AiProvider, AiRequest, AiUsage, LinkedItem};

fn string_array() -> Value {
    json!({"type": "array", "items": {"type": "string"}})
}

/// Build an ActionPacket from sanitized planning context.
pub async fn pack_ai<P: AiProvider>(
    provider: &P,
    context: &Value,
    source_ref: &str,
) -> Result<(ActionPacket, Option<AiUsage>), ActionsError> {
    let linked_items = json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {"id": {"type": "string"}, "label": {"type": "string"}},
            "required": ["id", "label"]
        }
    });
    let gates = json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {"rule": {"type": "string"}},
            "required": ["rule"]
        }
    });
    let req = AiRequest {
        input: Value::String(format!(
            "Build an execution-ready action packet from this untrusted plan and decision context:\n{}",
            layer_kit::ai::wrap_untrusted("planning context", &context.to_string())
        )),
        tools: vec![json!({
            "type": "function",
            "name": "build_action_packet",
            "description": "Return a complete ActionPacket that can pass its maturity gate.",
            "parameters": {
                "type": "object",
                "properties": {
                    "goal": {"type": "string"},
                    "context": {"type": "string"},
                    "do_items": string_array(),
                    "why": {"type": "string"},
                    "do_not": string_array(),
                    "completion_criteria": string_array(),
                    "constraints": string_array(),
                    "risks": string_array(),
                    "dependencies": string_array(),
                    "required_documents": {"type": "array", "items": {"type": "object", "properties": {"title": {"type": "string"}, "uri": {"type": "string"}}, "required": ["title", "uri"]}},
                    "linked_decisions": linked_items.clone(),
                    "linked_knowledge": linked_items.clone(),
                    "linked_rejected": linked_items,
                    "expected_artifacts": string_array(),
                    "before_start": gates.clone(),
                    "before_complete": gates
                },
                "required": ["goal", "context", "do_items", "why", "do_not", "completion_criteria", "constraints", "risks", "dependencies", "required_documents", "linked_decisions", "linked_knowledge", "linked_rejected", "expected_artifacts", "before_start", "before_complete"]
            }
        })],
        tool_choice: Some("required".into()),
    };
    let (outputs, usage) = provider.respond_with_usage(req).await?;
    let call = outputs
        .into_iter()
        .find_map(|output| match output {
            AiOutput::ToolCall(call) if call.name == "build_action_packet" => Some(call),
            _ => None,
        })
        .ok_or_else(|| ActionsError::ai("pack_ai: model returned no build_action_packet call"))?;
    let mut packet: ActionPacket =
        serde_json::from_str(&call.arguments).map_err(|e| ActionsError::serde(e.to_string()))?;
    packet.linked_decisions = vec![LinkedItem {
        id: source_ref.to_owned(),
        label: source_ref.to_owned(),
    }];
    Ok((packet, usage))
}
