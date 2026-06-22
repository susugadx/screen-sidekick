#![forbid(unsafe_code)]

mod config;
mod sidecar;
mod wsl;

use std::{env, fmt};

use config::{
    load_native_host_config_from_environment, NativeHostPlatform, RuntimeSelection,
    RuntimeSelectionError,
};
use screen_sidekick_sidekick_daemon::{DaemonState, ProtocolConnection};
use screen_sidekick_sidekick_protocol::{
    ErrorCode, ErrorData, JsonRpcFailure, JsonRpcRequest, ProtocolError, JSONRPC_VERSION,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub use sidecar::run_sidecar_host;
pub(crate) use sidecar::validate_sidecar_ws_url;

pub const NATIVE_HOST_NAME: &str = "com.screen_sidekick.host";
pub const SCREEN_SIDEKICK_DAEMON_WS_URL_ENV: &str = "SCREEN_SIDEKICK_DAEMON_WS_URL";
pub const SCREEN_SIDEKICK_DAEMON_TOKEN_ENV: &str = "SCREEN_SIDEKICK_DAEMON_TOKEN";
pub const SCREEN_SIDEKICK_NATIVE_HOST_CONFIG_ENV: &str =
    config::SCREEN_SIDEKICK_NATIVE_HOST_CONFIG_ENV;
pub const NATIVE_HOST_CONFIG_SCHEMA_VERSION: &str = config::NATIVE_HOST_CONFIG_SCHEMA_VERSION;
pub const MAX_NATIVE_INCOMING_MESSAGE_BYTES: usize =
    screen_sidekick_sidekick_daemon::MAX_WS_MESSAGE_BYTES;
pub const MAX_NATIVE_OUTGOING_MESSAGE_BYTES: usize = 1024 * 1024;
const SETUP_REQUIRED_MESSAGE: &str = "Screen Sidekick Windows native host setup is required.";
const SETUP_REQUIRED_USER_ACTION: &str =
    "Install the Windows WSL native-host config or set the sidecar env variables.";

#[cfg(test)]
static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeFrameError {
    PartialLength,
    PayloadTooLarge { size: usize, max_size: usize },
    PartialPayload,
    InvalidUtf8,
    Io,
}

impl NativeFrameError {
    fn protocol_error_code(&self) -> ErrorCode {
        match self {
            Self::PayloadTooLarge { .. } => ErrorCode::PayloadTooLarge,
            Self::PartialLength | Self::PartialPayload | Self::InvalidUtf8 | Self::Io => {
                ErrorCode::InvalidRequest
            }
        }
    }

    fn protocol_error_message(&self) -> &'static str {
        match self {
            Self::PayloadTooLarge { .. } => "Native Messaging frame exceeds the Sidekick limit.",
            Self::InvalidUtf8 => "Native Messaging frame is not valid UTF-8.",
            Self::PartialLength | Self::PartialPayload | Self::Io => {
                "Native Messaging frame is invalid."
            }
        }
    }

    fn closes_connection(&self) -> bool {
        matches!(
            self,
            Self::PartialLength | Self::PartialPayload | Self::PayloadTooLarge { .. } | Self::Io
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeWriteError {
    PayloadTooLarge { size: usize, max_size: usize },
    LengthOverflow,
    Io,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeHostError {
    FrameRead(NativeFrameError),
    FrameWrite(NativeWriteError),
    Config,
    SetupRequired,
    SidecarUrl,
    SidecarConnect,
    SidecarProtocol,
    WslStart,
    WslStatus,
    RuntimeStart,
    TurnCleanup,
}

impl fmt::Display for NativeHostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::FrameRead(_) => "native host received an invalid frame",
            Self::FrameWrite(_) => "native host failed to write a protocol frame",
            Self::Config => "native host configuration is invalid",
            Self::SetupRequired => "native host setup is required",
            Self::SidecarUrl => "native host sidecar URL is invalid",
            Self::SidecarConnect => {
                "native host failed to connect to the configured daemon sidecar"
            }
            Self::SidecarProtocol => "native host sidecar protocol failed",
            Self::WslStart => "native host failed to start the WSL Sidekick daemon",
            Self::WslStatus => "native host received an invalid WSL daemon status",
            Self::RuntimeStart => "native host failed to start Sidekick runtime",
            Self::TurnCleanup => "native host failed to clean up owned active turns",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for NativeHostError {}

pub async fn read_native_message<R: AsyncRead + Unpin>(
    reader: &mut R,
    max_size: usize,
) -> Result<Option<String>, NativeFrameError> {
    let mut length_bytes = [0_u8; 4];
    let mut read = 0_usize;
    while read < length_bytes.len() {
        match reader.read(&mut length_bytes[read..]).await {
            Ok(0) if read == 0 => return Ok(None),
            Ok(0) => return Err(NativeFrameError::PartialLength),
            Ok(count) => read += count,
            Err(_) => return Err(NativeFrameError::Io),
        }
    }

    let size = u32::from_ne_bytes(length_bytes) as usize;
    if size > max_size {
        return Err(NativeFrameError::PayloadTooLarge { size, max_size });
    }

    let mut payload = vec![0_u8; size];
    if reader.read_exact(&mut payload).await.is_err() {
        return Err(NativeFrameError::PartialPayload);
    }
    String::from_utf8(payload)
        .map(Some)
        .map_err(|_| NativeFrameError::InvalidUtf8)
}

pub async fn write_native_message<W: AsyncWrite + Unpin>(
    writer: &mut W,
    text: &str,
    max_size: usize,
) -> Result<(), NativeWriteError> {
    let payload = text.as_bytes();
    if payload.len() >= max_size {
        return Err(NativeWriteError::PayloadTooLarge {
            size: payload.len(),
            max_size,
        });
    }
    let length = u32::try_from(payload.len()).map_err(|_| NativeWriteError::LengthOverflow)?;
    writer
        .write_all(&length.to_ne_bytes())
        .await
        .map_err(|_| NativeWriteError::Io)?;
    writer
        .write_all(payload)
        .await
        .map_err(|_| NativeWriteError::Io)?;
    writer.flush().await.map_err(|_| NativeWriteError::Io)
}

pub async fn run_in_process_host<R, W>(
    mut reader: R,
    mut writer: W,
    state: DaemonState,
    caller_origin: Option<String>,
) -> Result<(), NativeHostError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut connection = ProtocolConnection::native_host(state, caller_origin);
    let mut events = connection.event_receiver();

    let run_result = loop {
        tokio::select! {
            frame = read_native_message(&mut reader, MAX_NATIVE_INCOMING_MESSAGE_BYTES) => {
                match frame {
                    Ok(Some(text)) => {
                        if let Some(response) = connection.handle_text(&text).await {
                            if let Err(error) = write_native_message(&mut writer, &response, MAX_NATIVE_OUTGOING_MESSAGE_BYTES)
                                .await
                                .map_err(NativeHostError::FrameWrite)
                            {
                                break Err(error);
                            }
                        }
                    }
                    Ok(None) => break Ok(()),
                    Err(error) => {
                        if let Err(write_error) = write_frame_error(&mut writer, &error).await {
                            break Err(write_error);
                        }
                        if error.closes_connection() {
                            break Err(NativeHostError::FrameRead(error));
                        }
                    }
                }
            }
            notification = events.recv() => {
                match notification {
                    Ok(notification) => {
                        if let Some(text) = connection.notification_text(&notification) {
                            if let Err(error) = write_native_message(&mut writer, &text, MAX_NATIVE_OUTGOING_MESSAGE_BYTES)
                                .await
                                .map_err(NativeHostError::FrameWrite)
                            {
                                break Err(error);
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_))
                    | Err(tokio::sync::broadcast::error::RecvError::Closed) => break Ok(()),
                }
            }
        }
    };

    if connection.fail_owned_active_turns_on_disconnect().is_err() {
        return Err(NativeHostError::TurnCleanup);
    }
    run_result
}

pub async fn run_from_environment<R, W>(
    reader: R,
    writer: W,
    caller_origin: Option<String>,
) -> Result<(), NativeHostError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    run_from_environment_on_platform(reader, writer, caller_origin, NativeHostPlatform::current())
        .await
}

async fn run_from_environment_on_platform<R, W>(
    reader: R,
    writer: W,
    caller_origin: Option<String>,
    platform: NativeHostPlatform,
) -> Result<(), NativeHostError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let sidecar_url = env::var(SCREEN_SIDEKICK_DAEMON_WS_URL_ENV).ok();
    let sidecar_token = env::var(SCREEN_SIDEKICK_DAEMON_TOKEN_ENV).ok();
    let config = if sidecar_url.is_some() && sidecar_token.is_some() {
        None
    } else {
        match load_native_host_config_from_environment(platform) {
            Ok(config) => config,
            Err(_) if platform == NativeHostPlatform::Windows => {
                return run_setup_required_host(reader, writer).await;
            }
            Err(_) => return Err(NativeHostError::Config),
        }
    };
    let selection = match config::select_runtime(sidecar_url, sidecar_token, platform, config) {
        Ok(selection) => selection,
        Err(RuntimeSelectionError::WindowsConfigRequired) => {
            return run_setup_required_host(reader, writer).await;
        }
    };
    match selection {
        RuntimeSelection::Sidecar { ws_url, token } => {
            run_sidecar_host(reader, writer, &ws_url, &token, caller_origin).await
        }
        RuntimeSelection::WslAuto(config) => {
            run_wsl_auto_host(reader, writer, &config, caller_origin).await
        }
        RuntimeSelection::InProcess => {
            let state =
                DaemonState::default_runtime_state().map_err(|_| NativeHostError::RuntimeStart)?;
            run_in_process_host(reader, writer, state, caller_origin).await
        }
    }
}

async fn run_wsl_auto_host<R, W>(
    reader: R,
    writer: W,
    config: &config::WslAutoStartConfig,
    caller_origin: Option<String>,
) -> Result<(), NativeHostError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut daemon = match wsl::start_wsl_daemon(config).await {
        Ok(daemon) => daemon,
        Err(_) => return run_setup_required_host(reader, writer).await,
    };
    let result = sidecar::run_wsl_auto_sidecar_host(
        reader,
        writer,
        &daemon.status.ws_url,
        &daemon.status.token,
        caller_origin,
    )
    .await;
    if result.is_ok() {
        daemon.shutdown().await?;
    } else {
        let _ = daemon.shutdown().await;
    }
    result
}

async fn run_setup_required_host<R, W>(mut reader: R, mut writer: W) -> Result<(), NativeHostError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    write_setup_required_response_for_next_request(&mut reader, &mut writer).await
}

