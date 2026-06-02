#![forbid(unsafe_code)]

use std::{error::Error, fmt};

use screen_sidekick_prompt::build_codex_prompt;
use screen_sidekick_safety::{review_screen_context, DangerCategory, DangerSource, SafetyReview};
use screen_sidekick_screen_context::{
    CapturedAt, InputKind, RawButton, RawInput, RawPageMetadata, RawScreenContext,
    RawScreenshotMetadata, ScreenshotFormat,
};
use serde::{Deserialize, Serialize};

pub const RAW_BROWSER_CONTEXT_SCHEMA_VERSION: &str = "raw_browser_context.v0.1";
pub const CAPTURE_RESPONSE_SCHEMA_VERSION: &str = "capture_response.v0.1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawBrowserContext {
    pub schema_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<RawBrowserPage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<RawBrowserScreenshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buttons: Option<Vec<RawBrowserButton>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inputs: Option<Vec<RawBrowserInput>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawBrowserPage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawBrowserScreenshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub captured_at: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawBrowserButton {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aria_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawBrowserInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<InputKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aria_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureResponse {
    pub schema_version: String,
    pub screen_context_json: String,
    pub prompt_text: String,
    pub safety: CaptureSafetySummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureSafetySummary {
    pub has_danger: bool,
    pub warning_count: usize,
    pub warnings: Vec<CaptureSafetyWarning>,
    pub masked_input_values: usize,
    pub masked_secret_texts: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureSafetyWarning {
    pub category: String,
    pub category_label: String,
    pub source: String,
    pub source_label: String,
}

#[derive(Debug)]
pub enum CapturePipelineError {
    UnsupportedSchemaVersion,
    SerializeSanitizedContext(serde_json::Error),
}

impl fmt::Display for CapturePipelineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchemaVersion => write!(
                formatter,
                "unsupported raw browser context schema version; expected {RAW_BROWSER_CONTEXT_SCHEMA_VERSION}"
            ),
            Self::SerializeSanitizedContext(_) => {
                formatter.write_str("failed to serialize sanitized screen context")
            }
        }
    }
}

impl Error for CapturePipelineError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::UnsupportedSchemaVersion => None,
            Self::SerializeSanitizedContext(error) => Some(error),
        }
    }
}

pub fn process_capture(
    request: RawBrowserContext,
) -> Result<CaptureResponse, CapturePipelineError> {
    if request.schema_version != RAW_BROWSER_CONTEXT_SCHEMA_VERSION {
        return Err(CapturePipelineError::UnsupportedSchemaVersion);
    }

    let context = normalize_raw_browser_context(request);
    let review = review_screen_context(&context);
    let screen_context_json = review
        .sanitized_context()
        .to_pretty_json()
        .map_err(CapturePipelineError::SerializeSanitizedContext)?;
    let prompt_safety = review.prompt_safety();
    let prompt = build_codex_prompt(review.sanitized_context(), &prompt_safety);
    let safety = build_safety_summary(&review);

    Ok(CaptureResponse {
        schema_version: CAPTURE_RESPONSE_SCHEMA_VERSION.to_owned(),
        screen_context_json,
        prompt_text: prompt.text,
        safety,
    })
}

pub fn normalize_raw_browser_context(request: RawBrowserContext) -> RawScreenContext {
    let mut context = RawScreenContext::new();
    context.page = request.page.map(|page| RawPageMetadata {
        url: page.url,
        title: page.title,
    });
    context.selected_text = request.selected_text;
    context.screenshot = request.screenshot.and_then(normalize_screenshot_metadata);
    context.buttons = request.buttons.map(|buttons| {
        buttons
            .into_iter()
            .map(|button| RawButton {
                text: button.text,
                aria_label: button.aria_label,
                title: button.title,
                disabled: button.disabled,
                visible: button.visible,
            })
            .collect()
    });
    context.inputs = request.inputs.map(|inputs| {
        inputs
            .into_iter()
            .map(|input| RawInput {
                kind: input.kind,
                name: input.name,
                label: input.label,
                aria_label: input.aria_label,
                title: input.title,
                placeholder: input.placeholder,
                disabled: input.disabled,
                visible: input.visible,
                value: None,
            })
            .collect()
    });
    context
}

fn normalize_screenshot_metadata(
    screenshot: RawBrowserScreenshot,
) -> Option<RawScreenshotMetadata> {
    let metadata = RawScreenshotMetadata {
        format: screenshot
            .format
            .and_then(|format| ScreenshotFormat::parse(&format)),
        width: screenshot.width,
        height: screenshot.height,
        captured_at: screenshot
            .captured_at
            .and_then(CapturedAt::parse_extension_iso_millis_utc),
    };

    if metadata.format.is_some()
        || metadata.width.is_some()
        || metadata.height.is_some()
        || metadata.captured_at.is_some()
    {
        Some(metadata)
    } else {
        None
    }
}

fn build_safety_summary(review: &SafetyReview) -> CaptureSafetySummary {
    let warnings = review
        .findings()
        .iter()
        .map(|finding| CaptureSafetyWarning {
            category: danger_category_id(finding.category).to_owned(),
            category_label: finding.category.warning_label().to_owned(),
            source: danger_source_id(finding.source).to_owned(),
            source_label: finding.source.label().to_owned(),
        })
        .collect::<Vec<_>>();

    CaptureSafetySummary {
        has_danger: review.has_danger(),
        warning_count: warnings.len(),
        warnings,
        masked_input_values: review.masked_input_values(),
        masked_secret_texts: review.masked_secret_texts(),
    }
}

fn danger_category_id(category: DangerCategory) -> &'static str {
    match category {
        DangerCategory::Destructive => "destructive",
        DangerCategory::Publish => "publish",
        DangerCategory::SendOrSubmit => "send_or_submit",
        DangerCategory::Billing => "billing",
        DangerCategory::Permission => "permission",
        DangerCategory::RevokeOrDisconnect => "revoke_or_disconnect",
        DangerCategory::SecretOrToken => "secret_or_token",
    }
}

fn danger_source_id(source: DangerSource) -> &'static str {
    match source {
        DangerSource::PageTitle => "page_title",
        DangerSource::SelectedText => "selected_text",
        DangerSource::Button => "button",
        DangerSource::InputLabel => "input_label",
    }
}
