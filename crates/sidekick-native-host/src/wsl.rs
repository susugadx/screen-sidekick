use std::{process::Stdio, time::Duration};

use serde_json::Value;
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, BufReader},
    process::{Child, ChildStdin, Command},
    time::timeout,
};

use crate::{
    config::{build_wsl_daemon_command, WslAutoStartConfig},
    validate_sidecar_ws_url, NativeHostError,
};

const WSL_DAEMON_STATUS_TIMEOUT: Duration = Duration::from_secs(15);
const WSL_DAEMON_STATUS_MAX_BYTES: usize = 8 * 1024;
const WSL_DAEMON_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) struct WslDaemonProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    pub(crate) status: WslDaemonStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WslDaemonStatus {
    pub(crate) ws_url: String,
    pub(crate) token: String,
}

pub(crate) async fn start_wsl_daemon(
    config: &WslAutoStartConfig,
) -> Result<WslDaemonProcess, NativeHostError> {
    let command = build_wsl_daemon_command(config);
    let mut child = Command::new(&command.program)
        .args(&command.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| NativeHostError::WslStart)?;
    let stdin = child.stdin.take();
    let stdout = child.stdout.take().ok_or(NativeHostError::WslStart)?;
    let mut reader = BufReader::new(stdout);
    let line = timeout(WSL_DAEMON_STATUS_TIMEOUT, read_status_line(&mut reader))
        .await
        .map_err(|_| NativeHostError::WslStatus)?
        .map_err(|_| NativeHostError::WslStatus)?;
    let status = parse_daemon_status_line(&line)?;
    Ok(WslDaemonProcess {
        child,
        stdin,
        status,
    })
}

impl WslDaemonProcess {
    pub(crate) async fn shutdown(&mut self) -> Result<(), NativeHostError> {
        drop(self.stdin.take());
        match timeout(WSL_DAEMON_SHUTDOWN_TIMEOUT, self.child.wait()).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(_)) => Err(NativeHostError::WslStart),
            Err(_) => {
                self.child
                    .kill()
                    .await
                    .map_err(|_| NativeHostError::WslStart)?;
                Ok(())
            }
        }
    }
}

async fn read_status_line<R>(reader: &mut R) -> Result<String, NativeHostError>
where
    R: AsyncBufRead + Unpin,
{
    let mut line = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .await
            .map_err(|_| NativeHostError::WslStatus)?;
        if available.is_empty() {
            return Err(NativeHostError::WslStatus);
        }

        let chunk_len = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |index| index + 1);
        if line.len() + chunk_len > WSL_DAEMON_STATUS_MAX_BYTES {
            return Err(NativeHostError::WslStatus);
        }
        line.extend_from_slice(&available[..chunk_len]);
        reader.consume(chunk_len);
        if line.last() == Some(&b'\n') {
            return String::from_utf8(line).map_err(|_| NativeHostError::WslStatus);
        }
    }
}

pub(crate) fn parse_daemon_status_line(line: &str) -> Result<WslDaemonStatus, NativeHostError> {
    let value: Value =
        serde_json::from_str(line.trim_end()).map_err(|_| NativeHostError::WslStatus)?;
    if value.get("schema_version").and_then(Value::as_str)
        != Some(screen_sidekick_sidekick_daemon::DAEMON_STATUS_SCHEMA_VERSION)
    {
        return Err(NativeHostError::WslStatus);
    }
    let ws_url = required_string(&value, "ws_url")?;
    validate_sidecar_ws_url(ws_url)?;
    let token = required_string(&value, "token")?;
    if token.chars().any(char::is_control) {
        return Err(NativeHostError::WslStatus);
    }
    Ok(WslDaemonStatus {
        ws_url: ws_url.to_owned(),
        token: token.to_owned(),
    })
}

fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, NativeHostError> {
    let text = value
        .get(key)
        .and_then(Value::as_str)
        .ok_or(NativeHostError::WslStatus)?;
    if text.is_empty() {
        return Err(NativeHostError::WslStatus);
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn status_line_reader_accepts_bounded_newline_terminated_output() {
        let line = json!({
            "schema_version": screen_sidekick_sidekick_daemon::DAEMON_STATUS_SCHEMA_VERSION,
            "url": "http://127.0.0.1:43001",
            "ws_url": "ws://127.0.0.1:43001/v0/ws",
            "token": "pairing-token",
            "status": "running"
        })
        .to_string()
            + "\n";
        let mut reader = BufReader::new(line.as_bytes());

        let read = read_status_line(&mut reader)
            .await
            .expect("bounded status line reads");

        assert_eq!(read, line);
    }

    #[tokio::test]
    async fn status_line_reader_rejects_oversized_output_without_newline() {
        let input = vec![b'x'; WSL_DAEMON_STATUS_MAX_BYTES + 1];
        let mut reader = BufReader::new(input.as_slice());

        let error = read_status_line(&mut reader)
            .await
            .expect_err("oversized output is rejected");

        assert_eq!(error, NativeHostError::WslStatus);
    }

    #[tokio::test]
    async fn status_line_reader_rejects_oversized_output_before_newline() {
        let mut input = vec![b'x'; WSL_DAEMON_STATUS_MAX_BYTES];
        input.push(b'\n');
        let mut reader = BufReader::new(input.as_slice());

        let error = read_status_line(&mut reader)
            .await
            .expect_err("oversized line is rejected before parsing");

        assert_eq!(error, NativeHostError::WslStatus);
    }

    #[test]
    fn fake_wsl_daemon_status_output_is_parsed() {
        let line = json!({
            "schema_version": screen_sidekick_sidekick_daemon::DAEMON_STATUS_SCHEMA_VERSION,
            "url": "http://127.0.0.1:43001",
            "ws_url": "ws://127.0.0.1:43001/v0/ws",
            "token": "pairing-token",
            "status": "running"
        })
        .to_string();

        let status = parse_daemon_status_line(&line).expect("status parses");

        assert_eq!(status.ws_url, "ws://127.0.0.1:43001/v0/ws");
        assert_eq!(status.token, "pairing-token");
    }

    #[test]
    fn fake_wsl_daemon_status_rejects_non_loopback_or_decorated_urls() {
        for ws_url in [
            "ws://localhost:43001/v0/ws",
            "http://127.0.0.1:43001/v0/ws",
            "ws://127.0.0.1:43001/v0/ws?token=SECRET",
            "ws://127.0.0.1:43001/v0/ws#fragment",
        ] {
            let line = json!({
                "schema_version": screen_sidekick_sidekick_daemon::DAEMON_STATUS_SCHEMA_VERSION,
                "url": "http://127.0.0.1:43001",
                "ws_url": ws_url,
                "token": "pairing-token",
                "status": "running"
            })
            .to_string();

            assert!(parse_daemon_status_line(&line).is_err(), "{ws_url}");
        }
    }
}
