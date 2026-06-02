use screen_sidekick_screen_context::{RawButton, RawInput, RawPageMetadata, RawScreenContext};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DangerFinding {
    pub category: DangerCategory,
    pub source: DangerSource,
    pub keyword: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DangerCategory {
    Destructive,
    Publish,
    SendOrSubmit,
    Billing,
    Permission,
    RevokeOrDisconnect,
    SecretOrToken,
}

impl DangerCategory {
    #[must_use]
    pub fn warning_label(self) -> &'static str {
        match self {
            Self::Destructive => "destructive action",
            Self::Publish => "publish action",
            Self::SendOrSubmit => "send or submit action",
            Self::Billing => "billing or payment action",
            Self::Permission => "permission or ownership change",
            Self::RevokeOrDisconnect => "revoke, disconnect, or reset action",
            Self::SecretOrToken => "secret, token, key, or password change",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DangerSource {
    PageTitle,
    SelectedText,
    Button,
    InputLabel,
}

impl DangerSource {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::PageTitle => "page title",
            Self::SelectedText => "selected text",
            Self::Button => "button",
            Self::InputLabel => "input label",
        }
    }
}

#[must_use]
pub fn detect_danger(context: &RawScreenContext) -> Vec<DangerFinding> {
    let mut findings = Vec::new();

    if let Some(page) = &context.page {
        scan_page(page, &mut findings);
    }

    if let Some(selected_text) = context.selected_text.as_deref() {
        scan_text(DangerSource::SelectedText, selected_text, &mut findings);
    }

    for button in context.buttons() {
        if button.is_visible() {
            scan_button(button, &mut findings);
        }
    }

    for input in context.inputs() {
        if input.is_visible() {
            scan_input(input, &mut findings);
        }
    }

    findings
}

fn scan_page(page: &RawPageMetadata, findings: &mut Vec<DangerFinding>) {
    if let Some(title) = page.title.as_deref() {
        scan_text(DangerSource::PageTitle, title, findings);
    }
}

fn scan_button(button: &RawButton, findings: &mut Vec<DangerFinding>) {
    for text in [
        button.text.as_deref(),
        button.aria_label.as_deref(),
        button.title.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        scan_text(DangerSource::Button, text, findings);
    }
}

fn scan_input(input: &RawInput, findings: &mut Vec<DangerFinding>) {
    for text in [
        input.label.as_deref(),
        input.aria_label.as_deref(),
        input.title.as_deref(),
        input.placeholder.as_deref(),
        input.name.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        scan_text(DangerSource::InputLabel, text, findings);
    }
}

fn scan_text(source: DangerSource, text: &str, findings: &mut Vec<DangerFinding>) {
    let normalized = text.to_ascii_lowercase();

    for &(category, keywords) in DANGER_KEYWORDS {
        for &keyword in keywords {
            if normalized.contains(keyword) {
                findings.push(DangerFinding {
                    category,
                    source,
                    keyword: keyword.to_owned(),
                });
                break;
            }
        }
    }
}

const DANGER_KEYWORDS: &[(DangerCategory, &[&str])] = &[
    (
        DangerCategory::Destructive,
        &["delete", "remove", "destroy"],
    ),
    (DangerCategory::Publish, &["publish"]),
    (DangerCategory::SendOrSubmit, &["send", "submit"]),
    (DangerCategory::Billing, &["billing", "payment", "charge"]),
    (
        DangerCategory::Permission,
        &["permission", "admin", "owner"],
    ),
    (
        DangerCategory::RevokeOrDisconnect,
        &["revoke", "disconnect", "reset"],
    ),
    (
        DangerCategory::SecretOrToken,
        &[
            "secret", "token", "api key", "api-key", "api_key", "apikey", "api.key", "password",
        ],
    ),
];
