use screen_sidekick_safety_rules::{detect_danger, DangerCategory, DangerFinding, DangerSource};
use screen_sidekick_screen_context::{Button, Input, PageMetadata, ScreenContext};

#[test]
fn detects_visible_button_danger() {
    let mut context = ScreenContext::new();
    context.buttons = Some(vec![Button {
        text: Some("Delete account".to_owned()),
        visible: Some(true),
        ..Button::default()
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
    let mut context = ScreenContext::new();
    context.buttons = Some(vec![Button {
        text: Some("Delete account".to_owned()),
        visible: Some(false),
        ..Button::default()
    }]);
    context.inputs = Some(vec![Input {
        label: Some("Reset password".to_owned()),
        visible: Some(false),
        ..Input::default()
    }]);

    let findings = detect_danger(&context);

    assert!(findings.is_empty());
}

#[test]
fn detects_page_selected_text_and_input_label_sources() {
    let mut context = ScreenContext::new();
    context.page = Some(PageMetadata {
        title: Some("Billing settings".to_owned()),
        url: None,
    });
    context.selected_text = Some("Submit invoice".to_owned());
    context.inputs = Some(vec![Input {
        placeholder: Some("Owner email".to_owned()),
        visible: Some(true),
        ..Input::default()
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
    let mut context = ScreenContext::new();
    context.buttons = Some(vec![Button {
        text: Some("Rotate apiKey".to_owned()),
        visible: Some(true),
        ..Button::default()
    }]);
    context.inputs = Some(vec![Input {
        name: Some("api_key".to_owned()),
        visible: Some(true),
        ..Input::default()
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
