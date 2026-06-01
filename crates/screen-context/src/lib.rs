#![forbid(unsafe_code)]

use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub const SCREEN_CONTEXT_SCHEMA_VERSION: &str = "0.1";
pub const MASKED_VALUE: &str = "[masked]";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawScreenContext {
    pub schema_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<RawPageMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<RawScreenshotMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buttons: Option<Vec<RawButton>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inputs: Option<Vec<RawInput>>,
}

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
    pub fn buttons(&self) -> &[RawButton] {
        match self.buttons.as_deref() {
            Some(buttons) => buttons,
            None => &[],
        }
    }

    #[must_use]
    pub fn inputs(&self) -> &[RawInput] {
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
pub struct RawPageMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawScreenshotMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<ScreenshotFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captured_at: Option<CapturedAt>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScreenshotFormat {
    Png,
    Jpeg,
    Webp,
}

impl ScreenshotFormat {
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "png" => Some(Self::Png),
            "jpeg" => Some(Self::Jpeg),
            "webp" => Some(Self::Webp),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Webp => "webp",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedAt(String);

impl CapturedAt {
    #[must_use]
    pub fn parse_extension_iso_millis_utc(value: String) -> Option<Self> {
        if is_valid_extension_timestamp(&value) {
            Some(Self(value))
        } else {
            None
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for CapturedAt {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for CapturedAt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse_extension_iso_millis_utc(value)
            .ok_or_else(|| serde::de::Error::custom("invalid captured_at timestamp"))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawButton {
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

impl RawButton {
    #[must_use]
    pub fn is_visible(&self) -> bool {
        !matches!(self.visible, Some(false))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawInput {
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
    pub value: Option<RawInputValue>,
}

impl RawInput {
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
pub struct RawInputValue {
    text: String,
    masked: bool,
}

impl RawInputValue {
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

fn is_valid_extension_timestamp(timestamp: &str) -> bool {
    let bytes = timestamp.as_bytes();
    if bytes.len() != 24
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'.'
        || bytes[23] != b'Z'
    {
        return false;
    }

    let Some(year) = parse_fixed_digits(bytes, 0, 4) else {
        return false;
    };
    let Some(month) = parse_fixed_digits(bytes, 5, 2) else {
        return false;
    };
    let Some(day) = parse_fixed_digits(bytes, 8, 2) else {
        return false;
    };
    let Some(hour) = parse_fixed_digits(bytes, 11, 2) else {
        return false;
    };
    let Some(minute) = parse_fixed_digits(bytes, 14, 2) else {
        return false;
    };
    let Some(second) = parse_fixed_digits(bytes, 17, 2) else {
        return false;
    };

    parse_fixed_digits(bytes, 20, 3).is_some()
        && (1..=12).contains(&month)
        && (1..=days_in_month(year, month)).contains(&day)
        && hour <= 23
        && minute <= 59
        && second <= 59
}

fn parse_fixed_digits(bytes: &[u8], start: usize, len: usize) -> Option<u32> {
    let mut value = 0;
    for byte in bytes.get(start..start + len)? {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value * 10 + u32::from(*byte - b'0');
    }
    Some(value)
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: u32) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}
