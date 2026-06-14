use screen_sidekick_codex_client::{
    fixture_events_from_jsonl, hash_schema_bytes, schema_hash, CodexClientErrorKind, CodexEvent,
};

#[test]
fn committed_schema_metadata_has_hash() {
    let hash = schema_hash().expect("schema hash exists");
    assert!(hash.starts_with("sha256:"));
}

#[test]
fn app_server_schema_fixture_is_parseable_json() {
    let schema = include_str!("../schema/app-server/v2/TurnStartParams.json");
    let value: serde_json::Value = serde_json::from_str(schema).expect("schema parses");
    assert_eq!(value["title"], "TurnStartParams");
}

#[test]
fn event_fixture_maps_to_handwritten_subset() {
    let fixture = include_str!("../schema/examples/turn_stream.jsonl");
    let events = fixture_events_from_jsonl(fixture).expect("events parse");

    assert!(matches!(
        events.first(),
        Some(CodexEvent::TurnStarted { turn_id }) if turn_id == "turn_123"
    ));
    assert!(matches!(
        events.get(1),
        Some(CodexEvent::Delta { turn_id, delta }) if turn_id == "turn_123" && delta == "Hello"
    ));
    assert!(matches!(
        events.get(2),
        Some(CodexEvent::Completed {
            turn_id,
            final_assistant_text,
        }) if turn_id == "turn_123" && final_assistant_text.is_none()
    ));
}

#[test]
fn item_completed_agent_message_maps_to_final_assistant_message() {
    let events = fixture_events_from_jsonl(
        r#"{"method":"item/completed","params":{"threadId":"thread_123","turnId":"turn_123","completedAtMs":1,"item":{"id":"item_1","type":"agentMessage","text":"Final answer"}}}"#,
    )
    .expect("events parse");

    assert_eq!(
        events,
        vec![CodexEvent::FinalAssistantMessage {
            turn_id: "turn_123".to_owned(),
            text: "Final answer".to_owned()
        }]
    );
}

#[test]
fn item_completed_final_answer_agent_message_maps_to_final_assistant_message() {
    let events = fixture_events_from_jsonl(
        r#"{"method":"item/completed","params":{"threadId":"thread_123","turnId":"turn_123","completedAtMs":1,"item":{"id":"item_1","type":"agentMessage","phase":"final_answer","text":"Final answer"}}}"#,
    )
    .expect("events parse");

    assert_eq!(
        events,
        vec![CodexEvent::FinalAssistantMessage {
            turn_id: "turn_123".to_owned(),
            text: "Final answer".to_owned()
        }]
    );
}

#[test]
fn item_completed_commentary_agent_message_is_not_assistant_text() {
    let events = fixture_events_from_jsonl(
        r#"{"method":"item/completed","params":{"threadId":"thread_123","turnId":"turn_123","completedAtMs":1,"item":{"id":"item_1","type":"agentMessage","phase":"commentary","text":"Progress update"}}}"#,
    )
    .expect("events parse");

    assert!(events.is_empty());
}

#[test]
fn turn_completed_without_items_view_uses_legacy_full_items_default_for_final_assistant_text() {
    let events = fixture_events_from_jsonl(
        r#"{"method":"turn/completed","params":{"threadId":"thread_123","turn":{"id":"turn_123","items":[{"id":"plan_1","type":"plan","text":"Do not store this"},{"id":"item_1","type":"agentMessage","text":"Final answer"}],"status":"completed"}}}"#,
    )
    .expect("events parse");

    assert_eq!(
        events,
        vec![CodexEvent::Completed {
            turn_id: "turn_123".to_owned(),
            final_assistant_text: Some("Final answer".to_owned())
        }]
    );
}

#[test]
fn turn_completed_full_items_view_supplies_final_assistant_text() {
    let events = fixture_events_from_jsonl(
        r#"{"method":"turn/completed","params":{"threadId":"thread_123","turn":{"id":"turn_123","itemsView":"full","items":[{"id":"plan_1","type":"plan","text":"Do not store this"},{"id":"item_1","type":"agentMessage","phase":"final_answer","text":"Final answer"}],"status":"completed"}}}"#,
    )
    .expect("events parse");

    assert_eq!(
        events,
        vec![CodexEvent::Completed {
            turn_id: "turn_123".to_owned(),
            final_assistant_text: Some("Final answer".to_owned())
        }]
    );
}

#[test]
fn turn_completed_summary_items_view_does_not_supply_final_assistant_text() {
    let events = fixture_events_from_jsonl(
        r#"{"method":"turn/completed","params":{"threadId":"thread_123","turn":{"id":"turn_123","itemsView":"summary","items":[{"id":"summary_final","type":"agentMessage","phase":"final_answer","text":"Partial summary should not win"}],"status":"completed"}}}"#,
    )
    .expect("events parse");

    assert_eq!(
        events,
        vec![CodexEvent::Completed {
            turn_id: "turn_123".to_owned(),
            final_assistant_text: None
        }]
    );
}

