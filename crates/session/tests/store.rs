use screen_sidekick_session::{BeginTurn, CreateAttachment, SessionStore, SessionStoreError};
use screen_sidekick_sidekick_protocol::{
    AttachmentSourceType, ErrorCode, SafetyStatus, TurnStatus,
};

#[test]
fn migrations_apply_on_empty_database() {
    let store = SessionStore::in_memory().expect("store opens");
    let sessions = store.list_sessions().expect("sessions list");
    assert!(sessions.is_empty());
}

#[test]
fn begin_turn_enforces_one_active_turn_per_session() {
    let store = SessionStore::in_memory().expect("store opens");
    let session = store.create_session(Some("Test")).expect("session created");
    let first = store
        .begin_turn(BeginTurn {
            session_id: session.id.clone(),
            user_text: "hello".to_owned(),
            attachment_ids: Vec::new(),
            idempotency_key: "key-1".to_owned(),
            request_hash: "hash-1".to_owned(),
        })
        .expect("turn begins");

    let second = store.begin_turn(BeginTurn {
        session_id: session.id.clone(),
        user_text: "again".to_owned(),
        attachment_ids: Vec::new(),
        idempotency_key: "key-2".to_owned(),
        request_hash: "hash-2".to_owned(),
    });

    assert!(matches!(second, Err(SessionStoreError::TurnAlreadyRunning)));
    assert!(store.get_turn(&first.turn_id).is_ok());
}

#[test]
fn begin_turn_enforces_one_active_turn_for_daemon() {
    let store = SessionStore::in_memory().expect("store opens");
    let first_session = store
        .create_session(Some("First"))
        .expect("session created");
    let second_session = store
        .create_session(Some("Second"))
        .expect("session created");
    let first = store
        .begin_turn(BeginTurn {
            session_id: first_session.id,
            user_text: "hello".to_owned(),
            attachment_ids: Vec::new(),
            idempotency_key: "key-1".to_owned(),
            request_hash: "hash-1".to_owned(),
        })
        .expect("turn begins");

    let second = store.begin_turn(BeginTurn {
        session_id: second_session.id,
        user_text: "again".to_owned(),
        attachment_ids: Vec::new(),
        idempotency_key: "key-2".to_owned(),
        request_hash: "hash-2".to_owned(),
    });

    assert!(matches!(second, Err(SessionStoreError::TurnAlreadyRunning)));
    assert!(store.get_turn(&first.turn_id).is_ok());
}

#[test]
fn recover_interrupted_active_turns_fails_stale_turns_and_allows_new_send() {
    let store = SessionStore::in_memory().expect("store opens");
    let stale_session = store
        .create_session(Some("Stale"))
        .expect("stale session created");
    let stale_turn = store
        .begin_turn(BeginTurn {
            session_id: stale_session.id.clone(),
            user_text: "stale".to_owned(),
            attachment_ids: Vec::new(),
            idempotency_key: "stale-key".to_owned(),
            request_hash: "stale-hash".to_owned(),
        })
        .expect("stale turn begins");
    store
        .mark_turn_running(
            &stale_turn.turn_id,
            Some("remote_thread"),
            Some("remote_turn"),
        )
        .expect("stale turn runs");

    let recovered = store
        .recover_interrupted_active_turns()
        .expect("interrupted turns recover");
    let failed_turn = store.get_turn(&stale_turn.turn_id).expect("turn loads");
    let stale_session_state = store
        .get_session(&stale_session.id)
        .expect("stale session loads");
    let new_session = store
        .create_session(Some("New"))
        .expect("new session created");
    let new_turn = store.begin_turn(BeginTurn {
        session_id: new_session.id,
        user_text: "new".to_owned(),
        attachment_ids: Vec::new(),
        idempotency_key: "new-key".to_owned(),
        request_hash: "new-hash".to_owned(),
    });

    assert_eq!(recovered, 1);
    assert_eq!(failed_turn.status, TurnStatus::Failed);
    assert_eq!(
        failed_turn.error.as_ref().map(|error| error.code),
        Some(ErrorCode::CodexAppServerUnavailable)
    );
    assert!(stale_session_state.active_turn.is_none());
    assert!(stale_session_state.messages.iter().any(|message| {
        message.id == stale_turn.message_id
            && message.status == screen_sidekick_sidekick_protocol::MessageStatus::Failed
    }));
    assert!(new_turn.is_ok());
}

