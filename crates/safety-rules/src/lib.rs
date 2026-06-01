#![forbid(unsafe_code)]

pub mod danger;
pub mod redaction;

pub use danger::{detect_danger, DangerCategory, DangerFinding, DangerSource};
pub use redaction::{
    mask_secret_like_text, redact_secret_bearing_url, TextMaskResult, REDACTED_URL_VALUE,
};
