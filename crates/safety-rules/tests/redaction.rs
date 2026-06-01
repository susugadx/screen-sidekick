use screen_sidekick_safety_rules::{
    mask_secret_like_text, redact_secret_bearing_url, REDACTED_URL_VALUE,
};
use screen_sidekick_screen_context::MASKED_VALUE;

#[test]
fn masks_secret_like_text_and_card_like_digits() {
    for raw_text in [
        "token=sk-abc123",
        "Authorization: Bearer abc123",
        "Card 4111 1111 1111 1111",
        "card4111111111111",
        "card=4111111111111",
    ] {
        let result = mask_secret_like_text(raw_text);

        assert!(result.was_masked);
        assert_eq!(result.text, MASKED_VALUE);
    }
}

#[test]
fn masks_secret_key_assignments_with_normalized_keys() {
    for raw_text in [
        "token=theme",
        "token=featureflagrollout2026",
        "secret=not-a-secret",
        "secret=not-a-secret-value-for-docs",
        "api-key=LIVESECRET",
        "api key=LIVESECRET",
        "api_key: LIVESECRET",
        "api.key: LIVESECRET",
        "apikey: LIVESECRET",
        "x-api-key=LIVESECRET",
        "access token=LIVESECRET",
        "client-secret: LIVESECRET",
        "accessToken=LIVESECRET",
        "refreshToken=LIVESECRET",
        "idToken: LIVESECRET",
        "clientSecret: LIVESECRET",
    ] {
        let result = mask_secret_like_text(raw_text);

        assert!(result.was_masked, "text was not masked: {raw_text}");
        assert_eq!(result.text, MASKED_VALUE);
    }
}

#[test]
fn masks_secret_label_value_sequences() {
    for raw_text in [
        "password mypass123",
        "password swordfish",
        "passcode sesame",
        "enter password swordfish",
        "api key LIVEVALUE",
        "api key livevalue",
        "client secret ABCD1234",
        "client secret swordfish",
        "verification code 123456",
        "access token abc-123",
        "token swordfish",
    ] {
        let result = mask_secret_like_text(raw_text);

        assert!(result.was_masked, "text was not masked: {raw_text}");
        assert_eq!(result.text, MASKED_VALUE);
    }
}

#[test]
fn masks_encoded_nested_secret_text() {
    for raw_text in [
        "redirect=https%3A%2F%2Fidp.test%2Fcallback%3Faccess_token%3DTEXTSECRET",
        "https%3A%2F%2Fidp.test%2Fcallback%3Faccess_token%3DTEXTSECRET",
        "redirect=https%253A%252F%252Fidp.test%252Fcallback%253Faccess_token%253DTEXTSECRET",
    ] {
        let result = mask_secret_like_text(raw_text);

        assert!(result.was_masked, "text was not masked: {raw_text}");
        assert_eq!(result.text, MASKED_VALUE);
    }
}

#[test]
fn masks_code_assignments_only_when_values_look_secret() {
    for raw_text in ["code=123456", "code=AUTHCODE1234567890abcdef"] {
        let result = mask_secret_like_text(raw_text);

        assert!(result.was_masked, "text was not masked: {raw_text}");
        assert_eq!(result.text, MASKED_VALUE);
    }
}

#[test]
fn masks_value_only_secret_like_text() {
    for raw_text in [
        "abc123def456ghi789jkl012",
        "abcdef1234567890abcdef12",
        "deadbeefdeadbeefdeadbeef",
        "aB39_xYz-92QpLmN8vT6rS0uK",
        "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjMifQ.signature",
        "123456",
    ] {
        let result = mask_secret_like_text(raw_text);

        assert!(result.was_masked, "text was not masked: {raw_text}");
        assert_eq!(result.text, MASKED_VALUE);
    }
}

