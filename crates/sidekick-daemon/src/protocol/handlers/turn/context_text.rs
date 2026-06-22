use screen_sidekick_session::SessionStore;
use screen_sidekick_sidekick_protocol::{ErrorCode, ProtocolError};
use serde::Deserialize;

use super::super::super::protocol_error;
use super::super::support::session_error;

pub(super) fn load_context_text(
    store: &SessionStore,
    attachment_ids: &[String],
) -> Result<String, ProtocolError> {
    let mut parts = Vec::new();
    for attachment_id in attachment_ids {
        let context = store
            .attachment_codex_context(attachment_id)
            .map_err(session_error)?;
        parts.push(format_attachment_context(context)?);
    }
    Ok(parts.join("\n\n"))
}

fn format_attachment_context(
    context: screen_sidekick_session::AttachmentCodexContext,
) -> Result<String, ProtocolError> {
    Ok(format!(
        "Safety review:\n{}\n\nScreenContext JSON:\n{}",
        format_safety_review(&context.safety_review_json)?,
        context.sanitized_context_json
    ))
}

fn format_safety_review(safety_review_json: &str) -> Result<String, ProtocolError> {
    let review: StoredSafetyReview = serde_json::from_str(safety_review_json).map_err(|_| {
        protocol_error(
            ErrorCode::SafetyReviewFailed,
            "Stored safety review is invalid.",
            None,
        )
    })?;
    let mut lines = vec![
        format!("has_danger: {}", review.has_danger),
        format!("warning_count: {}", review.warning_count),
        format!("masked_input_values: {}", review.masked_input_values),
        format!("masked_secret_texts: {}", review.masked_secret_texts),
    ];
    if review.warnings.is_empty() {
        lines.push("warnings: none".to_owned());
    } else {
        for warning in review.warnings {
            lines.push(format!(
                "warning: category={} category_label={} source={} source_label={}",
                quote_for_prompt(&warning.category),
                quote_for_prompt(&warning.category_label),
                quote_for_prompt(&warning.source),
                quote_for_prompt(&warning.source_label)
            ));
        }
    }
    Ok(lines.join("\n"))
}

fn quote_for_prompt(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
}

#[derive(Debug, Deserialize)]
struct StoredSafetyReview {
    #[serde(default)]
    has_danger: bool,
    #[serde(default)]
    warning_count: usize,
    #[serde(default)]
    warnings: Vec<StoredSafetyWarning>,
    #[serde(default)]
    masked_input_values: usize,
    #[serde(default)]
    masked_secret_texts: usize,
}

#[derive(Debug, Deserialize)]
struct StoredSafetyWarning {
    category: String,
    category_label: String,
    source: String,
    source_label: String,
}