pub(crate) async fn write_setup_required_response_for_next_request<R, W>(
    reader: &mut R,
    writer: &mut W,
) -> Result<(), NativeHostError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    match read_native_message(reader, MAX_NATIVE_INCOMING_MESSAGE_BYTES).await {
        Ok(Some(text)) => write_setup_required_response_for_request_text(writer, &text).await,
        Ok(None) => Ok(()),
        Err(error) => {
            write_frame_error(writer, &error).await?;
            if error.closes_connection() {
                Err(NativeHostError::FrameRead(error))
            } else {
                Ok(())
            }
        }
    }
}

pub(crate) async fn write_setup_required_response_for_request_text<W>(
    writer: &mut W,
    text: &str,
) -> Result<(), NativeHostError>
where
    W: AsyncWrite + Unpin,
{
    let response = setup_required_response(text);
    write_native_message(writer, &response, MAX_NATIVE_OUTGOING_MESSAGE_BYTES)
        .await
        .map_err(NativeHostError::FrameWrite)
}

pub fn caller_origin_from_args<I>(args: I) -> Option<String>
where
    I: IntoIterator<Item = String>,
{
    args.into_iter()
        .skip(1)
        .find(|arg| arg.starts_with("chrome-extension://"))
}

pub(crate) async fn write_frame_error<W: AsyncWrite + Unpin>(
    writer: &mut W,
    error: &NativeFrameError,
) -> Result<(), NativeHostError> {
    let response = frame_error_response(error);
    write_native_message(writer, &response, MAX_NATIVE_OUTGOING_MESSAGE_BYTES)
        .await
        .map_err(NativeHostError::FrameWrite)
}

