use screen_sidekick_safety::{review_screen_context, DangerCategory, DangerSource};
use screen_sidekick_screen_context::{
    Button, Input, InputKind, InputValue, PageMetadata, ScreenContext, MASKED_VALUE,
};

#[test]
fn review_keeps_danger_findings_from_rules() {
    let mut context = ScreenContext::new();
    context.buttons = Some(vec![Button {
        text: Some("Delete user".to_owned()),
        visible: Some(true),
        ..Button::default()
    }]);

    let review = review_screen_context(&context);

    assert!(review.has_danger());
    assert_eq!(review.findings().len(), 1);
    assert_eq!(review.findings()[0].category, DangerCategory::Destructive);
    assert_eq!(review.findings()[0].source, DangerSource::Button);
}

#[test]
fn review_sanitizes_prompt_visible_fields_and_input_values() {
    let mut context = ScreenContext::new();
    context.page = Some(PageMetadata {
        url: Some("https://example.test/admin?access_token=URLSECRET&state=keep".to_owned()),
        title: Some("token=sk-PAGESECRET".to_owned()),
    });
    context.selected_text = Some("secret=sk-SELECTEDSECRET".to_owned());
    context.buttons = Some(vec![Button {
        text: Some("api_key=BUTTONTEXTSECRET".to_owned()),
        aria_label: Some("token=sk-BUTTONARIASECRET".to_owned()),
        title: Some("password=BUTTONTITLESECRET".to_owned()),
        visible: Some(true),
        ..Button::default()
    }]);
    context.inputs = Some(vec![Input {
        kind: Some(InputKind::Text),
        name: Some("token=sk-INPUTNAMESECRET".to_owned()),
        label: Some("secret=sk-INPUTLABELSECRET".to_owned()),
        aria_label: Some("api_key=INPUTARIASECRET".to_owned()),
        title: Some("password=INPUTTITLESECRET".to_owned()),
        placeholder: Some("otp=INPUTPLACEHOLDERSECRET".to_owned()),
        value: Some(InputValue::plain("person@example.test")),
        ..Input::default()
    }]);

    let review = review_screen_context(&context);
    let sanitized = review.sanitized_context();
    let page = sanitized.page().expect("page is retained");
    let button = &sanitized.buttons()[0];
    let input = &sanitized.inputs()[0];
    let input_value = input.value.as_ref().expect("input value is retained");

    assert_eq!(
        page.url.as_deref(),
        Some("https://example.test/admin?access_token=[REDACTED]&state=keep")
    );
    assert_eq!(page.title.as_deref(), Some(MASKED_VALUE));
    assert_eq!(sanitized.selected_text(), Some(MASKED_VALUE));
    assert_eq!(button.text.as_deref(), Some(MASKED_VALUE));
    assert_eq!(button.aria_label.as_deref(), Some(MASKED_VALUE));
    assert_eq!(button.title.as_deref(), Some(MASKED_VALUE));
    assert_eq!(input.name.as_deref(), Some(MASKED_VALUE));
    assert_eq!(input.label.as_deref(), Some(MASKED_VALUE));
    assert_eq!(input.aria_label.as_deref(), Some(MASKED_VALUE));
    assert_eq!(input.title.as_deref(), Some(MASKED_VALUE));
    assert_eq!(input.placeholder.as_deref(), Some(MASKED_VALUE));
    assert!(input_value.is_masked());
    assert_eq!(input_value.text(), MASKED_VALUE);
    assert_eq!(review.masked_secret_texts(), 11);
    assert_eq!(review.masked_input_values(), 1);
}

#[test]
fn review_sanitizes_normalized_secret_key_variants_in_prompt_visible_fields() {
    let mut context = ScreenContext::new();
    context.page = Some(PageMetadata {
        url: Some(
            "https://example.test/admin?accessToken=URLSECRET&state=keep#/reset/sk-FRAGSECRET"
                .to_owned(),
        ),
        title: Some("clientSecret=TITLESECRET".to_owned()),
    });
    context.selected_text = Some("refreshToken=SELECTEDSECRET".to_owned());
    context.buttons = Some(vec![Button {
        text: Some("idToken=BUTTONSECRET".to_owned()),
        visible: Some(true),
        ..Button::default()
    }]);
    context.inputs = Some(vec![Input {
        label: Some("client-secret: INPUTSECRET".to_owned()),
        ..Input::default()
    }]);

    let review = review_screen_context(&context);
    let sanitized = review.sanitized_context();
    let page = sanitized.page().expect("page is retained");

    assert_eq!(
        page.url.as_deref(),
        Some("https://example.test/admin?accessToken=[REDACTED]&state=keep#[REDACTED]")
    );
    assert_eq!(page.title.as_deref(), Some(MASKED_VALUE));
    assert_eq!(sanitized.selected_text(), Some(MASKED_VALUE));
    assert_eq!(sanitized.buttons()[0].text.as_deref(), Some(MASKED_VALUE));
    assert_eq!(sanitized.inputs()[0].label.as_deref(), Some(MASKED_VALUE));
    assert_eq!(review.masked_secret_texts(), 5);
}

