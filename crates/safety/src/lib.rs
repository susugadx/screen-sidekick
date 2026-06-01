#![forbid(unsafe_code)]

use screen_sidekick_screen_context::{Button, Input, InputValue, PageMetadata, RawScreenContext};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SanitizedScreenContext {
    context: RawScreenContext,
}

impl SanitizedScreenContext {
    #[must_use]
    pub fn schema_version(&self) -> &str {
        &self.context.schema_version
    }

    #[must_use]
    pub fn page(&self) -> Option<&PageMetadata> {
        self.context.page.as_ref()
    }

    #[must_use]
    pub fn selected_text(&self) -> Option<&str> {
        self.context.selected_text.as_deref()
    }

    #[must_use]
    pub fn buttons(&self) -> &[Button] {
        self.context.buttons()
    }

    #[must_use]
    pub fn inputs(&self) -> &[Input] {
        self.context.inputs()
    }

    fn new(context: RawScreenContext) -> Self {
        Self { context }
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
}

#[must_use]
pub fn review_screen_context(context: &RawScreenContext) -> SafetyReview {
    let findings = detect_danger(context);
    let mut sanitized_context = context.clone();
    let masked_secret_texts = mask_prompt_text_fields(&mut sanitized_context)
        + redact_page_url_fields(&mut sanitized_context);
    let masked_input_values = mask_input_values(&mut sanitized_context);

    SafetyReview {
        sanitized_context: SanitizedScreenContext::new(sanitized_context),
        findings,
        masked_input_values,
        masked_secret_texts,
    }
}

fn mask_prompt_text_fields(context: &mut RawScreenContext) -> usize {
    let mut masked = 0;

    if let Some(page) = context.page.as_mut() {
        masked += mask_optional_text(&mut page.title);
    }

    if let Some(selected_text) = context.selected_text.as_mut() {
        masked += mask_text(selected_text);
    }

    if let Some(buttons) = context.buttons.as_mut() {
        for button in buttons {
            masked += mask_optional_text(&mut button.text);
            masked += mask_optional_text(&mut button.aria_label);
            masked += mask_optional_text(&mut button.title);
        }
    }

    if let Some(inputs) = context.inputs.as_mut() {
        for input in inputs {
            masked += mask_optional_text(&mut input.name);
            masked += mask_optional_text(&mut input.label);
            masked += mask_optional_text(&mut input.aria_label);
            masked += mask_optional_text(&mut input.title);
            masked += mask_optional_text(&mut input.placeholder);
        }
    }

    masked
}

fn mask_optional_text(text: &mut Option<String>) -> usize {
    match text.as_mut() {
        Some(text) => mask_text(text),
        None => 0,
    }
}

fn mask_text(text: &mut String) -> usize {
    let result = mask_secret_like_text(text);
    if result.was_masked {
        *text = result.text;
        1
    } else {
        0
    }
}

fn redact_page_url_fields(context: &mut RawScreenContext) -> usize {
    let mut masked = 0;

    if let Some(page) = context.page.as_mut() {
        if let Some(url) = page.url.as_mut() {
            let result = redact_secret_bearing_url(url);
            if result.was_masked {
                *url = result.text;
                masked += 1;
            }
        }
    }

    masked
}

fn mask_input_values(context: &mut RawScreenContext) -> usize {
    let mut masked = 0;

    if let Some(inputs) = context.inputs.as_mut() {
        for input in inputs {
            if input.value.is_some() {
                input.value = Some(InputValue::masked());
                masked += 1;
            }
        }
    }

    masked
}
