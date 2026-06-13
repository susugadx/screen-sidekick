use axum::{
    body::Body,
    extract::{rejection::JsonRejection, Json, State},
    http::{
        header::{
            ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS,
            ACCESS_CONTROL_ALLOW_ORIGIN, ACCESS_CONTROL_MAX_AGE, ACCESS_CONTROL_REQUEST_METHOD,
            AUTHORIZATION, ORIGIN, VARY,
        },
        HeaderMap, HeaderValue, Method, Response, StatusCode,
    },
    response::IntoResponse,
};
use screen_sidekick_capture_pipeline::{process_capture, CapturePipelineError, RawBrowserContext};
use url::Url;

use crate::DaemonState;

pub(crate) fn validate_extension_origin_for_optional_header(
    headers: &HeaderMap,
) -> Result<(), DaemonRejection> {
    match headers.get(ORIGIN) {
        Some(origin) => {
            let origin_text = origin
                .to_str()
                .map_err(|_| DaemonRejection::InvalidOrigin)?;
            if is_chrome_extension_origin(origin_text) {
                Ok(())
            } else {
                Err(DaemonRejection::InvalidOrigin)
            }
        }
        None => Ok(()),
    }
}

pub(crate) async fn legacy_preflight(
    State(_state): State<DaemonState>,
    headers: HeaderMap,
) -> Result<Response<Body>, DaemonRejection> {
    let origin = validated_extension_origin(&headers)?.clone();
    let response = match validate_preflight_method(&headers) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(rejection) => rejection.into_response(),
    };
    Ok(with_cors_headers(response, &origin))
}

pub(crate) async fn legacy_capture(
    State(state): State<DaemonState>,
    headers: HeaderMap,
    body: Result<Json<RawBrowserContext>, JsonRejection>,
) -> Result<Response<Body>, DaemonRejection> {
    let origin = validated_extension_origin(&headers)?.clone();
    let response = match capture_with_valid_origin(&state, &headers, body) {
        Ok(response) => response,
        Err(rejection) => rejection.into_response(),
    };
    Ok(with_cors_headers(response, &origin))
}

fn capture_with_valid_origin(
    state: &DaemonState,
    headers: &HeaderMap,
    body: Result<Json<RawBrowserContext>, JsonRejection>,
) -> Result<Response<Body>, DaemonRejection> {
    validate_authorization(headers, state.token())?;
    let Json(request) = body.map_err(DaemonRejection::from_json_rejection)?;
    let capture_response =
        process_capture(request).map_err(DaemonRejection::from_pipeline_error)?;
    Ok((StatusCode::OK, Json(capture_response)).into_response())
}

fn with_cors_headers(mut response: Response<Body>, origin: &HeaderValue) -> Response<Body> {
    apply_cors_headers(response.headers_mut(), origin);
    response
}

fn validated_extension_origin(headers: &HeaderMap) -> Result<&HeaderValue, DaemonRejection> {
    let origin = headers.get(ORIGIN).ok_or(DaemonRejection::MissingOrigin)?;
    let origin_text = origin
        .to_str()
        .map_err(|_| DaemonRejection::InvalidOrigin)?;
    if is_chrome_extension_origin(origin_text) {
        Ok(origin)
    } else {
        Err(DaemonRejection::InvalidOrigin)
    }
}

fn is_chrome_extension_origin(origin: &str) -> bool {
    let Ok(parsed) = Url::parse(origin) else {
        return false;
    };
    parsed.scheme() == "chrome-extension"
        && parsed.host_str().is_some_and(|host| !host.is_empty())
        && matches!(parsed.path(), "" | "/")
        && parsed.query().is_none()
        && parsed.fragment().is_none()
}

fn validate_preflight_method(headers: &HeaderMap) -> Result<(), DaemonRejection> {
    let method = headers
        .get(ACCESS_CONTROL_REQUEST_METHOD)
        .and_then(|value| value.to_str().ok());

    if method == Some(Method::POST.as_str()) {
        Ok(())
    } else {
        Err(DaemonRejection::InvalidPreflight)
    }
}

fn validate_authorization(
    headers: &HeaderMap,
    expected_token: &str,
) -> Result<(), DaemonRejection> {
    let authorization = headers
        .get(AUTHORIZATION)
        .ok_or(DaemonRejection::MissingAuthorization)?;
    let authorization = authorization
        .to_str()
        .map_err(|_| DaemonRejection::InvalidAuthorization)?;
    match authorization.strip_prefix("Bearer ") {
        Some(token) if token == expected_token => Ok(()),
        _ => Err(DaemonRejection::InvalidAuthorization),
    }
}

fn apply_cors_headers(headers: &mut HeaderMap, origin: &HeaderValue) {
    headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
    headers.insert(
        ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("POST, OPTIONS"),
    );
    headers.insert(
        ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("authorization, content-type"),
    );
    headers.insert(ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("600"));
    headers.insert(VARY, HeaderValue::from_static("Origin"));
}

#[derive(Debug)]
pub(crate) enum DaemonRejection {
    MissingOrigin,
    InvalidOrigin,
    InvalidPreflight,
    MissingAuthorization,
    InvalidAuthorization,
    InvalidJson(StatusCode),
    InvalidCapture,
    SerializationFailure,
}

impl DaemonRejection {
    fn from_json_rejection(rejection: JsonRejection) -> Self {
        Self::InvalidJson(rejection.status())
    }

    fn from_pipeline_error(error: CapturePipelineError) -> Self {
        match error {
            CapturePipelineError::UnsupportedSchemaVersion => Self::InvalidCapture,
            CapturePipelineError::SerializeSanitizedContext(_) => Self::SerializationFailure,
        }
    }
}

impl IntoResponse for DaemonRejection {
    fn into_response(self) -> Response<Body> {
        match self {
            Self::MissingOrigin => {
                (StatusCode::FORBIDDEN, "extension Origin header is required").into_response()
            }
            Self::InvalidOrigin => (
                StatusCode::FORBIDDEN,
                "only chrome-extension origins are allowed",
            )
                .into_response(),
            Self::InvalidPreflight => {
                (StatusCode::FORBIDDEN, "invalid CORS preflight").into_response()
            }
            Self::MissingAuthorization => {
                (StatusCode::UNAUTHORIZED, "bearer token is required").into_response()
            }
            Self::InvalidAuthorization => {
                (StatusCode::FORBIDDEN, "bearer token is invalid").into_response()
            }
            Self::InvalidJson(status) => {
                (status, "capture request JSON is invalid").into_response()
            }
            Self::InvalidCapture => {
                (StatusCode::BAD_REQUEST, "capture request is invalid").into_response()
            }
            Self::SerializationFailure => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to serialize capture response",
            )
                .into_response(),
        }
    }
}