#[test]
fn review_sanitizes_benign_looking_secret_key_values() {
    let mut context = ScreenContext::new();
    context.page = Some(PageMetadata {
        url: Some(
            "https://example.test/admin?code=EXAMPLE&status_code=200&promo_code=SAVE20&state=keep"
                .to_owned(),
        ),
        title: Some("token=theme".to_owned()),
    });
    context.selected_text = Some("secret=not-a-secret-value-for-docs".to_owned());
    context.buttons = Some(vec![Button {
        text: Some("token=featureflagrollout2026".to_owned()),
        visible: Some(true),
        ..Button::default()
    }]);
    context.inputs = Some(vec![Input {
        label: Some("secret=not-a-secret".to_owned()),
        placeholder: Some("promo_code=SAVE20".to_owned()),
        ..Input::default()
    }]);

    let review = review_screen_context(&context);
    let sanitized = review.sanitized_context();

    assert_eq!(
        sanitized.page().and_then(|page| page.url.as_deref()),
        Some("https://example.test/admin?code=[REDACTED]&status_code=200&promo_code=SAVE20&state=keep")
    );
    assert_eq!(
        sanitized.page().and_then(|page| page.title.as_deref()),
        Some(MASKED_VALUE)
    );
    assert_eq!(sanitized.selected_text(), Some(MASKED_VALUE));
    assert_eq!(sanitized.buttons()[0].text.as_deref(), Some(MASKED_VALUE));
    assert_eq!(sanitized.inputs()[0].label.as_deref(), Some(MASKED_VALUE));
    assert_eq!(
        sanitized.inputs()[0].placeholder.as_deref(),
        Some("promo_code=SAVE20")
    );
    assert_eq!(review.masked_secret_texts(), 5);
    assert_eq!(review.masked_input_values(), 0);
}

#[test]
fn review_sanitizes_url_path_and_encoded_nested_text_secrets() {
    let mut context = ScreenContext::new();
    context.page = Some(PageMetadata {
        url: Some("https://example.test/reset/sk-LIVESECRET?state=keep".to_owned()),
        title: Some(
            "redirect=https%3A%2F%2Fidp.test%2Fcallback%3Faccess_token%3DTITLESECRET".to_owned(),
        ),
    });
    context.selected_text =
        Some("https%3A%2F%2Fidp.test%2Fcallback%3Faccess_token%3DSELECTEDSECRET".to_owned());
    context.buttons = Some(vec![Button {
        text: Some(
            "redirect=https%3A%2F%2Fidp.test%2Fcallback%3Faccess_token%3DBUTTONSECRET".to_owned(),
        ),
        visible: Some(true),
        ..Button::default()
    }]);
    context.inputs = Some(vec![Input {
        label: Some(
            "redirect=https%3A%2F%2Fidp.test%2Fcallback%3Faccess_token%3DINPUTSECRET".to_owned(),
        ),
        ..Input::default()
    }]);

    let review = review_screen_context(&context);
    let sanitized = review.sanitized_context();

    assert_eq!(
        sanitized.page().and_then(|page| page.url.as_deref()),
        Some("https://example.test/reset/[REDACTED]?state=keep")
    );
    assert_eq!(
        sanitized.page().and_then(|page| page.title.as_deref()),
        Some(MASKED_VALUE)
    );
    assert_eq!(sanitized.selected_text(), Some(MASKED_VALUE));
    assert_eq!(sanitized.buttons()[0].text.as_deref(), Some(MASKED_VALUE));
    assert_eq!(sanitized.inputs()[0].label.as_deref(), Some(MASKED_VALUE));
    assert_eq!(review.masked_secret_texts(), 5);
}