#[test]
fn recover_interrupted_active_turns_preserves_finished_turns() {
    let store = SessionStore::in_memory().expect("store opens");
    let session = store.create_session(Some("Done")).expect("session created");
    let turn = store
        .begin_turn(BeginTurn {
            session_id: session.id.clone(),
            user_text: "done".to_owned(),
            attachment_ids: Vec::new(),
            idempotency_key: "done-key".to_owned(),
            request_hash: "done-hash".to_owned(),
        })
        .expect("turn begins");
    store
        .complete_turn(&turn.turn_id, "answer")
        .expect("turn completes");

    let recovered = store
        .recover_interrupted_active_turns()
        .expect("recovery runs");
    let completed = store.get_turn(&turn.turn_id).expect("turn loads");
    let session_state = store.get_session(&session.id).expect("session loads");

    assert_eq!(recovered, 0);
    assert_eq!(completed.status, TurnStatus::Completed);
    assert!(session_state.messages.iter().any(|message| {
        message.role == screen_sidekick_sidekick_protocol::MessageRole::Assistant
            && message.text == "answer"
            && message.status == screen_sidekick_sidekick_protocol::MessageStatus::Completed
    }));
}

#[test]
fn message_send_idempotency_reuses_existing_turn() {
    let store = SessionStore::in_memory().expect("store opens");
    let session = store.create_session(Some("Test")).expect("session created");
    let request = BeginTurn {
        session_id: session.id,
        user_text: "hello".to_owned(),
        attachment_ids: Vec::new(),
        idempotency_key: "same-key".to_owned(),
        request_hash: "same-hash".to_owned(),
    };

    let first = store.begin_turn(request.clone()).expect("turn begins");
    let second = store.begin_turn(request).expect("turn is reused");

    assert!(!first.reused);
    assert!(second.reused);
    assert_eq!(first.message_id, second.message_id);
    assert_eq!(first.turn_id, second.turn_id);
}

#[test]
fn failed_idempotency_retry_returns_stored_failure_without_reusing_turn() {
    let store = SessionStore::in_memory().expect("store opens");
    let session = store.create_session(Some("Test")).expect("session created");
    let request = BeginTurn {
        session_id: session.id,
        user_text: "hello".to_owned(),
        attachment_ids: Vec::new(),
        idempotency_key: "same-key".to_owned(),
        request_hash: "same-hash".to_owned(),
    };
    let first = store.begin_turn(request.clone()).expect("turn begins");
    store
        .fail_turn(&first.turn_id, ErrorCode::CodexNotFound, Some("start"))
        .expect("turn fails");

    let retry = store.begin_turn(request);

    assert!(matches!(
        retry,
        Err(SessionStoreError::IdempotencyFailed(
            ErrorCode::CodexNotFound
        ))
    ));
}

#[test]
fn unsupported_version_failure_round_trips_through_turn_and_idempotency() {
    let store = SessionStore::in_memory().expect("store opens");
    let session = store.create_session(Some("Test")).expect("session created");
    let request = BeginTurn {
        session_id: session.id,
        user_text: "hello".to_owned(),
        attachment_ids: Vec::new(),
        idempotency_key: "same-key".to_owned(),
        request_hash: "same-hash".to_owned(),
    };
    let first = store.begin_turn(request.clone()).expect("turn begins");
    store
        .fail_turn(
            &first.turn_id,
            ErrorCode::UnsupportedCodexVersion,
            Some("start"),
        )
        .expect("turn fails");

    let failed = store.get_turn(&first.turn_id).expect("turn loads");
    let retry = store.begin_turn(request);

    assert_eq!(
        failed.error.as_ref().map(|error| error.code),
        Some(ErrorCode::UnsupportedCodexVersion)
    );
    assert!(matches!(
        retry,
        Err(SessionStoreError::IdempotencyFailed(
            ErrorCode::UnsupportedCodexVersion
        ))
    ));
}