#[test]
fn turn_completed_not_loaded_items_view_does_not_supply_final_assistant_text() {
    let events = fixture_events_from_jsonl(
        r#"{"method":"turn/completed","params":{"threadId":"thread_123","turn":{"id":"turn_123","itemsView":"notLoaded","items":[{"id":"summary_final","type":"agentMessage","phase":"final_answer","text":"Partial summary should not win"}],"status":"completed"}}}"#,
    )
    .expect("events parse");

    assert_eq!(
        events,
        vec![CodexEvent::Completed {
            turn_id: "turn_123".to_owned(),
            final_assistant_text: None
        }]
    );
}

#[test]
fn turn_completed_unknown_items_view_does_not_supply_final_assistant_text() {
    let events = fixture_events_from_jsonl(
        r#"{"method":"turn/completed","params":{"threadId":"thread_123","turn":{"id":"turn_123","itemsView":"futurePartialView","items":[{"id":"summary_final","type":"agentMessage","phase":"final_answer","text":"Future partial payload should not win"}],"status":"completed"}}}"#,
    )
    .expect("events parse");

    assert_eq!(
        events,
        vec![CodexEvent::Completed {
            turn_id: "turn_123".to_owned(),
            final_assistant_text: None
        }]
    );
}

#[test]
fn turn_completed_non_string_items_view_does_not_supply_final_assistant_text() {
    let events = fixture_events_from_jsonl(
        r#"{"method":"turn/completed","params":{"threadId":"thread_123","turn":{"id":"turn_123","itemsView":true,"items":[{"id":"summary_final","type":"agentMessage","phase":"final_answer","text":"Malformed view should not win"}],"status":"completed"}}}"#,
    )
    .expect("events parse");

    assert_eq!(
        events,
        vec![CodexEvent::Completed {
            turn_id: "turn_123".to_owned(),
            final_assistant_text: None
        }]
    );
}

#[test]
fn turn_completed_prefers_final_answer_over_later_commentary() {
    let events = fixture_events_from_jsonl(
        r#"{"method":"turn/completed","params":{"threadId":"thread_123","turn":{"id":"turn_123","itemsView":"full","items":[{"id":"legacy_1","type":"agentMessage","text":"Legacy fallback"},{"id":"final_1","type":"agentMessage","phase":"final_answer","text":"Final answer"},{"id":"commentary_1","type":"agentMessage","phase":"commentary","text":"Progress update"}],"status":"completed"}}}"#,
    )
    .expect("events parse");

    assert_eq!(
        events,
        vec![CodexEvent::Completed {
            turn_id: "turn_123".to_owned(),
            final_assistant_text: Some("Final answer".to_owned())
        }]
    );
}

#[test]
fn turn_completed_commentary_only_does_not_supply_final_assistant_text() {
    let events = fixture_events_from_jsonl(
        r#"{"method":"turn/completed","params":{"threadId":"thread_123","turn":{"id":"turn_123","items":[{"id":"commentary_1","type":"agentMessage","phase":"commentary","text":"Progress update"}],"status":"completed"}}}"#,
    )
    .expect("events parse");

    assert_eq!(
        events,
        vec![CodexEvent::Completed {
            turn_id: "turn_123".to_owned(),
            final_assistant_text: None
        }]
    );
}

#[test]
fn item_completed_non_agent_message_is_not_assistant_text() {
    let events = fixture_events_from_jsonl(
        r#"{"method":"item/completed","params":{"threadId":"thread_123","turnId":"turn_123","completedAtMs":1,"item":{"id":"plan_1","type":"plan","text":"Plan text"}}}"#,
    )
    .expect("events parse");

    assert!(events.is_empty());
}

#[test]
fn turn_completed_failed_status_maps_to_failed_event() {
    let events = fixture_events_from_jsonl(
        r#"{"method":"turn/completed","params":{"threadId":"thread_123","turn":{"id":"turn_123","items":[],"status":"failed","error":{"message":"model failed"}}}}"#,
    )
    .expect("events parse");

    assert_eq!(
        events,
        vec![CodexEvent::Failed {
            turn_id: Some("turn_123".to_owned()),
            message: "model failed".to_owned(),
            error_kind: None
        }]
    );
}

#[test]
fn turn_completed_failed_status_preserves_unauthorized_error_kind() {
    let events = fixture_events_from_jsonl(
        r#"{"method":"turn/completed","params":{"threadId":"thread_123","turn":{"id":"turn_123","items":[],"status":"failed","error":{"message":"not logged in","codexErrorInfo":"unauthorized"}}}}"#,
    )
    .expect("events parse");

    assert_eq!(
        events,
        vec![CodexEvent::Failed {
            turn_id: Some("turn_123".to_owned()),
            message: "not logged in".to_owned(),
            error_kind: Some(CodexClientErrorKind::NotLoggedIn)
        }]
    );
}

