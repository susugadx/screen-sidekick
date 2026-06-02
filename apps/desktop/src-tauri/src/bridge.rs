use std::{
    error::Error,
    fmt, io,
    net::{Ipv4Addr, TcpListener},
    sync::Mutex,
    thread::{self, JoinHandle},
};

use axum::{
    body::Body,
    extract::{rejection::JsonRejection, DefaultBodyLimit, Json, State},
    http::{
        header::{
            ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS,
            ACCESS_CONTROL_ALLOW_ORIGIN, ACCESS_CONTROL_MAX_AGE, ACCESS_CONTROL_REQUEST_METHOD,
            AUTHORIZATION, ORIGIN, VARY,
        },
        HeaderMap, HeaderValue, Method, Response, StatusCode,
    },
    response::IntoResponse,
    routing::post,
    Router,
};
use screen_sidekick_capture_pipeline::{process_capture, CapturePipelineError, RawBrowserContext};
use serde::Serialize;
use tokio::sync::oneshot;
use url::Url;
use uuid::Uuid;

pub const BRIDGE_STATUS_SCHEMA_VERSION: &str = "bridge_status.v0.1";
pub const MAX_CAPTURE_BODY_BYTES: usize = 128 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BridgeStatus {
    pub schema_version: String,
    pub url: String,
    pub token: String,
    pub status: BridgeRuntimeStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeRuntimeStatus {
    Running,
}

#[derive(Debug, Clone)]
pub struct BridgeHttpState {
    token: String,
}

impl BridgeHttpState {
    #[must_use]
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
        }
    }
}

pub struct BridgeRuntime {
    shutdown: Mutex<Option<oneshot::Sender<()>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl BridgeRuntime {
    pub fn start() -> Result<(Self, BridgeStatus), BridgeStartError> {
        let token = Uuid::new_v4().to_string();
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(BridgeStartError::Bind)?;
        listener
            .set_nonblocking(true)
            .map_err(BridgeStartError::SetNonblocking)?;
        let address = listener.local_addr().map_err(BridgeStartError::LocalAddr)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(BridgeStartError::Runtime)?;
        let router = build_bridge_router(BridgeHttpState::new(token.clone()));
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let thread = spawn_bridge_thread(runtime, listener, router, shutdown_receiver);
        let status = BridgeStatus {
            schema_version: BRIDGE_STATUS_SCHEMA_VERSION.to_owned(),
            url: format!("http://{address}"),
            token,
            status: BridgeRuntimeStatus::Running,
        };

        Ok((
            Self {
                shutdown: Mutex::new(Some(shutdown_sender)),
                thread: Mutex::new(Some(thread)),
            },
            status,
        ))
    }
}

impl Drop for BridgeRuntime {
    fn drop(&mut self) {
        if let Ok(mut shutdown) = self.shutdown.lock() {
            if let Some(sender) = shutdown.take() {
                let _ = sender.send(());
            }
        }

        if let Ok(mut thread) = self.thread.lock() {
            if let Some(handle) = thread.take() {
                let _ = handle.join();
            }
        }
    }
}

#[derive(Debug)]
pub enum BridgeStartError {
    Bind(io::Error),
    SetNonblocking(io::Error),
    LocalAddr(io::Error),
    Runtime(io::Error),
}

impl fmt::Display for BridgeStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bind(_) => formatter.write_str("failed to bind loopback bridge"),
            Self::SetNonblocking(_) => formatter.write_str("failed to prepare loopback listener"),
            Self::LocalAddr(_) => formatter.write_str("failed to read loopback bridge address"),
            Self::Runtime(_) => formatter.write_str("failed to start loopback bridge runtime"),
        }
    }
}

impl Error for BridgeStartError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Bind(error)
            | Self::SetNonblocking(error)
            | Self::LocalAddr(error)
            | Self::Runtime(error) => Some(error),
        }
    }
}

pub fn build_bridge_router(state: BridgeHttpState) -> Router {
    Router::new()
        .route("/v0/capture", post(capture).options(preflight))
        .with_state(state)
        .layer(DefaultBodyLimit::max(MAX_CAPTURE_BODY_BYTES))
}

