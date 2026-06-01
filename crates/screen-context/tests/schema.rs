use screen_sidekick_screen_context::{
    RawButton, RawPageMetadata, RawScreenContext, SCREEN_CONTEXT_SCHEMA_VERSION,
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
