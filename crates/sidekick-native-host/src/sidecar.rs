use futures_util::{SinkExt, StreamExt};
use screen_sidekick_sidekick_daemon::{
    SIDECAR_OWNED_WEBSOCKET_HEADER, SIDECAR_OWNED_WEBSOCKET_HEADER_VALUE,
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        client::IntoClientRequest, http::HeaderValue as WsHeaderValue, Message as WsMessage,
    },
};
use url::Url;

use crate::{
    read_native_message, write_frame_error, write_native_message,
    write_setup_required_response_for_next_request, write_setup_required_response_for_request_text,
    NativeHostError, MAX_NATIVE_INCOMING_MESSAGE_BYTES, MAX_NATIVE_OUTGOING_MESSAGE_BYTES,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidecarStartupFailurePolicy {
    ReturnError,
    SetupRequired,
}

pub async fn run_sidecar_host<R, W>(
    reader: R,
    writer: W,
    ws_url: &str,
    token: &str,
    caller_origin: Option<String>,
) -> Result<(), NativeHostError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    run_sidecar_host_with_startup_failure_policy(
        reader,
        writer,
        ws_url,
        token,
        caller_origin,
        SidecarStartupFailurePolicy::ReturnError,
    )
    .await
}

pub(crate) async fn run_wsl_auto_sidecar_host<R, W>(
    reader: R,
    writer: W,
    ws_url: &str,
    token: &str,
    caller_origin: Option<String>,
) -> Result<(), NativeHostError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    run_sidecar_host_with_startup_failure_policy(
        reader,
        writer,
        ws_url,
        token,
        caller_origin,
        SidecarStartupFailurePolicy::SetupRequired,
    )
    .await
}

async fn run_sidecar_host_with_startup_failure_policy<R, W>(
    mut reader: R,
    mut writer: W,
    ws_url: &str,
    token: &str,
    caller_origin: Option<String>,
    startup_failure_policy: SidecarStartupFailurePolicy,
) -> Result<(), NativeHostError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let url = validate_sidecar_ws_url(ws_url)?;
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|_| NativeHostError::SidecarUrl)?;
    if let Some(origin) = caller_origin.as_deref() {
        let header = WsHeaderValue::from_str(origin).map_err(|_| NativeHostError::SidecarUrl)?;
        request.headers_mut().insert("Origin", header);
    }
    request.headers_mut().insert(
        SIDECAR_OWNED_WEBSOCKET_HEADER,
        WsHeaderValue::from_static(SIDECAR_OWNED_WEBSOCKET_HEADER_VALUE),
    );
    let socket = match connect_async(request).await {
        Ok((socket, _)) => socket,
        Err(_) => match startup_failure_policy {
            SidecarStartupFailurePolicy::ReturnError => {
                return Err(NativeHostError::SidecarConnect)
            }
            SidecarStartupFailurePolicy::SetupRequired => {
                return write_setup_required_response_for_next_request(&mut reader, &mut writer)
                    .await;
            }
        },
    };
    run_connected_sidecar_host(reader, writer, socket, token, startup_failure_policy).await
}

async fn run_connected_sidecar_host<R, W>(
    mut reader: R,
    mut writer: W,
    socket: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    token: &str,
    startup_failure_policy: SidecarStartupFailurePolicy,
) -> Result<(), NativeHostError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let mut first_request_text: Option<String> = None;
    let mut wrote_first_sidecar_response = false;

    loop {
        tokio::select! {
            frame = read_native_message(&mut reader, MAX_NATIVE_INCOMING_MESSAGE_BYTES) => {
                match frame {
                    Ok(Some(text)) => {
                        if !wrote_first_sidecar_response && first_request_text.is_none() {
                            first_request_text = Some(text.clone());
                        }
                        let text = inject_sidecar_initialize_token(&text, token);
                        if ws_sender
                            .send(WsMessage::Text(text.into()))
                            .await
                            .is_err()
                        {
                            return handle_sidecar_startup_protocol_failure(
                                startup_failure_policy,
                                &mut reader,
                                &mut writer,
                                first_request_text.as_deref(),
                                wrote_first_sidecar_response,
                            )
                            .await;
                        }
                    }
                    Ok(None) => return Ok(()),
                    Err(error) => {
                        write_frame_error(&mut writer, &error).await?;
                        return Err(NativeHostError::FrameRead(error));
                    }
                }
            }
            message = ws_receiver.next() => {
                match message {
                    Some(Ok(WsMessage::Text(text))) => {
                        write_native_message(&mut writer, text.as_ref(), MAX_NATIVE_OUTGOING_MESSAGE_BYTES)
                            .await
                            .map_err(NativeHostError::FrameWrite)?;
                        wrote_first_sidecar_response = true;
                    }
                    Some(Ok(WsMessage::Ping(bytes))) => {
                        if ws_sender
                            .send(WsMessage::Pong(bytes))
                            .await
                            .is_err()
                        {
                            return handle_sidecar_startup_protocol_failure(
                                startup_failure_policy,
                                &mut reader,
                                &mut writer,
                                first_request_text.as_deref(),
                                wrote_first_sidecar_response,
                            )
                            .await;
                        }
                    }
                    Some(Ok(WsMessage::Close(_))) | None => {
                        if startup_failure_policy == SidecarStartupFailurePolicy::SetupRequired
                            && !wrote_first_sidecar_response
                        {
                            return write_setup_required_response_for_sidecar_startup_failure(
                                &mut reader,
                                &mut writer,
                                first_request_text.as_deref(),
                            )
                            .await;
                        }
                        return Ok(());
                    }
                    Some(Ok(WsMessage::Binary(_))) | Some(Ok(WsMessage::Pong(_))) => {}
                    Some(Ok(WsMessage::Frame(_))) => {}
                    Some(Err(_)) => {
                        return handle_sidecar_startup_protocol_failure(
                            startup_failure_policy,
                            &mut reader,
                            &mut writer,
                            first_request_text.as_deref(),
                            wrote_first_sidecar_response,
                        )
                        .await;
                    }
                }
            }
        }
    }
}