#[test]
fn safety_review_failure_round_trips_through_turn_and_idempotency() {
    let store = SessionStore::in_memory().expect("store opens");
    let session = store.create_session(Some("Test")).expect("session created");
    let request = BeginTurn {
        session_id: session.id,
        user_text: "hello".to_owned(),
        attachment_ids: Vec::new(),
        idempotency_key: "same-key".to_owned(),
        request_hash: "same-hash".to_owned(),
    };
    let first = store.begin_turn(request.clone()).expect("turn begins");
    store
        .fail_turn(
            &first.turn_id,
            ErrorCode::SafetyReviewFailed,
            Some("context_load"),
        )
        .expect("turn fails");

    let failed = store.get_turn(&first.turn_id).expect("turn loads");
    let retry = store.begin_turn(request);

    assert_eq!(
        failed.error.as_ref().map(|error| error.code),
        Some(ErrorCode::SafetyReviewFailed)
    );
    assert!(matches!(
        retry,
        Err(SessionStoreError::IdempotencyFailed(
            ErrorCode::SafetyReviewFailed
        ))
    ));
}

#[test]
fn clear_codex_thread_link_only_removes_expected_thread() {
    let store = SessionStore::in_memory().expect("store opens");
    let session = store.create_session(Some("Test")).expect("session created");
    store
        .link_codex_thread(&session.id, "thread_1", None, None)
        .expect("thread link is stored");

    let mismatched = store
        .clear_codex_thread_link(&session.id, "thread_other")
        .expect("mismatched clear runs");
    let retained = store
        .codex_thread_id(&session.id)
        .expect("thread link loads");
    let cleared = store
        .clear_codex_thread_link(&session.id, "thread_1")
        .expect("matching clear runs");
    let missing = store
        .codex_thread_id(&session.id)
        .expect("thread link loads after clear");

    assert!(!mismatched);
    assert_eq!(retained.as_deref(), Some("thread_1"));
    assert!(cleared);
    assert_eq!(missing, None);
}

#[test]
fn cancelled_idempotency_retry_is_rejected_without_reusing_turn() {
    let store = SessionStore::in_memory().expect("store opens");
    let session = store.create_session(Some("Test")).expect("session created");
    let request = BeginTurn {
        session_id: session.id,
        user_text: "hello".to_owned(),
        attachment_ids: Vec::new(),
        idempotency_key: "same-key".to_owned(),
        request_hash: "same-hash".to_owned(),
    };
    let first = store.begin_turn(request.clone()).expect("turn begins");
    store.cancel_turn(&first.turn_id).expect("turn cancels");

    let retry = store.begin_turn(request);

    assert!(matches!(
        retry,
        Err(SessionStoreError::IdempotencyCancelled)
    ));
}

#[test]
fn idempotency_key_rejects_different_request_hash() {
    let store = SessionStore::in_memory().expect("store opens");
    let session = store.create_session(Some("Test")).expect("session created");
    store
        .begin_turn(BeginTurn {
            session_id: session.id.clone(),
            user_text: "hello".to_owned(),
            attachment_ids: Vec::new(),
            idempotency_key: "same-key".to_owned(),
            request_hash: "hash-1".to_owned(),
        })
        .expect("turn begins");

    let result = store.begin_turn(BeginTurn {
        session_id: session.id,
        user_text: "hello".to_owned(),
        attachment_ids: Vec::new(),
        idempotency_key: "same-key".to_owned(),
        request_hash: "hash-2".to_owned(),
    });

    assert!(matches!(
        result,
        Err(SessionStoreError::IdempotencyConflict)
    ));
}

#[test]
fn attachment_persists_sanitized_context_without_raw_capture() {
    let store = SessionStore::in_memory().expect("store opens");
    let session = store.create_session(Some("Test")).expect("session created");

    let attachment = store
        .create_attachment(test_attachment(&session.id, None))
        .expect("attachment created");

    let sanitized = store
        .sanitized_context_json(&attachment.id)
        .expect("sanitized context loads");

    assert_eq!(sanitized, "{\"page\":{\"title\":\"safe\"}}");
    assert!(!sanitized.contains("RAWSECRET"));
}

