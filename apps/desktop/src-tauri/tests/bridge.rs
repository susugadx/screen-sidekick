use axum::{
    body::{to_bytes, Body},
    http::{
        header::{
            ACCESS_CONTROL_ALLOW_ORIGIN, ACCESS_CONTROL_REQUEST_METHOD, AUTHORIZATION,
            CONTENT_TYPE, ORIGIN,
        },
        HeaderValue, Method, Request, StatusCode,
    },
};
use screen_sidekick_capture_pipeline::RAW_BROWSER_CONTEXT_SCHEMA_VERSION;
use screen_sidekick_desktop::bridge::{
    build_bridge_router, BridgeHttpState, MAX_CAPTURE_BODY_BYTES,
};
use serde_json::json;
use tower::ServiceExt;

const TOKEN: &str = "test-token";
const EXTENSION_ORIGIN: &str = "chrome-extension://abcdefghijklmnop";

#[tokio::test]
async fn rejects_missing_bearer_token() {
    let response = send_capture(None, Some(EXTENSION_ORIGIN), valid_body()).await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_allows_extension_origin(&response);
}

#[tokio::test]
async fn rejects_bad_bearer_token() {
    let response = send_capture(
        Some("Bearer wrong-token"),
        Some(EXTENSION_ORIGIN),
        valid_body(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_allows_extension_origin(&response);
}

#[tokio::test]
async fn rejects_non_extension_origin() {
    let response = send_capture(
        Some("Bearer test-token"),
        Some("https://example.test"),
        valid_body(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_no_allow_origin(&response);
}

#[tokio::test]
async fn rejects_missing_origin() {
    let response = send_capture(Some("Bearer test-token"), None, valid_body()).await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_no_allow_origin(&response);
}

#[tokio::test]
async fn accepts_extension_preflight() {
    let app = build_bridge_router(BridgeHttpState::new(TOKEN));
    let request = Request::builder()
        .method(Method::OPTIONS)
        .uri("/v0/capture")
        .header(ORIGIN, EXTENSION_ORIGIN)
        .header(ACCESS_CONTROL_REQUEST_METHOD, "POST")
        .body(Body::empty())
        .expect("request is valid");

    let response = app.oneshot(request).await.expect("router responds");

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_allows_extension_origin(&response);
}

#[tokio::test]
async fn rejects_invalid_preflight_method_with_cors_for_extension_origin() {
    let app = build_bridge_router(BridgeHttpState::new(TOKEN));
    let request = Request::builder()
        .method(Method::OPTIONS)
        .uri("/v0/capture")
        .header(ORIGIN, EXTENSION_ORIGIN)
        .header(ACCESS_CONTROL_REQUEST_METHOD, "GET")
        .body(Body::empty())
        .expect("request is valid");

    let response = app.oneshot(request).await.expect("router responds");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_allows_extension_origin(&response);
}

#[tokio::test]
async fn returns_valid_capture_response_without_raw_secret_values() {
    let body = json!({
        "schema_version": RAW_BROWSER_CONTEXT_SCHEMA_VERSION,
        "page": {
            "url": "https://example.test/reset/sk-PATHSECRET?access_token=URLSECRET",
            "title": "api_key=TITLESECRET"
        },
        "selected_text": "password swordfish",
        "screenshot": {
            "format": "api_key=SCREENSHOTFORMATSECRET",
            "width": 640,
            "height": 480,
            "captured_at": "password screenshotsecret"
        },
        "buttons": [{
            "text": "Delete user",
            "visible": true
        }],
        "inputs": [{
            "kind": "email",
            "label": "token=INPUTSECRET",
            "visible": true
        }]
    })
    .to_string();

    let response = send_capture(Some("Bearer test-token"), Some(EXTENSION_ORIGIN), body).await;
    let status = response.status();
    assert_allows_extension_origin(&response);
    let bytes = to_bytes(response.into_body(), MAX_CAPTURE_BODY_BYTES)
        .await
        .expect("body is readable");
    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("response JSON parses");

    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["schema_version"], json!("capture_response.v0.1"));
    let screen_context_json = value["screen_context_json"]
        .as_str()
        .expect("screen_context_json is a string");
    let screen_context: serde_json::Value =
        serde_json::from_str(screen_context_json).expect("screen_context_json parses");
    assert!(screen_context_json.contains("reset/[REDACTED]"));
    assert_eq!(screen_context["screenshot"]["width"], json!(640));
    assert_eq!(screen_context["screenshot"]["height"], json!(480));
    assert!(screen_context["screenshot"].get("format").is_none());
    assert!(screen_context["screenshot"].get("captured_at").is_none());
    assert!(value["prompt_text"]
        .as_str()
        .expect("prompt_text is a string")
        .contains("destructive action"));

    let response_text = String::from_utf8(bytes.to_vec()).expect("response is UTF-8");
    for raw_secret in [
        "PATHSECRET",
        "URLSECRET",
        "TITLESECRET",
        "swordfish",
        "SCREENSHOTFORMATSECRET",
        "screenshotsecret",
        "INPUTSECRET",
    ] {
        assert!(
            !response_text.contains(raw_secret),
            "bridge response leaked raw secret: {raw_secret}"
        );
    }
}

#[tokio::test]
async fn rejects_oversized_capture_body() {
    let oversized_body = format!(
        "{{\"schema_version\":\"{}\",\"selected_text\":\"{}\"}}",
        RAW_BROWSER_CONTEXT_SCHEMA_VERSION,
        "x".repeat(MAX_CAPTURE_BODY_BYTES)
    );

    let response = send_capture(
        Some("Bearer test-token"),
        Some(EXTENSION_ORIGIN),
        oversized_body,
    )
    .await;

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_allows_extension_origin(&response);
}

#[tokio::test]
async fn rejects_invalid_capture_json_with_cors_for_extension_origin() {
    let response = send_capture(
        Some("Bearer test-token"),
        Some(EXTENSION_ORIGIN),
        "{not-json".to_owned(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_allows_extension_origin(&response);
}

#[tokio::test]
async fn rejects_unsupported_schema_with_cors_for_extension_origin() {
    let body = json!({
        "schema_version": "raw_browser_context.v999",
        "page": {
            "url": "https://example.test/admin",
            "title": "Users"
        }
    })
    .to_string();

    let response = send_capture(Some("Bearer test-token"), Some(EXTENSION_ORIGIN), body).await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_allows_extension_origin(&response);
}

async fn send_capture(
    authorization: Option<&str>,
    origin: Option<&str>,
    body: String,
) -> axum::http::Response<Body> {
    let app = build_bridge_router(BridgeHttpState::new(TOKEN));
    let mut request = Request::builder()
        .method(Method::POST)
        .uri("/v0/capture")
        .header(CONTENT_TYPE, "application/json");

    if let Some(authorization) = authorization {
        request = request.header(AUTHORIZATION, authorization);
    }

    if let Some(origin) = origin {
        request = request.header(ORIGIN, origin);
    }

    app.oneshot(request.body(Body::from(body)).expect("request is valid"))
        .await
        .expect("router responds")
}

fn valid_body() -> String {
    json!({
        "schema_version": RAW_BROWSER_CONTEXT_SCHEMA_VERSION,
        "page": {
            "url": "https://example.test/admin",
            "title": "Users"
        }
    })
    .to_string()
}

fn assert_allows_extension_origin(response: &axum::http::Response<Body>) {
    assert_eq!(
        response.headers().get(ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&HeaderValue::from_static(EXTENSION_ORIGIN))
    );
}

fn assert_no_allow_origin(response: &axum::http::Response<Body>) {
    assert_eq!(response.headers().get(ACCESS_CONTROL_ALLOW_ORIGIN), None);
}
