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

/// Why one attempt to turn the model output into a packet failed.
enum Parse {
    /// Fixable by asking the model again (the message is fed to repair).
    Invalid(String),
    /// Only a human can supply what is missing — repair cannot help.
    NeedsInput(Vec<String>),
}

fn parse_action_packet(
    arguments: &str,
    context: &Value,
    source_ref: &str,
    last_chance: bool,
) -> Result<ActionPacket, Parse> {
    let invalid = |e: serde_json::Error| Parse::Invalid(e.to_string());
    let mut arguments: Value = serde_json::from_str(arguments).map_err(invalid)?;
    // Provenance is host-filled (below) and not required of the model.
    // Explicit upstream bounds win over generated values, including empty bounds.
    // Deserialize afterwards so malformed upstream restrictions fail closed.
    if let Some(arguments) = arguments.as_object_mut() {
        for key in [
            "required_documents",
            "linked_decisions",
            "linked_knowledge",
            "linked_rejected",
        ] {
            arguments.entry(key).or_insert_with(|| json!([]));
        }
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
    let mut packet: ActionPacket = serde_json::from_value(arguments).map_err(invalid)?;
    // Before the upstream fallbacks below: an unaddressable entry must not
    // count as "the model supplied provenance" and suppress a real value the
    // brief already carries.
    if last_chance {
        ask_for_missing_addresses(&mut packet)?;
    }
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
        crate::Maturity::NotReady { missing } => Err(Parse::Invalid(format!(
            "missing or blank fields: {}",
            missing.join(", ")
        ))),
    }
}