#[test]
fn attachment_codex_context_loads_safety_review_with_sanitized_context() {
    let store = SessionStore::in_memory().expect("store opens");
    let session = store.create_session(Some("Test")).expect("session created");
    let attachment = store
        .create_attachment(test_attachment(&session.id, None))
        .expect("attachment created");

    let context = store
        .attachment_codex_context(&attachment.id)
        .expect("attachment context loads");

    assert_eq!(
        context.sanitized_context_json,
        "{\"page\":{\"title\":\"safe\"}}"
    );
    assert_eq!(context.safety_review_json, "{\"has_danger\":false}");
}

#[test]
fn attachment_can_reference_message_in_same_session() {
    let store = SessionStore::in_memory().expect("store opens");
    let session = store.create_session(Some("Test")).expect("session created");
    let turn = store
        .begin_turn(BeginTurn {
            session_id: session.id.clone(),
            user_text: "hello".to_owned(),
            attachment_ids: Vec::new(),
            idempotency_key: "key".to_owned(),
            request_hash: "hash".to_owned(),
        })
        .expect("turn begins");

    let attachment = store
        .create_attachment(test_attachment(&session.id, Some(turn.message_id)))
        .expect("attachment created");

    assert_eq!(attachment.session_id, session.id);
}

#[test]
fn linked_attachment_cannot_be_reassigned_to_later_message() {
    let store = SessionStore::in_memory().expect("store opens");
    let session = store.create_session(Some("Test")).expect("session created");
    let attachment = store
        .create_attachment(test_attachment(&session.id, None))
        .expect("attachment created");
    let first = store
        .begin_turn(BeginTurn {
            session_id: session.id.clone(),
            user_text: "first".to_owned(),
            attachment_ids: vec![attachment.id.clone()],
            idempotency_key: "first-key".to_owned(),
            request_hash: "first-hash".to_owned(),
        })
        .expect("first turn begins");
    store
        .complete_turn(&first.turn_id, "answer")
        .expect("first turn completes");

    let second = store.begin_turn(BeginTurn {
        session_id: session.id.clone(),
        user_text: "second".to_owned(),
        attachment_ids: vec![attachment.id.clone()],
        idempotency_key: "second-key".to_owned(),
        request_hash: "second-hash".to_owned(),
    });
    let session_state = store.get_session(&session.id).expect("session loads");
    let first_message = session_state
        .messages
        .iter()
        .find(|message| message.id == first.message_id)
        .expect("first message remains in history");

    assert!(matches!(
        second,
        Err(SessionStoreError::AttachmentAlreadyLinked(_))
    ));
    assert_eq!(first_message.attachment_ids, vec![attachment.id]);
    assert!(!session_state
        .messages
        .iter()
        .any(|message| message.text == "second"));
}

#[test]
fn attachment_rejects_message_from_another_session_as_not_found() {
    let store = SessionStore::in_memory().expect("store opens");
    let source_session = store
        .create_session(Some("Source"))
        .expect("source session created");
    let target_session = store
        .create_session(Some("Target"))
        .expect("target session created");
    let turn = store
        .begin_turn(BeginTurn {
            session_id: source_session.id,
            user_text: "hello".to_owned(),
            attachment_ids: Vec::new(),
            idempotency_key: "key".to_owned(),
            request_hash: "hash".to_owned(),
        })
        .expect("turn begins");

    let result =
        store.create_attachment(test_attachment(&target_session.id, Some(turn.message_id)));

    assert!(matches!(result, Err(SessionStoreError::MessageNotFound)));
}

#[test]
fn attachment_rejects_unknown_message_as_not_found() {
    let store = SessionStore::in_memory().expect("store opens");
    let session = store.create_session(Some("Test")).expect("session created");

    let result =
        store.create_attachment(test_attachment(&session.id, Some("msg_missing".to_owned())));

    assert!(matches!(result, Err(SessionStoreError::MessageNotFound)));
}