#[test]
fn preserves_non_secret_text() {
    for raw_text in [
        "Search invoices",
        "redirect=https%3A%2F%2Fdocs.example.test%2Fpage%3Fsection%3Dintro",
        "code=EXAMPLE",
        "code=SAVE20",
        "status_code=200",
        "country_code=JP",
        "promo_code=SAVE20",
        "550e8400-e29b-41d4-a716-446655440000",
        "release-notes-2026-june",
        "featureflagrollout2026",
        "themePreferenceDarkMode",
        "VeryLongHumanReadableIdentifier2026",
        "not-a-secret-value-for-docs",
        "code=FEATUREFLAGROLLOUT2026",
        "code=release-notes-2026-june",
        "Verification code settings",
        "Password settings",
        "Password policy",
        "Password manager",
        "API key required",
        "API key documentation",
        "Reset password",
        "Reset password now",
        "Enter password",
        "Client secret settings",
        "Token bucket",
        "Token usage",
    ] {
        let result = mask_secret_like_text(raw_text);

        assert!(!result.was_masked, "text was masked: {raw_text}");
        assert_eq!(result.text, raw_text);
    }
}

#[test]
fn redacts_url_userinfo() {
    let result = redact_secret_bearing_url("https://user:secret@example.test/path");

    assert!(result.was_masked);
    assert_eq!(result.text, "https://example.test/path");
}

#[test]
fn redacts_secret_bearing_url_keys() {
    for (raw_url, expected) in [
        (
            "https://example.test/callback?token=sk-secret&state=keep",
            "https://example.test/callback?token=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/callback?token=theme&state=keep",
            "https://example.test/callback?token=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/callback?token=featureflagrollout2026&state=keep",
            "https://example.test/callback?token=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/callback?secret=not-a-secret-value-for-docs&state=keep",
            "https://example.test/callback?secret=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/callback?access_token=secret&state=keep",
            "https://example.test/callback?access_token=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/callback?api_key=secret&state=keep",
            "https://example.test/callback?api_key=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/callback?api-key=secret&state=keep",
            "https://example.test/callback?api-key=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/callback?api+key=secret&state=keep",
            "https://example.test/callback?api+key=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/callback?access-token=secret&state=keep",
            "https://example.test/callback?access-token=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/callback?client+secret=secret&state=keep",
            "https://example.test/callback?client+secret=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/callback?accessToken=secret&state=keep",
            "https://example.test/callback?accessToken=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/callback?refreshToken=secret&state=keep",
            "https://example.test/callback?refreshToken=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/callback?idToken=secret&state=keep",
            "https://example.test/callback?idToken=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/callback?clientSecret=secret&state=keep",
            "https://example.test/callback?clientSecret=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/callback?code=123456&state=keep",
            "https://example.test/callback?code=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/callback?code=EXAMPLE&state=keep",
            "https://example.test/callback?code=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/callback?code=AUTHCODE&state=keep",
            "https://example.test/callback?code=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/callback?code=AUTHCODE1234567890abcdef&state=keep",
            "https://example.test/callback?code=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/callback?token=abc123def456ghi789jkl012&state=keep",
            "https://example.test/callback?token=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/callback?token=abcdef1234567890abcdef12&state=keep",
            "https://example.test/callback?token=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/callback?token=deadbeefdeadbeefdeadbeef&state=keep",
            "https://example.test/callback?token=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/callback?token=aB39_xYz-92QpLmN8vT6rS0uK&state=keep",
            "https://example.test/callback?token=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/callback#token=sk-secret",
            "https://example.test/callback#token=[REDACTED]",
        ),
        (
            "https://example.test/callback#code=EXAMPLE",
            "https://example.test/callback#code=[REDACTED]",
        ),
    ] {
        let result = redact_secret_bearing_url(raw_url);

        assert!(result.was_masked);
        assert_eq!(result.text, expected);
    }
}

#[test]
fn redacts_bare_query_and_fragment_secret_values() {
    for (raw_url, expected) in [
        (
            "https://example.test/callback?sk-LIVESECRET",
            "https://example.test/callback?[REDACTED]",
        ),
        (
            "https://example.test/callback#sk-LIVESECRET",
            "https://example.test/callback#[REDACTED]",
        ),
    ] {
        let result = redact_secret_bearing_url(raw_url);

        assert!(result.was_masked);
        assert_eq!(result.text, expected);
    }
}

