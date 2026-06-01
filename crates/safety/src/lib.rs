#![forbid(unsafe_code)]

use std::ops::Deref;

use screen_sidekick_screen_context::{
    CapturedAt, InputKind, RawButton, RawInput, RawPageMetadata, RawScreenContext,
    RawScreenshotMetadata, ScreenshotFormat, MASKED_VALUE, SCREEN_CONTEXT_SCHEMA_VERSION,
};

pub use screen_sidekick_safety_rules::{
    detect_danger, mask_secret_like_text, redact_secret_bearing_url, DangerCategory, DangerFinding,
    DangerSource, TextMaskResult, REDACTED_URL_VALUE,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafetyReview {
    sanitized_context: SanitizedScreenContext,
    findings: Vec<DangerFinding>,
    masked_input_values: usize,
    masked_secret_texts: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SanitizedScreenContext {
    schema_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    page: Option<SanitizedPageMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    selected_text: Option<PromptSafeText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    screenshot: Option<SanitizedScreenshotMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    buttons: Option<Vec<SanitizedButton>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    inputs: Option<Vec<SanitizedInput>>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SanitizedPageMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<SanitizedUrl>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<PromptSafeText>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SanitizedScreenshotMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<ScreenshotFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    captured_at: Option<CapturedAt>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SanitizedButton {
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<PromptSafeText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    aria_label: Option<PromptSafeText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<PromptSafeText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    disabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    visible: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SanitizedInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<InputKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<PromptSafeText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<PromptSafeText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    aria_label: Option<PromptSafeText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<PromptSafeText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    placeholder: Option<PromptSafeText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    disabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    visible: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<SanitizedInputValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SanitizedInputValue {
    text: PromptSafeText,
    masked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptSafeText(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SanitizedUrl(String);

#[derive(Debug, Clone, Copy)]
pub struct PromptSafetyReview<'a> {
    findings: &'a [DangerFinding],
    masked_input_values: usize,
    masked_secret_texts: usize,
}

impl SanitizedScreenContext {
    #[must_use]
    pub fn schema_version(&self) -> &str {
        &self.schema_version
    }

    #[must_use]
    pub fn page(&self) -> Option<&SanitizedPageMetadata> {
        self.page.as_ref()
    }

    #[must_use]
    pub fn selected_text(&self) -> Option<&str> {
        self.selected_text.as_deref()
    }

    #[must_use]
    pub fn screenshot(&self) -> Option<&SanitizedScreenshotMetadata> {
        self.screenshot.as_ref()
    }

    #[must_use]
    pub fn buttons(&self) -> &[SanitizedButton] {
        match self.buttons.as_deref() {
            Some(buttons) => buttons,
            None => &[],
        }
    }

    #[must_use]
    pub fn inputs(&self) -> &[SanitizedInput] {
        match self.inputs.as_deref() {
            Some(inputs) => inputs,
            None => &[],
        }
    }

    pub fn to_json_value(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::to_value(self)
    }

    pub fn to_pretty_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    fn new(
        page: Option<SanitizedPageMetadata>,
        selected_text: Option<PromptSafeText>,
        screenshot: Option<SanitizedScreenshotMetadata>,
        buttons: Option<Vec<SanitizedButton>>,
        inputs: Option<Vec<SanitizedInput>>,
    ) -> Self {
        Self {
            schema_version: SCREEN_CONTEXT_SCHEMA_VERSION.to_owned(),
            page,
            selected_text,
            screenshot,
            buttons,
            inputs,
        }
    }
}

impl SanitizedPageMetadata {
    #[must_use]
    pub fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }
}

impl SanitizedScreenshotMetadata {
    #[must_use]
    pub fn format(&self) -> Option<ScreenshotFormat> {
        self.format
    }

    #[must_use]
    pub fn width(&self) -> Option<u32> {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> Option<u32> {
        self.height
    }

    #[must_use]
    pub fn captured_at(&self) -> Option<&CapturedAt> {
        self.captured_at.as_ref()
    }
}

impl SanitizedButton {
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    #[must_use]
    pub fn aria_label(&self) -> Option<&str> {
        self.aria_label.as_deref()
    }

    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    #[must_use]
    pub fn is_visible(&self) -> bool {
        !matches!(self.visible, Some(false))
    }
}

impl SanitizedInput {
    #[must_use]
    pub fn kind(&self) -> Option<&InputKind> {
        self.kind.as_ref()
    }

    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    #[must_use]
    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    #[must_use]
    pub fn aria_label(&self) -> Option<&str> {
        self.aria_label.as_deref()
    }

    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    #[must_use]
    pub fn placeholder(&self) -> Option<&str> {
        self.placeholder.as_deref()
    }

    #[must_use]
    pub fn value(&self) -> Option<&SanitizedInputValue> {
        self.value.as_ref()
    }

    #[must_use]
    pub fn is_visible(&self) -> bool {
        !matches!(self.visible, Some(false))
    }
}

impl SanitizedInputValue {
    #[must_use]
    pub fn text(&self) -> &str {
        self.text.as_str()
    }

    #[must_use]
    pub fn is_masked(&self) -> bool {
        self.masked
    }

    fn masked() -> Self {
        Self {
            text: PromptSafeText::new_unchecked(MASKED_VALUE.to_owned()),
            masked: true,
        }
    }
}

impl PromptSafeText {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn new_unchecked(value: String) -> Self {
        Self(value)
    }
}

impl Deref for PromptSafeText {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl serde::Serialize for PromptSafeText {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl SanitizedUrl {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn new_unchecked(value: String) -> Self {
        Self(value)
    }
}

impl Deref for SanitizedUrl {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl serde::Serialize for SanitizedUrl {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'a> PromptSafetyReview<'a> {
    #[must_use]
    pub fn findings(&self) -> &'a [DangerFinding] {
        self.findings
    }

    #[must_use]
    pub fn masked_input_values(&self) -> usize {
        self.masked_input_values
    }

    #[must_use]
    pub fn masked_secret_texts(&self) -> usize {
        self.masked_secret_texts
    }
}

impl SafetyReview {
    #[must_use]
    pub fn has_danger(&self) -> bool {
        !self.findings.is_empty()
    }

    #[must_use]
    pub fn sanitized_context(&self) -> &SanitizedScreenContext {
        &self.sanitized_context
    }

    #[must_use]
    pub fn findings(&self) -> &[DangerFinding] {
        &self.findings
    }

    #[must_use]
    pub fn masked_input_values(&self) -> usize {
        self.masked_input_values
    }

    #[must_use]
    pub fn masked_secret_texts(&self) -> usize {
        self.masked_secret_texts
    }

    #[must_use]
    pub fn prompt_safety(&self) -> PromptSafetyReview<'_> {
        PromptSafetyReview {
            findings: &self.findings,
            masked_input_values: self.masked_input_values,
            masked_secret_texts: self.masked_secret_texts,
        }
    }
}

#[must_use]
pub fn review_screen_context(context: &RawScreenContext) -> SafetyReview {
    let findings = detect_danger(context);
    let (sanitized_context, masked_secret_texts, masked_input_values) =
        sanitize_screen_context(context);

    SafetyReview {
        sanitized_context,
        findings,
        masked_input_values,
        masked_secret_texts,
    }
}

fn sanitize_screen_context(context: &RawScreenContext) -> (SanitizedScreenContext, usize, usize) {
    let (page, page_masked) = sanitize_page(context.page.as_ref());
    let (selected_text, selected_text_masked) =
        sanitize_optional_prompt_text(context.selected_text.as_deref());
    let screenshot = context.screenshot.as_ref().map(sanitize_screenshot);
    let (buttons, buttons_masked) = sanitize_buttons(context.buttons());
    let (inputs, inputs_masked, masked_input_values) = sanitize_inputs(context.inputs());

    (
        SanitizedScreenContext::new(page, selected_text, screenshot, buttons, inputs),
        page_masked + selected_text_masked + buttons_masked + inputs_masked,
        masked_input_values,
    )
}

fn sanitize_page(page: Option<&RawPageMetadata>) -> (Option<SanitizedPageMetadata>, usize) {
    let Some(page) = page else {
        return (None, 0);
    };

    let (url, url_masked) = sanitize_optional_url(page.url.as_deref());
    let (title, title_masked) = sanitize_optional_prompt_text(page.title.as_deref());
    if url.is_none() && title.is_none() {
        (None, url_masked + title_masked)
    } else {
        (
            Some(SanitizedPageMetadata { url, title }),
            url_masked + title_masked,
        )
    }
}

fn sanitize_screenshot(screenshot: &RawScreenshotMetadata) -> SanitizedScreenshotMetadata {
    SanitizedScreenshotMetadata {
        format: screenshot.format,
        width: screenshot.width,
        height: screenshot.height,
        captured_at: screenshot.captured_at.clone(),
    }
}

fn sanitize_buttons(buttons: &[RawButton]) -> (Option<Vec<SanitizedButton>>, usize) {
    if buttons.is_empty() {
        return (None, 0);
    }

    let mut masked = 0;
    let sanitized = buttons
        .iter()
        .map(|button| {
            let (text, text_masked) = sanitize_optional_prompt_text(button.text.as_deref());
            let (aria_label, aria_label_masked) =
                sanitize_optional_prompt_text(button.aria_label.as_deref());
            let (title, title_masked) = sanitize_optional_prompt_text(button.title.as_deref());
            masked += text_masked + aria_label_masked + title_masked;
            SanitizedButton {
                text,
                aria_label,
                title,
                disabled: button.disabled,
                visible: button.visible,
            }
        })
        .collect();

    (Some(sanitized), masked)
}

fn sanitize_inputs(inputs: &[RawInput]) -> (Option<Vec<SanitizedInput>>, usize, usize) {
    if inputs.is_empty() {
        return (None, 0, 0);
    }

    let mut masked_secret_texts = 0;
    let mut masked_input_values = 0;
    let sanitized = inputs
        .iter()
        .map(|input| {
            let (name, name_masked) = sanitize_optional_prompt_text(input.name.as_deref());
            let (label, label_masked) = sanitize_optional_prompt_text(input.label.as_deref());
            let (aria_label, aria_label_masked) =
                sanitize_optional_prompt_text(input.aria_label.as_deref());
            let (title, title_masked) = sanitize_optional_prompt_text(input.title.as_deref());
            let (placeholder, placeholder_masked) =
                sanitize_optional_prompt_text(input.placeholder.as_deref());
            masked_secret_texts +=
                name_masked + label_masked + aria_label_masked + title_masked + placeholder_masked;
            let value = if input.value.is_some() {
                masked_input_values += 1;
                Some(SanitizedInputValue::masked())
            } else {
                None
            };

            SanitizedInput {
                kind: input.kind.clone(),
                name,
                label,
                aria_label,
                title,
                placeholder,
                disabled: input.disabled,
                visible: input.visible,
                value,
            }
        })
        .collect();

    (Some(sanitized), masked_secret_texts, masked_input_values)
}

fn sanitize_optional_prompt_text(text: Option<&str>) -> (Option<PromptSafeText>, usize) {
    let Some(text) = text else {
        return (None, 0);
    };

    let result = mask_secret_like_text(text);
    let masked = usize::from(result.was_masked);
    (Some(PromptSafeText::new_unchecked(result.text)), masked)
}

fn sanitize_optional_url(url: Option<&str>) -> (Option<SanitizedUrl>, usize) {
    let Some(url) = url else {
        return (None, 0);
    };

    let result = redact_secret_bearing_url(url);
    let masked = usize::from(result.was_masked);
    (Some(SanitizedUrl::new_unchecked(result.text)), masked)
}