#[test]
fn failed_turn_clears_active_turn_without_fake_assistant_message() {
    let store = SessionStore::in_memory().expect("store opens");
    let session = store.create_session(Some("Test")).expect("session created");
    let turn = store
        .begin_turn(BeginTurn {
            session_id: session.id.clone(),
            user_text: "hello".to_owned(),
            attachment_ids: Vec::new(),
            idempotency_key: "key".to_owned(),
            request_hash: "hash".to_owned(),
        })
        .expect("turn begins");

    let failed = store
        .fail_turn(&turn.turn_id, ErrorCode::CodexTurnFailed, None)
        .expect("turn fails");
    let session_state = store.get_session(&session.id).expect("session loads");

    assert_eq!(failed.status, TurnStatus::Failed);
    assert!(session_state.active_turn.is_none());
    assert!(session_state
        .messages
        .iter()
        .all(|message| message.role != screen_sidekick_sidekick_protocol::MessageRole::Assistant));
}

#[test]
fn get_turn_for_session_rejects_cross_session_turn_as_not_found() {
    let store = SessionStore::in_memory().expect("store opens");
    let source_session = store
        .create_session(Some("Source"))
        .expect("source session created");
    let target_session = store
        .create_session(Some("Target"))
        .expect("target session created");
    let turn = store
        .begin_turn(BeginTurn {
            session_id: source_session.id.clone(),
            user_text: "hello".to_owned(),
            attachment_ids: Vec::new(),
            idempotency_key: "key".to_owned(),
            request_hash: "hash".to_owned(),
        })
        .expect("turn begins");

    let matching = store
        .get_turn_for_session(&source_session.id, &turn.turn_id)
        .expect("matching turn loads");
    let mismatched = store.get_turn_for_session(&target_session.id, &turn.turn_id);

    assert_eq!(matching.id, turn.turn_id);
    assert!(matches!(mismatched, Err(SessionStoreError::TurnNotFound)));
}

#[test]
fn get_cancellable_turn_for_session_returns_remote_codex_turn_id() {
    let store = SessionStore::in_memory().expect("store opens");
    let session = store.create_session(Some("Test")).expect("session created");
    let turn = store
        .begin_turn(BeginTurn {
            session_id: session.id.clone(),
            user_text: "hello".to_owned(),
            attachment_ids: Vec::new(),
            idempotency_key: "key".to_owned(),
            request_hash: "hash".to_owned(),
        })
        .expect("turn begins");
    store
        .mark_turn_running(&turn.turn_id, Some("remote_thread"), Some("remote_turn"))
        .expect("turn is marked running");

    let target = store
        .get_cancellable_turn_for_session(&session.id, &turn.turn_id)
        .expect("cancellation target loads");

    assert_eq!(target.turn.id, turn.turn_id);
    assert_eq!(target.codex_turn_id, "remote_turn");
}

#[test]
fn cancel_turn_for_session_rejects_finished_turn_without_rewriting_transcript() {
    let store = SessionStore::in_memory().expect("store opens");
    let session = store.create_session(Some("Test")).expect("session created");
    let turn = store
        .begin_turn(BeginTurn {
            session_id: session.id.clone(),
            user_text: "hello".to_owned(),
            attachment_ids: Vec::new(),
            idempotency_key: "key".to_owned(),
            request_hash: "hash".to_owned(),
        })
        .expect("turn begins");
    store
        .complete_turn(&turn.turn_id, "answer")
        .expect("turn completes");

    let result = store.cancel_turn_for_session(&session.id, &turn.turn_id);
    let completed = store.get_turn(&turn.turn_id).expect("turn loads");
    let session_state = store.get_session(&session.id).expect("session loads");

    assert!(matches!(result, Err(SessionStoreError::TurnNotCancellable)));
    assert_eq!(completed.status, TurnStatus::Completed);
    assert!(session_state.active_turn.is_none());
    assert!(session_state.messages.iter().any(|message| {
        message.role == screen_sidekick_sidekick_protocol::MessageRole::Assistant
            && message.text == "answer"
            && message.status == screen_sidekick_sidekick_protocol::MessageStatus::Completed
    }));
}