#[test]
fn redacts_secret_like_query_and_fragment_parameter_names() {
    for (raw_url, expected) in [
        (
            "https://example.test/callback?sk-LIVESECRET=1&state=keep",
            "https://example.test/callback?[REDACTED]=1&state=keep",
        ),
        (
            "https://example.test/callback?abc123def456ghi789jkl012=keep&state=keep",
            "https://example.test/callback?[REDACTED]=keep&state=keep",
        ),
        (
            "https://example.test/callback#sk-FRAGSECRET=1&state=keep",
            "https://example.test/callback#[REDACTED]=1&state=keep",
        ),
    ] {
        let result = redact_secret_bearing_url(raw_url);

        assert!(result.was_masked);
        assert_eq!(result.text, expected);
        assert!(!result.text.contains("LIVESECRET"));
        assert!(!result.text.contains("FRAGSECRET"));
        assert!(!result.text.contains("abc123def456ghi789jkl012"));
    }
}

#[test]
fn redacts_secret_like_url_path_segments() {
    for (raw_url, expected) in [
        (
            "https://example.test/reset/sk-LIVESECRET?state=keep",
            "https://example.test/reset/[REDACTED]?state=keep",
        ),
        (
            "https://example.test/invite/abc123def456ghi789jkl012?state=keep",
            "https://example.test/invite/[REDACTED]?state=keep",
        ),
        (
            "https://example.test/redirect/https%3A%2F%2Fidp.test%2Fcallback%3Faccess_token%3DPATHSECRET?state=keep",
            "https://example.test/redirect/[REDACTED]?state=keep",
        ),
    ] {
        let result = redact_secret_bearing_url(raw_url);

        assert!(result.was_masked);
        assert_eq!(result.text, expected);
        assert!(!result.text.contains("LIVESECRET"));
        assert!(!result.text.contains("PATHSECRET"));
    }
}

#[test]
fn redacts_secret_like_values_with_benign_keys() {
    for (raw_url, expected) in [
        (
            "https://example.test/search?q=sk-LIVESECRET&sort=name",
            "https://example.test/search?q=[REDACTED]&sort=name",
        ),
        (
            "https://example.test/search?q=abc123def456ghi789jkl012&sort=name",
            "https://example.test/search?q=[REDACTED]&sort=name",
        ),
        (
            "https://example.test/search?card_ref=card4111111111111&sort=name",
            "https://example.test/search?card_ref=[REDACTED]&sort=name",
        ),
    ] {
        let result = redact_secret_bearing_url(raw_url);

        assert!(result.was_masked);
        assert_eq!(result.text, expected);
    }
}

#[test]
fn redacts_nested_url_values_with_secret_keys() {
    for (raw_url, expected) in [
        (
            "https://example.test/search?redirect=https%3A%2F%2Fidp.test%2Fcallback%3Faccess_token%3Dsecret&state=keep",
            "https://example.test/search?redirect=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/search?redirect=https%3A%2F%2Fidp.test%2Fcallback%3Fcode%3DAUTHCODE1234567890abcdef&state=keep",
            "https://example.test/search?redirect=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/search?redirect=https%3A%2F%2Fidp.test%2Fcallback%3Fcode%3DEXAMPLE&state=keep",
            "https://example.test/search?redirect=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/search?redirect=https%3A%2F%2Fidp.test%2Fcallback%3Ftoken%3Dabc123def456ghi789jkl012&state=keep",
            "https://example.test/search?redirect=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/search?redirect=https%253A%252F%252Fidp.test%252Fcallback%253Faccess_token%253DDOUBLEENCODEDSECRET&state=keep",
            "https://example.test/search?redirect=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/search?redirect=https%253A%252F%252Fidp.test%252Fcallback%253Ftoken%253Dabc123def456ghi789jkl012&state=keep",
            "https://example.test/search?redirect=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/search?redirect=https%253A%252F%252Fidp.test%252Fcallback%253Fcode%253DEXAMPLE&state=keep",
            "https://example.test/search?redirect=[REDACTED]&state=keep",
        ),
        (
            "https://example.test/search#redirect=https%253A%252F%252Fidp.test%252Fcallback%253Faccess_token%253DDOUBLEENCODEDSECRET&state=keep",
            "https://example.test/search#redirect=[REDACTED]&state=keep",
        ),
    ] {
        let result = redact_secret_bearing_url(raw_url);

        assert!(result.was_masked);
        assert_eq!(result.text, expected);
        assert!(!result.text.contains("DOUBLEENCODEDSECRET"));
    }
}

