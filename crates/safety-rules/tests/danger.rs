use screen_sidekick_safety_rules::{detect_danger, DangerCategory, DangerFinding, DangerSource};
use screen_sidekick_screen_context::{RawButton, RawInput, RawPageMetadata, RawScreenContext};

#[test]
fn detects_visible_button_danger() {
    let mut context = RawScreenContext::new();
    context.buttons = Some(vec![RawButton {
        text: Some("Delete account".to_owned()),
        visible: Some(true),
        ..RawButton::default()
    }]);

    let findings = detect_danger(&context);

    assert_eq!(
        findings,
        vec![DangerFinding {
            category: DangerCategory::Destructive,
            source: DangerSource::Button,
            keyword: "delete".to_owned(),
        }]
    );
}

#[test]
fn ignores_invisible_controls() {
    let mut context = RawScreenContext::new();
    context.buttons = Some(vec![RawButton {
        text: Some("Delete account".to_owned()),
        visible: Some(false),
        ..RawButton::default()
    }]);
    context.inputs = Some(vec![RawInput {
        label: Some("Reset password".to_owned()),
        visible: Some(false),
        ..RawInput::default()
    }]);

    let findings = detect_danger(&context);

    assert!(findings.is_empty());
}

#[test]
fn detects_page_selected_text_and_input_label_sources() {
    let mut context = RawScreenContext::new();
    context.page = Some(RawPageMetadata {
        title: Some("Billing settings".to_owned()),
        url: None,
    });
    context.selected_text = Some("Submit invoice".to_owned());
    context.inputs = Some(vec![RawInput {
        placeholder: Some("Owner email".to_owned()),
        visible: Some(true),
        ..RawInput::default()
    }]);

    let findings = detect_danger(&context);

    assert!(findings.iter().any(|finding| {
        finding.category == DangerCategory::Billing && finding.source == DangerSource::PageTitle
    }));
    assert!(findings.iter().any(|finding| {
        finding.category == DangerCategory::SendOrSubmit
            && finding.source == DangerSource::SelectedText
    }));
    assert!(findings.iter().any(|finding| {
        finding.category == DangerCategory::Permission && finding.source == DangerSource::InputLabel
    }));
}

#[test]
fn detects_api_key_identifier_variants() {
    let mut context = RawScreenContext::new();
    context.buttons = Some(vec![RawButton {
        text: Some("Rotate apiKey".to_owned()),
        visible: Some(true),
        ..RawButton::default()
    }]);
    context.inputs = Some(vec![RawInput {
        name: Some("api_key".to_owned()),
        visible: Some(true),
        ..RawInput::default()
    }]);

    let findings = detect_danger(&context);

    assert!(findings.iter().any(|finding| {
        finding.category == DangerCategory::SecretOrToken
            && finding.source == DangerSource::Button
            && finding.keyword == "apikey"
    }));
    assert!(findings.iter().any(|finding| {
        finding.category == DangerCategory::SecretOrToken
            && finding.source == DangerSource::InputLabel
            && finding.keyword == "api_key"
    }));
}