#[test]
fn review_sanitizes_secret_label_value_sequences() {
    let mut context = ScreenContext::new();
    context.page = Some(PageMetadata {
        url: Some("https://example.test/admin?state=keep".to_owned()),
        title: Some("password swordfish".to_owned()),
    });
    context.selected_text = Some("passcode sesame".to_owned());
    context.buttons = Some(vec![Button {
        text: Some("client secret swordfish".to_owned()),
        visible: Some(true),
        ..Button::default()
    }]);
    context.inputs = Some(vec![Input {
        label: Some("verification code 123456".to_owned()),
        ..Input::default()
    }]);

    let review = review_screen_context(&context);
    let sanitized = review.sanitized_context();

    assert_eq!(
        sanitized.page().and_then(|page| page.title.as_deref()),
        Some(MASKED_VALUE)
    );
    assert_eq!(sanitized.selected_text(), Some(MASKED_VALUE));
    assert_eq!(sanitized.buttons()[0].text.as_deref(), Some(MASKED_VALUE));
    assert_eq!(sanitized.inputs()[0].label.as_deref(), Some(MASKED_VALUE));
    assert_eq!(review.masked_secret_texts(), 4);
}

#[test]
fn review_sanitizes_opaque_value_aware_tokens_and_card_like_digits() {
    let mut context = ScreenContext::new();
    context.page = Some(PageMetadata {
        url: Some(
            "https://example.test/admin?token=abc123def456ghi789jkl012&state=keep".to_owned(),
        ),
        title: Some("card4111111111111".to_owned()),
    });
    context.selected_text = Some("abcdef1234567890abcdef12".to_owned());
    context.buttons = Some(vec![Button {
        text: Some("card=4111111111111".to_owned()),
        visible: Some(true),
        ..Button::default()
    }]);
    context.inputs = Some(vec![Input {
        label: Some("code=abc123def456ghi789jkl012".to_owned()),
        ..Input::default()
    }]);

    let review = review_screen_context(&context);
    let sanitized = review.sanitized_context();
    let page = sanitized.page().expect("page is retained");

    assert_eq!(
        page.url.as_deref(),
        Some("https://example.test/admin?token=[REDACTED]&state=keep")
    );
    assert_eq!(page.title.as_deref(), Some(MASKED_VALUE));
    assert_eq!(sanitized.selected_text(), Some(MASKED_VALUE));
    assert_eq!(sanitized.buttons()[0].text.as_deref(), Some(MASKED_VALUE));
    assert_eq!(sanitized.inputs()[0].label.as_deref(), Some(MASKED_VALUE));
    assert_eq!(review.masked_secret_texts(), 5);
}

#[test]
fn review_preserves_safe_context_fields() {
    let mut context = ScreenContext::new();
    context.page = Some(PageMetadata {
        url: Some(
            "https://example.test/admin?status_code=200&country_code=JP&promo_code=SAVE20&readable=release-notes-2026-june&long_code=release-notes-2026-june&state=keep"
                .to_owned(),
        ),
        title: Some("Verification code settings".to_owned()),
    });
    context.selected_text = Some("code=EXAMPLE".to_owned());
    context.buttons = Some(vec![Button {
        text: Some("release-notes-2026-june".to_owned()),
        visible: Some(true),
        ..Button::default()
    }]);
    context.inputs = Some(vec![Input {
        label: Some("promo_code=SAVE20".to_owned()),
        placeholder: Some("person@example.test".to_owned()),
        ..Input::default()
    }]);

    let review = review_screen_context(&context);
    let sanitized = review.sanitized_context();

    assert_eq!(sanitized.schema_version(), context.schema_version);
    assert_eq!(
        sanitized.page().and_then(|page| page.url.as_deref()),
        Some("https://example.test/admin?status_code=200&country_code=JP&promo_code=SAVE20&readable=release-notes-2026-june&long_code=release-notes-2026-june&state=keep")
    );
    assert_eq!(
        sanitized.page().and_then(|page| page.title.as_deref()),
        Some("Verification code settings")
    );
    assert_eq!(sanitized.selected_text(), Some("code=EXAMPLE"));
    assert_eq!(
        sanitized.buttons()[0].text.as_deref(),
        Some("release-notes-2026-june")
    );
    assert_eq!(
        sanitized.inputs()[0].label.as_deref(),
        Some("promo_code=SAVE20")
    );
    assert_eq!(
        sanitized.inputs()[0].placeholder.as_deref(),
        Some("person@example.test")
    );
    assert_eq!(review.masked_secret_texts(), 0);
    assert_eq!(review.masked_input_values(), 0);
}
