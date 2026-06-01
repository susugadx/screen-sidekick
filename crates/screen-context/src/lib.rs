#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

pub const SCREEN_CONTEXT_SCHEMA_VERSION: &str = "0.1";
pub const MASKED_VALUE: &str = "[masked]";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawScreenContext {
    pub schema_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<PageMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<ScreenshotMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buttons: Option<Vec<Button>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inputs: Option<Vec<Input>>,
}

pub type ScreenContext = RawScreenContext;

impl RawScreenContext {
    #[must_use]
    pub fn new() -> Self {
        Self {
            schema_version: SCREEN_CONTEXT_SCHEMA_VERSION.to_owned(),
            page: None,
            selected_text: None,
            screenshot: None,
            buttons: None,
            inputs: None,
        }
    }

    #[must_use]
    pub fn buttons(&self) -> &[Button] {
        match self.buttons.as_deref() {
            Some(buttons) => buttons,
            None => &[],
        }
    }

    #[must_use]
    pub fn inputs(&self) -> &[Input] {
        match self.inputs.as_deref() {
            Some(inputs) => inputs,
            None => &[],
        }
    }
}

impl Default for RawScreenContext {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenshotMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captured_at: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Button {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aria_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
}

impl Button {
    #[must_use]
    pub fn is_visible(&self) -> bool {
        !matches!(self.visible, Some(false))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Input {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<InputKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aria_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<InputValue>,
}

impl Input {
    #[must_use]
    pub fn is_visible(&self) -> bool {
        !matches!(self.visible, Some(false))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    Text,
    Search,
    Email,
    Password,
    Number,
    Tel,
    Url,
    Checkbox,
    Radio,
    Select,
    Textarea,
    ContentEditable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputValue {
    text: String,
    masked: bool,
}

impl InputValue {
    #[must_use]
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            masked: false,
        }
    }

    #[must_use]
    pub fn masked() -> Self {
        Self {
            text: MASKED_VALUE.to_owned(),
            masked: true,
        }
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub fn is_masked(&self) -> bool {
        self.masked
    }
}
