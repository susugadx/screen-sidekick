use screen_sidekick_screen_context::{
    RawButton, RawPageMetadata, RawScreenContext, ScreenshotFormat, SCREEN_CONTEXT_SCHEMA_VERSION,
};
use serde_json::json;

#[test]
fn serializes_v0_1_with_optional_fields_omitted() {
    let mut context = RawScreenContext::new();
    context.page = Some(RawPageMetadata {
        url: Some("https://example.test/admin".to_owned()),
        title: Some("Admin".to_owned()),
    });
    context.buttons = Some(vec![RawButton {
        text: Some("Save".to_owned()),
        visible: Some(true),
        ..RawButton::default()
    }]);

    let value = serde_json::to_value(&context).expect("screen context serializes");

    assert_eq!(value["schema_version"], json!("0.1"));
    assert_eq!(value["page"]["title"], json!("Admin"));
    assert!(value.get("selected_text").is_none());
    assert!(value.get("screenshot").is_none());
}

#[test]
fn deserializes_missing_optional_fields_and_ignores_unknown_fields() {
    let json = r#"{
        "schema_version": "0.1",
        "future_field": {"ignored": true}
    }"#;

    let context: RawScreenContext = serde_json::from_str(json).expect("unknown fields are ignored");

    assert_eq!(context.schema_version, SCREEN_CONTEXT_SCHEMA_VERSION);
    assert!(context.page.is_none());
    assert!(context.buttons().is_empty());
    assert!(context.inputs().is_empty());
}

#[test]
fn deserializes_valid_screenshot_metadata() {
    let json = r#"{
        "schema_version": "0.1",
        "screenshot": {
            "format": "png",
            "width": 1280,
            "height": 720,
            "captured_at": "2026-06-01T12:34:56.789Z"
        }
    }"#;

    let context: RawScreenContext =
        serde_json::from_str(json).expect("valid screenshot metadata loads");
    let screenshot = context.screenshot.expect("screenshot metadata is kept");

    assert_eq!(screenshot.format, Some(ScreenshotFormat::Png));
    assert_eq!(screenshot.width, Some(1280));
    assert_eq!(screenshot.height, Some(720));
    assert_eq!(
        screenshot.captured_at.as_ref().map(|value| value.as_str()),
        Some("2026-06-01T12:34:56.789Z")
    );
}

#[test]
fn drops_invalid_screenshot_string_metadata_without_rejecting_raw_context() {
    let json = r#"{
        "schema_version": "0.1",
        "screenshot": {
            "format": "api_key=live-secret",
            "width": 1280,
            "height": 720,
            "captured_at": "password swordfish"
        },
        "buttons": [
            {"text": "Save", "visible": true}
        ]
    }"#;

    let context: RawScreenContext =
        serde_json::from_str(json).expect("invalid screenshot strings are dropped");
    let screenshot = context
        .screenshot
        .as_ref()
        .expect("numeric screenshot metadata is kept");

    assert_eq!(screenshot.format, None);
    assert_eq!(screenshot.captured_at, None);
    assert_eq!(screenshot.width, Some(1280));
    assert_eq!(screenshot.height, Some(720));
    assert_eq!(context.buttons()[0].text.as_deref(), Some("Save"));
}

#[test]
fn drops_unknown_screenshot_format_without_rejecting_raw_context() {
    let json = r#"{
        "schema_version": "0.1",
        "screenshot": {
            "format": "PNG",
            "captured_at": "2026-06-01T12:34:56.789Z"
        }
    }"#;

    let context: RawScreenContext =
        serde_json::from_str(json).expect("unknown screenshot format is ignored");
    let screenshot = context
        .screenshot
        .expect("screenshot metadata object is kept");

    assert_eq!(screenshot.format, None);
    assert_eq!(
        screenshot.captured_at.as_ref().map(|value| value.as_str()),
        Some("2026-06-01T12:34:56.789Z")
    );
}