#[test]
fn turn_completed_interrupted_status_preserves_unauthorized_error_kind() {
    let events = fixture_events_from_jsonl(
        r#"{"method":"turn/completed","params":{"threadId":"thread_123","turn":{"id":"turn_123","items":[],"status":"interrupted","error":{"message":"not logged in","codexErrorInfo":{"unauthorized":{}}}}}}"#,
    )
    .expect("events parse");

    assert_eq!(
        events,
        vec![CodexEvent::Failed {
            turn_id: Some("turn_123".to_owned()),
            message: "not logged in".to_owned(),
            error_kind: Some(CodexClientErrorKind::NotLoggedIn)
        }]
    );
}

#[test]
fn turn_completed_interrupted_status_maps_to_failed_event() {
    let events = fixture_events_from_jsonl(
        r#"{"method":"turn/completed","params":{"threadId":"thread_123","turn":{"id":"turn_123","items":[],"status":"interrupted"}}}"#,
    )
    .expect("events parse");

    assert_eq!(
        events,
        vec![CodexEvent::Failed {
            turn_id: Some("turn_123".to_owned()),
            message: "Codex turn was interrupted.".to_owned(),
            error_kind: None
        }]
    );
}

#[test]
fn retryable_error_notification_is_non_terminal() {
    let events = fixture_events_from_jsonl(
        r#"
        {"method":"error","params":{"threadId":"thread_123","turnId":"turn_123","willRetry":true,"error":{"message":"temporary upstream error"}}}
        {"method":"turn/completed","params":{"threadId":"thread_123","turn":{"id":"turn_123","items":[],"status":"completed"}}}
        "#,
    )
    .expect("events parse");

    assert_eq!(
        events,
        vec![CodexEvent::Completed {
            turn_id: "turn_123".to_owned(),
            final_assistant_text: None
        }]
    );
}

#[test]
fn retryable_unauthorized_error_notification_is_non_terminal() {
    let events = fixture_events_from_jsonl(
        r#"
        {"method":"error","params":{"threadId":"thread_123","turnId":"turn_123","willRetry":true,"error":{"message":"temporary auth error","codexErrorInfo":"unauthorized"}}}
        {"method":"turn/completed","params":{"threadId":"thread_123","turn":{"id":"turn_123","items":[],"status":"completed"}}}
        "#,
    )
    .expect("events parse");

    assert_eq!(
        events,
        vec![CodexEvent::Completed {
            turn_id: "turn_123".to_owned(),
            final_assistant_text: None
        }]
    );
}

#[test]
fn non_retryable_error_notification_maps_to_failed_event() {
    let events = fixture_events_from_jsonl(
        r#"{"method":"error","params":{"threadId":"thread_123","turnId":"turn_123","willRetry":false,"error":{"message":"permanent upstream error"}}}"#,
    )
    .expect("events parse");

    assert_eq!(
        events,
        vec![CodexEvent::Failed {
            turn_id: Some("turn_123".to_owned()),
            message: "permanent upstream error".to_owned(),
            error_kind: None
        }]
    );
}

#[test]
fn non_retryable_unauthorized_error_notification_maps_to_not_logged_in_failed_event() {
    let events = fixture_events_from_jsonl(
        r#"{"method":"error","params":{"threadId":"thread_123","turnId":"turn_123","willRetry":false,"error":{"message":"not logged in","codexErrorInfo":"unauthorized"}}}"#,
    )
    .expect("events parse");

    assert_eq!(
        events,
        vec![CodexEvent::Failed {
            turn_id: Some("turn_123".to_owned()),
            message: "not logged in".to_owned(),
            error_kind: Some(CodexClientErrorKind::NotLoggedIn)
        }]
    );
}

#[test]
fn unsupported_server_request_is_protocol_error_not_unknown_event() {
    let error = fixture_events_from_jsonl(
        r#"{"id":"approval_1","method":"item/commandExecution/requestApproval","params":{"turnId":"turn_123"}}"#,
    )
    .expect_err("server request is unsupported");

    assert!(error
        .message
        .contains("Codex app-server requested unsupported client method"));
}

#[test]
fn numeric_id_server_request_is_protocol_error_not_unknown_event() {
    let error = fixture_events_from_jsonl(
        r#"{"id":42,"method":"item/commandExecution/requestApproval","params":{"turnId":"turn_123"}}"#,
    )
    .expect_err("numeric id server request is unsupported");

    assert!(error
        .message
        .contains("Codex app-server requested unsupported client method"));
}

#[test]
fn turn_completed_in_progress_status_is_protocol_error() {
    let error = fixture_events_from_jsonl(
        r#"{"method":"turn/completed","params":{"threadId":"thread_123","turn":{"id":"turn_123","items":[],"status":"inProgress"}}}"#,
    )
    .expect_err("inProgress is not terminal");

    assert!(error.message.contains("non-terminal status"));
}

#[test]
fn hash_schema_bytes_is_stable() {
    assert_eq!(
        hash_schema_bytes(b"abc"),
        "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}
