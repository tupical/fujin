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
    let packet: ActionPacket = serde_json::from_str(arguments).map_err(|e| e.to_string())?;
    let mut arguments = serde_json::to_value(packet).map_err(|e| e.to_string())?;
    // Explicit upstream bounds win over generated values, including empty bounds.
    // Deserialize afterwards so malformed upstream restrictions fail closed.
    if let Some(arguments) = arguments.as_object_mut() {
        for key in [
            "target_files",
            "conflict_policy",
            "required_checks",
            "reviewer_profile",
        ] {
            if let Some(value) = context["plan_brief"].get(key) {
                arguments.insert(key.to_owned(), value.clone());
            }
        }
    }
    let mut packet: ActionPacket = serde_json::from_value(arguments).map_err(|e| e.to_string())?;
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
    drop_incomplete_provenance(&mut packet);

    match crate::assess(&packet) {
        crate::Maturity::Ready => Ok(packet),
        crate::Maturity::NotReady { missing } => {
            Err(format!("missing or blank fields: {}", missing.join(", ")))
        }
    }
}

/// Drop provenance entries the model could not address.
///
/// The §13 provenance trio (`required_documents`, `linked_knowledge`,
/// `linked_rejected`) is *valid-or-empty* under
/// [`FujinStrictness::Soft`](crate::FujinStrictness::Soft): an empty list
/// means "no provenance established", while a half-filled entry is a
/// contract violation. Models routinely name a document they read about in
/// the planning context but have no address for, emitting `{title: "...",
/// uri: ""}`. That is not meaning the model failed to produce — it is
/// meaning that does not exist in the input, so the honest packet omits the
/// entry rather than inventing a URI (which `repair_request` forbids).
///
/// Dropping here keeps the gate's contract intact: under `Strict` the now
/// empty list still fails, correctly reporting that provenance is missing
/// instead of accepting a blank address.
fn drop_incomplete_provenance(packet: &mut ActionPacket) {
    packet
        .required_documents
        .retain(|doc| !doc.title.trim().is_empty() && !doc.uri.trim().is_empty());
    for links in [&mut packet.linked_knowledge, &mut packet.linked_rejected] {
        links.retain(|item| !item.id.trim().is_empty() && !item.label.trim().is_empty());
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
            "Build an execution-ready action packet from this untrusted plan and decision context. Preserve supplied execution bounds; use empty arrays or null when no bound is established, never invent file paths or reviewer requirements:\n{}",
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
                    "before_complete": gates,
                    "target_files": {
                        "type": "object",
                        "properties": {"owned": string_array(), "read_only": string_array(), "forbidden": string_array()},
                        "required": ["owned", "read_only", "forbidden"],
                        "additionalProperties": false
                    },
                    "conflict_policy": {"type": ["string", "null"]},
                    "required_checks": string_array(),
                    "reviewer_profile": {"type": ["string", "null"]}
                },
                "required": ["goal", "context", "do_items", "why", "do_not", "completion_criteria", "constraints", "risks", "dependencies", "required_documents", "linked_decisions", "linked_knowledge", "linked_rejected", "expected_artifacts", "before_start", "before_complete", "target_files", "conflict_policy", "required_checks", "reviewer_profile"]
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn context() -> Value {
        json!({"plan_brief": {}, "sensing_item": {}})
    }

    fn arguments(required_documents: Value) -> String {
        json!({
            "goal": "Ship the fix",
            "context": "pipeline run",
            "do_items": ["implement"],
            "why": "the gate rejects half-filled provenance",
            "do_not": ["do not invent a uri"],
            "completion_criteria": ["tests green"],
            "constraints": ["no new entities"],
            "risks": ["drift"],
            "dependencies": ["fujin"],
            "required_documents": required_documents,
            "linked_decisions": [],
            "linked_knowledge": [],
            "linked_rejected": [],
            "expected_artifacts": ["packet"],
            "before_start": [{"rule": "read the brief"}],
            "before_complete": [{"rule": "tests pass"}],
        })
        .to_string()
    }

    /// A model that names a document it has no address for must not block
    /// the run: the unaddressable entry is dropped, not invented.
    #[test]
    fn blank_uri_documents_are_dropped_instead_of_failing_the_gate() {
        let packet = parse_action_packet(
            &arguments(json!([{"title": "docs/guides/research-lineage.md", "uri": ""}])),
            &context(),
            "dec-1",
        )
        .expect("packet with an unaddressable document still matures");
        assert!(packet.required_documents.is_empty());
    }

    /// Dropping is surgical: addressable documents survive alongside the
    /// blank ones that are removed.
    #[test]
    fn addressable_documents_survive_the_drop() {
        let packet = parse_action_packet(
            &arguments(json!([
                {"title": "manifest", "uri": "docs/manifest.md"},
                {"title": "unaddressable", "uri": "   "},
            ])),
            &context(),
            "dec-1",
        )
        .expect("packet matures");
        assert_eq!(packet.required_documents.len(), 1);
        assert_eq!(packet.required_documents[0].uri, "docs/manifest.md");
    }

    /// The drop never rescues a genuinely missing required field — only the
    /// valid-or-empty provenance trio is affected.
    #[test]
    fn required_fields_still_fail_the_gate() {
        let mut args: Value = serde_json::from_str(&arguments(json!([]))).unwrap();
        args["do_items"] = json!([]);
        let error = parse_action_packet(&args.to_string(), &context(), "dec-1")
            .expect_err("an empty required list is still missing");
        assert!(error.contains("do_items"), "unexpected error: {error}");
    }
}