#[test]
fn preserves_benign_url_keys_and_readable_values() {
    for raw_url in [
        "https://example.test/callback?status_code=200&state=keep",
        "https://example.test/callback?country_code=JP&state=keep",
        "https://example.test/callback?promo_code=SAVE20&state=keep",
        "https://example.test/callback?id=550e8400-e29b-41d4-a716-446655440000&state=keep",
        "https://example.test/callback?readable=release-notes-2026-june&state=keep",
        "https://example.test/callback?readable=featureflagrollout2026&state=keep",
        "https://example.test/callback?readable=themePreferenceDarkMode&state=keep",
        "https://example.test/callback?readable=VeryLongHumanReadableIdentifier2026&state=keep",
        "https://example.test/callback?long_code=release-notes-2026-june&state=keep",
        "https://example.test/#/callback/code=EXAMPLE?state=keep",
    ] {
        let result = redact_secret_bearing_url(raw_url);

        assert!(!result.was_masked, "url was masked: {raw_url}");
        assert_eq!(result.text, raw_url);
    }
}

#[test]
fn redacts_hash_router_route_secret_values_before_query() {
    for (raw_url, expected) in [
        (
            "https://example.test/#/reset/sk-LIVESECRET?state=keep",
            "https://example.test/#[REDACTED]?state=keep",
        ),
        (
            "https://example.test/#/callback/access_token=FRAGSECRET?state=keep",
            "https://example.test/#[REDACTED]?state=keep",
        ),
        (
            "https://example.test/#/callback/clientSecret=FRAGSECRET?state=keep",
            "https://example.test/#[REDACTED]?state=keep",
        ),
    ] {
        let result = redact_secret_bearing_url(raw_url);

        assert!(result.was_masked);
        assert_eq!(result.text, expected);
        assert!(!result.text.contains("LIVESECRET"));
        assert!(!result.text.contains("FRAGSECRET"));
    }
}

#[test]
fn redacts_hash_router_route_secret_assignments_without_query() {
    for (raw_url, expected) in [
        (
            "https://example.test/#/callback/access_token=FRAGSECRET",
            "https://example.test/#[REDACTED]",
        ),
        (
            "https://example.test/#/callback/clientSecret=FRAGSECRET",
            "https://example.test/#[REDACTED]",
        ),
    ] {
        let result = redact_secret_bearing_url(raw_url);

        assert!(result.was_masked);
        assert_eq!(result.text, expected);
        assert!(!result.text.contains("FRAGSECRET"));
    }
}

#[test]
fn preserves_non_secret_url_parts() {
    for raw_url in [
        "https://example.test/search?q=release-notes&redirect=https%3A%2F%2Fdocs.example.test%2Fpage%3Fsection%3Dintro#view=summary",
        "https://example.test/docs/release-notes-2026-june?state=keep",
        "https://example.test/items/550e8400-e29b-41d4-a716-446655440000?state=keep",
    ] {
        let result = redact_secret_bearing_url(raw_url);

        assert!(!result.was_masked, "url was masked: {raw_url}");
        assert_eq!(result.text, raw_url);
    }
}

#[test]
fn preserves_repeatedly_encoded_non_secret_nested_url_values() {
    let raw_url = "https://example.test/search?redirect=https%253A%252F%252Fdocs.example.test%252Fpage%253Fsection%253Dintro&state=keep";
    let result = redact_secret_bearing_url(raw_url);

    assert!(!result.was_masked);
    assert_eq!(result.text, raw_url);
}

#[test]
fn exposes_redacted_url_value_constant() {
    assert_eq!(REDACTED_URL_VALUE, "[REDACTED]");
}