fn frame_error_response(error: &NativeFrameError) -> String {
    let data = match error {
        NativeFrameError::PayloadTooLarge { max_size, .. } => Some(ErrorData {
            max_size_bytes: Some(*max_size),
            retryable: Some(false),
            ..ErrorData::default()
        }),
        _ => Some(ErrorData {
            retryable: Some(false),
            ..ErrorData::default()
        }),
    };
    failure_response(
        "unknown",
        ProtocolError {
            code: error.protocol_error_code(),
            message: error.protocol_error_message().to_owned(),
            data: data.map(Box::new),
        },
    )
}

fn setup_required_response(text: &str) -> String {
    match serde_json::from_str::<JsonRpcRequest>(text) {
        Ok(request) => failure_response(request.id, setup_required_error()),
        Err(_) => failure_response(
            "unknown",
            ProtocolError {
                code: ErrorCode::InvalidRequest,
                message: "Request JSON is invalid.".to_owned(),
                data: None,
            },
        ),
    }
}

fn setup_required_error() -> ProtocolError {
    ProtocolError {
        code: ErrorCode::SetupRequired,
        message: SETUP_REQUIRED_MESSAGE.to_owned(),
        data: Some(Box::new(ErrorData {
            retryable: Some(false),
            user_action: Some(SETUP_REQUIRED_USER_ACTION.to_owned()),
            ..ErrorData::default()
        })),
    }
}

fn failure_response(id: impl Into<String>, error: ProtocolError) -> String {
    serde_json::to_string(&JsonRpcFailure::new(id, error)).unwrap_or_else(|_| {
        format!(
            "{{\"jsonrpc\":\"{}\",\"id\":\"unknown\",\"error\":{{\"code\":\"internal_error\",\"message\":\"internal error\"}}}}",
            JSONRPC_VERSION
        )
    })
}

#[cfg(test)]
mod tests;
