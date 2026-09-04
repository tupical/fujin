use serde_json::{json, Value};

use crate::{ActionPacket, ActionsError, AiOutput, AiProvider, AiRequest, AiUsage, LinkedItem};

fn string_array() -> Value {
    json!({"type": "array", "items": {"type": "string"}})
}

fn merge_usage(total: Option<AiUsage>, next: Option<AiUsage>) -> Option<AiUsage> {
    fn add(left: Option<u64>, right: Option<u64>) -> Option<u64> {
        match (left, right) {
            (None, None) => None,
            (left, right) => Some(left.unwrap_or(0).saturating_add(right.unwrap_or(0))),
        }
    }

    match (total, next) {
        (None, next) => next,
        (total, None) => total,
        (Some(total), Some(next)) => Some(AiUsage {
            input_tokens: add(total.input_tokens, next.input_tokens),
            output_tokens: add(total.output_tokens, next.output_tokens),
            total_tokens: add(total.total_tokens, next.total_tokens),
        }),
    }
}

fn action_packet_arguments(outputs: Vec<AiOutput>) -> Result<String, String> {
    outputs
        .into_iter()
        .find_map(|output| match output {
            AiOutput::ToolCall(call) if call.name == "build_action_packet" => Some(call.arguments),
            _ => None,
        })
        .ok_or_else(|| "model returned no build_action_packet call".into())
}

fn parse_action_packet(
    arguments: &str,
    context: &Value,
    source_ref: &str,
) -> Result<ActionPacket, String> {
    let mut packet: ActionPacket = serde_json::from_str(arguments).map_err(|e| e.to_string())?;
    let from_brief = crate::packet_from_brief(&context["plan_brief"]);
    if packet.linked_rejected.is_empty() {
        packet.linked_rejected = from_brief.linked_rejected;
    }
    if packet.linked_knowledge.is_empty() {
        packet.linked_knowledge = match (
            context["sensing_item"]["kind"].as_str(),
            context["sensing_item"]["id"].as_str(),
            context["sensing_item"]["body"].as_str(),
        ) {
            (Some("knowledge"), Some(id), Some(body))
                if !id.trim().is_empty() && !body.trim().is_empty() =>
            {
                vec![LinkedItem {
                    id: id.to_owned(),
                    label: body.to_owned(),
                }]
            }
            _ => from_brief.linked_knowledge,
        };
    }
    packet.linked_decisions = vec![LinkedItem {
        id: source_ref.to_owned(),
        label: source_ref.to_owned(),
    }];

    match crate::assess(&packet) {
        crate::Maturity::Ready => Ok(packet),
        crate::Maturity::NotReady { missing } => {
            Err(format!("missing or blank fields: {}", missing.join(", ")))
        }
    }
}

fn repair_request(
    mut request: AiRequest,
    validation_error: &str,
    invalid_arguments: Option<&str>,
) -> AiRequest {
    let previous = invalid_arguments
        .map(|arguments| layer_kit::ai::wrap_untrusted("invalid arguments", arguments))
        .unwrap_or_else(|| {
            "The transport rejected the malformed output before exposing its arguments.".into()
        });
    request.input = Value::String(format!(
        "{}\n\nThe previous build_action_packet call was invalid.\nValidation error: {validation_error}\n{previous}\nRetry the build_action_packet call exactly once. Correct only from the supplied planning context; do not invent goal, why, titles, or other meaning-bearing content.",
        request.input.as_str().unwrap_or_default()
    ));
    request
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
            "strict": true,
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
    let mut request = req;
    let mut usage = None;
    let mut first_error = None;

    for attempt in 0..=1 {
        let (outputs, attempt_usage) = match provider.respond_with_usage(request.clone()).await {
            Ok(response) => response,
            Err(error) if error.kind() == layer_kit::ai::AiErrorKind::Schema => {
                let error = error.to_string();
                if attempt == 0 {
                    first_error = Some(error.clone());
                    request = repair_request(request, &error, None);
                    continue;
                }
                return Err(ActionsError::validation(format!(
                    "pack_ai: build_action_packet remained invalid after one repair; initial: {}; repair: {error}",
                    first_error.as_deref().unwrap_or("unknown validation error")
                )));
            }
            Err(error) if attempt == 1 => {
                return Err(ActionsError::ai(format!(
                    "pack_ai: repair failed after initial validation error ({}): {error}",
                    first_error.as_deref().unwrap_or("unknown validation error")
                )));
            }
            Err(error) => return Err(error.into()),
        };
        usage = merge_usage(usage, attempt_usage);

        let arguments = match action_packet_arguments(outputs) {
            Ok(arguments) => arguments,
            Err(error) if attempt == 1 => {
                return Err(ActionsError::validation(format!(
                    "pack_ai: build_action_packet repair produced no usable call; initial: {}; repair: {error}",
                    first_error.as_deref().unwrap_or("unknown validation error")
                )));
            }
            Err(error) => return Err(ActionsError::ai(format!("pack_ai: {error}"))),
        };
        match parse_action_packet(&arguments, context, source_ref) {
            Ok(packet) => return Ok((packet, usage)),
            Err(error) if attempt == 0 => {
                first_error = Some(error.clone());
                request = repair_request(request, &error, Some(&arguments));
            }
            Err(error) => {
                return Err(ActionsError::validation(format!(
                    "pack_ai: build_action_packet remained invalid after one repair; initial: {}; repair: {error}",
                    first_error.as_deref().unwrap_or("unknown validation error")
                )));
            }
        }
    }

    unreachable!("bounded repair loop always returns")
}