fn spawn_bridge_thread(
    runtime: tokio::runtime::Runtime,
    listener: TcpListener,
    router: Router,
    shutdown_receiver: oneshot::Receiver<()>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        runtime.block_on(async move {
            let listener = match tokio::net::TcpListener::from_std(listener) {
                Ok(listener) => listener,
                Err(error) => {
                    eprintln!("Screen Sidekick bridge failed to accept loopback listener: {error}");
                    return;
                }
            };
            let server = axum::serve(listener, router).with_graceful_shutdown(async {
                let _ = shutdown_receiver.await;
            });
            if let Err(error) = server.await {
                eprintln!("Screen Sidekick bridge stopped: {error}");
            }
        });
    })
}

async fn preflight(
    State(_state): State<BridgeHttpState>,
    headers: HeaderMap,
) -> Result<Response<Body>, BridgeRejection> {
    let origin = validated_extension_origin(&headers)?.clone();
    let response = match validate_preflight_method(&headers) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(rejection) => rejection.into_response(),
    };
    Ok(with_cors_headers(response, &origin))
}

async fn capture(
    State(state): State<BridgeHttpState>,
    headers: HeaderMap,
    body: Result<Json<RawBrowserContext>, JsonRejection>,
) -> Result<Response<Body>, BridgeRejection> {
    let origin = validated_extension_origin(&headers)?.clone();
    let response = match capture_with_valid_origin(&state, &headers, body) {
        Ok(response) => response,
        Err(rejection) => rejection.into_response(),
    };
    Ok(with_cors_headers(response, &origin))
}

fn capture_with_valid_origin(
    state: &BridgeHttpState,
    headers: &HeaderMap,
    body: Result<Json<RawBrowserContext>, JsonRejection>,
) -> Result<Response<Body>, BridgeRejection> {
    validate_authorization(headers, &state.token)?;
    let Json(request) = body.map_err(BridgeRejection::from_json_rejection)?;
    let capture_response =
        process_capture(request).map_err(BridgeRejection::from_pipeline_error)?;
    Ok((StatusCode::OK, Json(capture_response)).into_response())
}

fn with_cors_headers(mut response: Response<Body>, origin: &HeaderValue) -> Response<Body> {
    apply_cors_headers(response.headers_mut(), origin);
    response
}

fn validated_extension_origin(headers: &HeaderMap) -> Result<&HeaderValue, BridgeRejection> {
    let origin = headers.get(ORIGIN).ok_or(BridgeRejection::MissingOrigin)?;
    let origin_text = origin
        .to_str()
        .map_err(|_| BridgeRejection::InvalidOrigin)?;
    if is_chrome_extension_origin(origin_text) {
        Ok(origin)
    } else {
        Err(BridgeRejection::InvalidOrigin)
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

fn validate_preflight_method(headers: &HeaderMap) -> Result<(), BridgeRejection> {
    let method = headers
        .get(ACCESS_CONTROL_REQUEST_METHOD)
        .and_then(|value| value.to_str().ok());

    if method == Some(Method::POST.as_str()) {
        Ok(())
    } else {
        Err(BridgeRejection::InvalidPreflight)
    }
}

fn validate_authorization(
    headers: &HeaderMap,
    expected_token: &str,
) -> Result<(), BridgeRejection> {
    let authorization = headers
        .get(AUTHORIZATION)
        .ok_or(BridgeRejection::MissingAuthorization)?;
    let authorization = authorization
        .to_str()
        .map_err(|_| BridgeRejection::InvalidAuthorization)?;
    match authorization.strip_prefix("Bearer ") {
        Some(token) if token == expected_token => Ok(()),
        _ => Err(BridgeRejection::InvalidAuthorization),
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
enum BridgeRejection {
    MissingOrigin,
    InvalidOrigin,
    InvalidPreflight,
    MissingAuthorization,
    InvalidAuthorization,
    InvalidJson(StatusCode),
    InvalidCapture,
    SerializationFailure,
}

impl BridgeRejection {
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

impl IntoResponse for BridgeRejection {
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