/// Last-attempt provenance check: a document the model named but could not
/// address becomes a question for the human instead of being invented or
/// silently dropped.
///
/// The §13 provenance trio is *valid-or-empty* under
/// [`FujinStrictness::Soft`](crate::FujinStrictness::Soft): an empty list
/// means "no provenance established", a half-filled entry is a contract
/// violation. Models name documents they read about in the planning context
/// but have no address for (`{title: "...", uri: ""}`), and `repair_request`
/// forbids inventing the URI. This runs only on the final attempt, after
/// repair had its chance to find the address upstream. Entries blank in both
/// fields are noise and dropped; blank `linked_*` entries are dropped so the
/// brief fallbacks can fill them.
fn ask_for_missing_addresses(packet: &mut ActionPacket) -> Result<(), Parse> {
    packet
        .required_documents
        .retain(|doc| !(doc.title.trim().is_empty() && doc.uri.trim().is_empty()));
    let questions: Vec<String> = packet
        .required_documents
        .iter()
        .filter(|doc| doc.uri.trim().is_empty())
        .map(|doc| format!("Укажите адрес документа «{}»", doc.title.trim()))
        .collect();
    if !questions.is_empty() {
        return Err(Parse::NeedsInput(questions));
    }
    for links in [&mut packet.linked_knowledge, &mut packet.linked_rejected] {
        links.retain(|item| !item.id.trim().is_empty() && !item.label.trim().is_empty());
    }
    Ok(())
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
                    "required_documents": {"type": "array", "description": "Only documents named in the context WITH a known address; empty array otherwise. Never invent a uri.", "items": {"type": "object", "properties": {"title": {"type": "string"}, "uri": {"type": "string"}}, "required": ["title", "uri"]}},
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
                "required": ["goal", "context", "do_items", "why", "do_not", "completion_criteria", "constraints", "risks", "dependencies", "required_documents", "expected_artifacts", "before_start", "before_complete", "target_files", "conflict_policy", "required_checks", "reviewer_profile"]
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
        match parse_action_packet(&arguments, context, source_ref, attempt == 1) {
            Ok(packet) => return Ok((packet, usage)),
            Err(Parse::NeedsInput(questions)) => return Err(ActionsError::NeedsInput(questions)),
            Err(Parse::Invalid(error)) if attempt == 0 => {
                first_error = Some(error.clone());
                request = repair_request(request, &error, Some(&arguments));
            }
            Err(Parse::Invalid(error)) => {
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

    fn invalid(result: Result<ActionPacket, Parse>) -> String {
        match result.expect_err("expected a failure") {
            Parse::Invalid(error) => error,
            Parse::NeedsInput(q) => panic!("unexpected needs_input: {q:?}"),
        }
    }

    /// The first attempt must still fail so `repair_request` gets its chance
    /// to ask for the missing address — asking the human is the last resort.
    #[test]
    fn first_attempt_still_reports_the_blank_uri() {
        let error = invalid(parse_action_packet(
            &arguments(json!([{"title": "docs/guides/research-lineage.md", "uri": ""}])),
            &context(),
            "dec-1",
            false,
        ));
        assert!(error.contains("required_documents"), "unexpected: {error}");
    }

    /// A document the model could not address even after repair becomes a
    /// question for the human: nothing is invented and nothing is dropped.
    #[test]
    fn blank_uri_document_becomes_a_question_on_the_last_attempt() {
        let Err(Parse::NeedsInput(questions)) = parse_action_packet(
            &arguments(json!([
                {"title": "manifest", "uri": "docs/manifest.md"},
                {"title": "docs/guides/research-lineage.md", "uri": "  "},
            ])),
            &context(),
            "dec-1",
            true,
        ) else {
            panic!("expected needs_input");
        };
        assert_eq!(
            questions,
            vec!["Укажите адрес документа «docs/guides/research-lineage.md»"]
        );
    }

    /// Fully addressed documents pass as before; blank/blank noise is dropped.
    #[test]
    fn addressable_documents_pass_and_noise_is_dropped() {
        let packet = parse_action_packet(
            &arguments(json!([
                {"title": "manifest", "uri": "docs/manifest.md"},
                {"title": " ", "uri": ""},
            ])),
            &context(),
            "dec-1",
            true,
        )
        .ok()
        .expect("packet matures");
        assert_eq!(packet.required_documents.len(), 1);
        assert_eq!(packet.required_documents[0].uri, "docs/manifest.md");
    }

    /// The model is no longer asked for host-filled provenance: omitting the
    /// `linked_*` keys must not fail deserialization.
    #[test]
    fn model_may_omit_host_filled_provenance() {
        let mut args: Value = serde_json::from_str(&arguments(json!([]))).unwrap();
        for key in [
            "required_documents",
            "linked_decisions",
            "linked_knowledge",
            "linked_rejected",
        ] {
            args.as_object_mut().unwrap().remove(key);
        }
        let packet = parse_action_packet(&args.to_string(), &context(), "dec-1", false)
            .ok()
            .expect("packet matures");
        assert_eq!(packet.linked_decisions[0].id, "dec-1");
    }

    /// An address the brief already carries must survive a malformed model
    /// link: dropping runs before the upstream fallbacks, so a blank entry
    /// cannot suppress real provenance.
    #[test]
    fn upstream_provenance_survives_a_malformed_model_link() {
        let mut args: Value = serde_json::from_str(&arguments(json!([]))).unwrap();
        args["linked_knowledge"] = json!([{"id": "", "label": ""}]);
        args["linked_rejected"] = json!([{"id": "", "label": ""}]);
        let context = json!({
            "plan_brief": {"knowledge_base": ["kn-1"], "rejected_alternatives": ["rej-1"]},
            "sensing_item": {},
        });
        let packet = parse_action_packet(&args.to_string(), &context, "dec-1", true)
            .ok()
            .expect("packet matures");
        assert_eq!(packet.linked_knowledge[0].id, "kn-1");
        assert_eq!(packet.linked_rejected[0].id, "rej-1");
    }

    /// The drop never rescues a genuinely missing required field — only the
    /// valid-or-empty provenance trio is affected.
    #[test]
    fn required_fields_still_fail_the_gate() {
        let mut args: Value = serde_json::from_str(&arguments(json!([]))).unwrap();
        args["do_items"] = json!([]);
        let error = invalid(parse_action_packet(
            &args.to_string(),
            &context(),
            "dec-1",
            true,
        ));
        assert!(error.contains("do_items"), "unexpected error: {error}");
    }
}