#[test]
fn complete_turn_rejects_cancelled_turn_without_assistant_message() {
    let store = SessionStore::in_memory().expect("store opens");
    let session = store.create_session(Some("Test")).expect("session created");
    let turn = store
        .begin_turn(BeginTurn {
            session_id: session.id.clone(),
            user_text: "hello".to_owned(),
            attachment_ids: Vec::new(),
            idempotency_key: "key".to_owned(),
            request_hash: "hash".to_owned(),
        })
        .expect("turn begins");
    store
        .mark_turn_running(&turn.turn_id, Some("remote_thread"), Some("remote_turn"))
        .expect("turn is marked running");
    store
        .cancel_turn_for_session(&session.id, &turn.turn_id)
        .expect("turn is cancelled");

    let result = store.complete_turn(&turn.turn_id, "late answer");
    let cancelled = store.get_turn(&turn.turn_id).expect("turn loads");
    let session_state = store.get_session(&session.id).expect("session loads");

    assert!(matches!(result, Err(SessionStoreError::TurnNotCancellable)));
    assert_eq!(cancelled.status, TurnStatus::Cancelled);
    assert!(session_state.active_turn.is_none());
    assert!(session_state
        .messages
        .iter()
        .all(|message| message.role != screen_sidekick_sidekick_protocol::MessageRole::Assistant));
    assert!(session_state.messages.iter().any(|message| {
        message.id == turn.message_id
            && message.status == screen_sidekick_sidekick_protocol::MessageStatus::Cancelled
    }));
}

#[test]
fn fail_turn_rejects_cancelled_turn_without_rewriting_status() {
    let store = SessionStore::in_memory().expect("store opens");
    let session = store.create_session(Some("Test")).expect("session created");
    let turn = store
        .begin_turn(BeginTurn {
            session_id: session.id.clone(),
            user_text: "hello".to_owned(),
            attachment_ids: Vec::new(),
            idempotency_key: "key".to_owned(),
            request_hash: "hash".to_owned(),
        })
        .expect("turn begins");
    store
        .mark_turn_running(&turn.turn_id, Some("remote_thread"), Some("remote_turn"))
        .expect("turn is marked running");
    store
        .cancel_turn_for_session(&session.id, &turn.turn_id)
        .expect("turn is cancelled");

    let result = store.fail_turn(&turn.turn_id, ErrorCode::CodexTurnFailed, Some("late"));
    let cancelled = store.get_turn(&turn.turn_id).expect("turn loads");
    let session_state = store.get_session(&session.id).expect("session loads");

    assert!(matches!(result, Err(SessionStoreError::TurnNotCancellable)));
    assert_eq!(cancelled.status, TurnStatus::Cancelled);
    assert!(cancelled.error.is_none());
    assert!(session_state
        .messages
        .iter()
        .all(|message| message.role != screen_sidekick_sidekick_protocol::MessageRole::Assistant));
}

#[test]
fn completing_turn_persists_assistant_message_and_clears_active_turn() {
    let store = SessionStore::in_memory().expect("store opens");
    let session = store.create_session(Some("Test")).expect("session created");
    let turn = store
        .begin_turn(BeginTurn {
            session_id: session.id.clone(),
            user_text: "hello".to_owned(),
            attachment_ids: Vec::new(),
            idempotency_key: "key".to_owned(),
            request_hash: "hash".to_owned(),
        })
        .expect("turn begins");

    let (completed, assistant) = store
        .complete_turn(&turn.turn_id, "answer")
        .expect("turn completes");
    let session_state = store.get_session(&session.id).expect("session loads");

    assert_eq!(completed.status, TurnStatus::Completed);
    assert_eq!(assistant.text, "answer");
    assert!(session_state.active_turn.is_none());
}

fn test_attachment(session_id: &str, message_id: Option<String>) -> CreateAttachment {
    CreateAttachment {
        session_id: session_id.to_owned(),
        message_id,
        source_type: AttachmentSourceType::BrowserTab,
        summary: "https://example.test".to_owned(),
        sanitized_context_json: "{\"page\":{\"title\":\"safe\"}}".to_owned(),
        safety_review_json: "{\"has_danger\":false}".to_owned(),
        source_metadata_json: "{\"capture_id\":\"cap_1\"}".to_owned(),
        safety_status: SafetyStatus::Clean,
        debug_available: true,
    }
}