async fn handle_sidecar_startup_protocol_failure<R, W>(
    startup_failure_policy: SidecarStartupFailurePolicy,
    reader: &mut R,
    writer: &mut W,
    first_request_text: Option<&str>,
    wrote_first_sidecar_response: bool,
) -> Result<(), NativeHostError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    if startup_failure_policy == SidecarStartupFailurePolicy::SetupRequired
        && !wrote_first_sidecar_response
    {
        return write_setup_required_response_for_sidecar_startup_failure(
            reader,
            writer,
            first_request_text,
        )
        .await;
    }
    Err(NativeHostError::SidecarProtocol)
}

async fn write_setup_required_response_for_sidecar_startup_failure<R, W>(
    reader: &mut R,
    writer: &mut W,
    first_request_text: Option<&str>,
) -> Result<(), NativeHostError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    if let Some(text) = first_request_text {
        return write_setup_required_response_for_request_text(writer, text).await;
    }
    write_setup_required_response_for_next_request(reader, writer).await
}

pub(crate) fn validate_sidecar_ws_url(raw_url: &str) -> Result<Url, NativeHostError> {
    let url = Url::parse(raw_url).map_err(|_| NativeHostError::SidecarUrl)?;
    if url.scheme() != "ws"
        || url.host_str() != Some("127.0.0.1")
        || url.port().is_none()
        || url.path() != "/v0/ws"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(NativeHostError::SidecarUrl);
    }
    Ok(url)
}

fn inject_sidecar_initialize_token(text: &str, token: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(text) else {
        return text.to_owned();
    };
    if value.get("method").and_then(serde_json::Value::as_str) != Some("initialize") {
        return text.to_owned();
    }
    match value.get_mut("params") {
        Some(serde_json::Value::Object(params)) => {
            params.insert("auth_token".to_owned(), serde_json::json!(token));
        }
        Some(_) | None => {
            value["params"] = serde_json::json!({ "auth_token": token });
        }
    }
    serde_json::to_string(&value).unwrap_or_else(|_| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use screen_sidekick_sidekick_protocol::{method, JsonRpcRequest, SIDEKICK_PROTOCOL_VERSION};
    use serde_json::{json, Value};

    #[test]
    fn sidecar_url_accepts_only_explicit_loopback_websocket_endpoint() {
        assert!(validate_sidecar_ws_url("ws://127.0.0.1:43001/v0/ws").is_ok());
        assert!(validate_sidecar_ws_url("ws://localhost:43001/v0/ws").is_err());
        assert!(validate_sidecar_ws_url("http://127.0.0.1:43001/v0/ws").is_err());
        assert!(validate_sidecar_ws_url("ws://127.0.0.1:43001/v0/ws?token=SECRET").is_err());
        assert!(validate_sidecar_ws_url("ws://127.0.0.1:43001/other").is_err());
    }

    #[test]
    fn sidecar_initialize_injection_keeps_token_out_of_non_initialize_messages() {
        let request = JsonRpcRequest::new("status", method::STATUS_GET, json!({}));
        let text = serde_json::to_string(&request).expect("request serializes");

        assert_eq!(inject_sidecar_initialize_token(&text, "secret-token"), text);

        let init = JsonRpcRequest::new(
            "init",
            method::INITIALIZE,
            json!({ "protocol_version": SIDEKICK_PROTOCOL_VERSION }),
        );
        let injected = inject_sidecar_initialize_token(
            &serde_json::to_string(&init).expect("request serializes"),
            "secret-token",
        );
        let injected: Value = serde_json::from_str(&injected).expect("injected request is JSON");

        assert_eq!(injected["params"]["auth_token"], json!("secret-token"));
    }
}
