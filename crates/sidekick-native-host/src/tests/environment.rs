use super::{read_value, send_request};
use crate::{
    run_from_environment, run_from_environment_on_platform, NativeHostError, NativeHostPlatform,
    ENV_LOCK, NATIVE_HOST_CONFIG_SCHEMA_VERSION, SCREEN_SIDEKICK_DAEMON_TOKEN_ENV,
    SCREEN_SIDEKICK_DAEMON_WS_URL_ENV, SCREEN_SIDEKICK_NATIVE_HOST_CONFIG_ENV,
    SETUP_REQUIRED_MESSAGE, SETUP_REQUIRED_USER_ACTION,
};
use screen_sidekick_session::{BeginTurn, SessionStore};
use screen_sidekick_sidekick_protocol::{method, TurnStatus, SIDEKICK_PROTOCOL_VERSION};
use serde_json::{json, Value};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use tokio::io::duplex;

#[tokio::test]
async fn default_in_process_host_does_not_recover_shared_active_turns() {
    let _guard = ENV_LOCK.lock().await;
    let temp = tempfile::tempdir().expect("temp dir is created");
    let _xdg_data_home = EnvVarGuard::set("XDG_DATA_HOME", temp.path());
    let _sidecar_url = EnvVarGuard::unset(SCREEN_SIDEKICK_DAEMON_WS_URL_ENV);
    let _sidecar_token = EnvVarGuard::unset(SCREEN_SIDEKICK_DAEMON_TOKEN_ENV);
    let _native_config = EnvVarGuard::unset(SCREEN_SIDEKICK_NATIVE_HOST_CONFIG_ENV);
    let data_dir = temp.path().join("screen-sidekick");
    std::fs::create_dir_all(&data_dir).expect("data dir is created");
    let database_path = data_dir.join("screen-sidekick.sqlite3");
    let store = SessionStore::open(&database_path).expect("store opens");
    let session = store
        .create_session(Some("Live native turn"))
        .expect("session created");
    let turn = store
        .begin_turn(BeginTurn {
            session_id: session.id.clone(),
            user_text: "still streaming".to_owned(),
            attachment_ids: Vec::new(),
            idempotency_key: "live-native-turn".to_owned(),
            request_hash: "live-native-hash".to_owned(),
        })
        .expect("turn begins");
    store
        .mark_turn_running(
            &turn.turn_id,
            Some("live_codex_thread"),
            Some("live_codex_turn"),
        )
        .expect("turn is running");
    drop(store);

    let (input_writer, input_reader) = duplex(64);
    let (output_writer, _output_reader) = duplex(64);
    drop(input_writer);

    run_from_environment(
        input_reader,
        output_writer,
        Some("chrome-extension://abcdefghijklmnop/".to_owned()),
    )
    .await
    .expect("native host exits after stdin closes");

    let store = SessionStore::open(&database_path).expect("store reopens");
    let stored_turn = store.get_turn(&turn.turn_id).expect("turn still exists");
    let stored_session = store
        .get_session(&session.id)
        .expect("session still exists");

    assert_eq!(stored_turn.status, TurnStatus::Running);
    assert_eq!(
        stored_session.session.active_turn_id.as_deref(),
        Some(turn.turn_id.as_str())
    );
    assert!(stored_session.active_turn.is_some());
}

#[tokio::test]
async fn windows_missing_config_reports_setup_required_protocol_error() {
    let _guard = ENV_LOCK.lock().await;
    let temp = tempfile::tempdir().expect("temp dir is created");
    let _appdata = EnvVarGuard::set("APPDATA", temp.path());
    let _sidecar_url = EnvVarGuard::unset(SCREEN_SIDEKICK_DAEMON_WS_URL_ENV);
    let _sidecar_token = EnvVarGuard::unset(SCREEN_SIDEKICK_DAEMON_TOKEN_ENV);
    let _native_config = EnvVarGuard::unset(SCREEN_SIDEKICK_NATIVE_HOST_CONFIG_ENV);
    let (mut input_writer, input_reader) = duplex(4096);
    let (output_writer, mut output_reader) = duplex(4096);
    let run = tokio::spawn(run_from_environment_on_platform(
        input_reader,
        output_writer,
        Some("chrome-extension://abcdefghijklmnop/".to_owned()),
        NativeHostPlatform::Windows,
    ));

    send_request(
        &mut input_writer,
        "init",
        method::INITIALIZE,
        json!({
            "client_kind": "chrome_extension",
            "client_version": "test",
            "protocol_version": SIDEKICK_PROTOCOL_VERSION,
            "capabilities": ["browser_context", "chat_stream"]
        }),
    )
    .await;
    let response = read_value(&mut output_reader).await;

    assert_setup_required_response(&response, "init");
    run.await
        .expect("host task joins")
        .expect("host exits after setup-required response");
}

#[tokio::test]
async fn windows_invalid_config_reports_setup_required_protocol_error() {
    let _guard = ENV_LOCK.lock().await;
    let temp = tempfile::tempdir().expect("temp dir is created");
    let invalid_config = temp.path().join("native-host-config.json");
    std::fs::write(&invalid_config, "{not-json").expect("invalid config is written");
    let _appdata = EnvVarGuard::set("APPDATA", temp.path());
    let _sidecar_url = EnvVarGuard::unset(SCREEN_SIDEKICK_DAEMON_WS_URL_ENV);
    let _sidecar_token = EnvVarGuard::unset(SCREEN_SIDEKICK_DAEMON_TOKEN_ENV);
    let _native_config = EnvVarGuard::set(SCREEN_SIDEKICK_NATIVE_HOST_CONFIG_ENV, &invalid_config);
    let (mut input_writer, input_reader) = duplex(4096);
    let (output_writer, mut output_reader) = duplex(4096);
    let run = tokio::spawn(run_from_environment_on_platform(
        input_reader,
        output_writer,
        Some("chrome-extension://abcdefghijklmnop/".to_owned()),
        NativeHostPlatform::Windows,
    ));

    send_request(
        &mut input_writer,
        "init-invalid-config",
        method::INITIALIZE,
        json!({
            "client_kind": "chrome_extension",
            "client_version": "test",
            "protocol_version": SIDEKICK_PROTOCOL_VERSION,
            "capabilities": ["browser_context", "chat_stream"]
        }),
    )
    .await;
    let response = read_value(&mut output_reader).await;

    assert_setup_required_response(&response, "init-invalid-config");
    run.await
        .expect("host task joins")
        .expect("host exits after setup-required response");
}

#[tokio::test]
async fn windows_wsl_start_failure_reports_setup_required_protocol_error() {
    let _guard = ENV_LOCK.lock().await;
    let temp = tempfile::tempdir().expect("temp dir is created");
    let config_path = temp.path().join("native-host-config.json");
    std::fs::write(&config_path, valid_wsl_config()).expect("valid config is written");
    let empty_path = temp.path().join("empty-path");
    std::fs::create_dir_all(&empty_path).expect("empty PATH dir is created");
    let _path = EnvVarGuard::set("PATH", &empty_path);
    let _appdata = EnvVarGuard::set("APPDATA", temp.path());
    let _sidecar_url = EnvVarGuard::unset(SCREEN_SIDEKICK_DAEMON_WS_URL_ENV);
    let _sidecar_token = EnvVarGuard::unset(SCREEN_SIDEKICK_DAEMON_TOKEN_ENV);
    let _native_config = EnvVarGuard::set(SCREEN_SIDEKICK_NATIVE_HOST_CONFIG_ENV, &config_path);
    let (mut input_writer, input_reader) = duplex(4096);
    let (output_writer, mut output_reader) = duplex(4096);
    let run = tokio::spawn(run_from_environment_on_platform(
        input_reader,
        output_writer,
        Some("chrome-extension://abcdefghijklmnop/".to_owned()),
        NativeHostPlatform::Windows,
    ));

    send_request(
        &mut input_writer,
        "init-wsl-start-failure",
        method::INITIALIZE,
        json!({
            "client_kind": "chrome_extension",
            "client_version": "test",
            "protocol_version": SIDEKICK_PROTOCOL_VERSION,
            "capabilities": ["browser_context", "chat_stream"]
        }),
    )
    .await;
    let response = read_value(&mut output_reader).await;

    assert_setup_required_response(&response, "init-wsl-start-failure");
    run.await
        .expect("host task joins")
        .expect("host exits after setup-required response");
}

#[cfg(unix)]
#[tokio::test]
async fn windows_wsl_oversized_status_reports_setup_required_protocol_error() {
    let _guard = ENV_LOCK.lock().await;
    let temp = tempfile::tempdir().expect("temp dir is created");
    let config_path = temp.path().join("native-host-config.json");
    std::fs::write(&config_path, valid_wsl_config()).expect("valid config is written");
    let fake_path = temp.path().join("fake-path");
    std::fs::create_dir_all(&fake_path).expect("fake PATH dir is created");
    let fake_wsl = fake_path.join("wsl.exe");
    std::fs::write(&fake_wsl, "#!/bin/sh\nprintf '%9000s' x\n").expect("fake wsl.exe is written");
    std::fs::set_permissions(&fake_wsl, std::fs::Permissions::from_mode(0o755))
        .expect("fake wsl.exe is executable");
    let _path = EnvVarGuard::set("PATH", &fake_path);
    let _appdata = EnvVarGuard::set("APPDATA", temp.path());
    let _sidecar_url = EnvVarGuard::unset(SCREEN_SIDEKICK_DAEMON_WS_URL_ENV);
    let _sidecar_token = EnvVarGuard::unset(SCREEN_SIDEKICK_DAEMON_TOKEN_ENV);
    let _native_config = EnvVarGuard::set(SCREEN_SIDEKICK_NATIVE_HOST_CONFIG_ENV, &config_path);
    let (mut input_writer, input_reader) = duplex(4096);
    let (output_writer, mut output_reader) = duplex(4096);
    let run = tokio::spawn(run_from_environment_on_platform(
        input_reader,
        output_writer,
        Some("chrome-extension://abcdefghijklmnop/".to_owned()),
        NativeHostPlatform::Windows,
    ));

    send_request(
        &mut input_writer,
        "init-wsl-oversized-status",
        method::INITIALIZE,
        json!({
            "client_kind": "chrome_extension",
            "client_version": "test",
            "protocol_version": SIDEKICK_PROTOCOL_VERSION,
            "capabilities": ["browser_context", "chat_stream"]
        }),
    )
    .await;
    let response = read_value(&mut output_reader).await;

    assert_setup_required_response(&response, "init-wsl-oversized-status");
    run.await
        .expect("host task joins")
        .expect("host exits after setup-required response");
}

#[tokio::test]
async fn sidecar_env_pair_skips_invalid_windows_config() {
    let _guard = ENV_LOCK.lock().await;
    let temp = tempfile::tempdir().expect("temp dir is created");
    let invalid_config = temp.path().join("native-host-config.json");
    std::fs::write(&invalid_config, "{not-json").expect("invalid config is written");
    let (input_writer, input_reader) = duplex(64);
    let (output_writer, _output_reader) = duplex(64);
    drop(input_writer);
    let _appdata = EnvVarGuard::set("APPDATA", temp.path());
    let _sidecar_url = EnvVarGuard::set(
        SCREEN_SIDEKICK_DAEMON_WS_URL_ENV,
        "ws://localhost:43001/v0/ws",
    );
    let _sidecar_token = EnvVarGuard::set(SCREEN_SIDEKICK_DAEMON_TOKEN_ENV, "pairing-token");
    let _native_config = EnvVarGuard::set(SCREEN_SIDEKICK_NATIVE_HOST_CONFIG_ENV, &invalid_config);

    let error = run_from_environment_on_platform(
        input_reader,
        output_writer,
        Some("chrome-extension://abcdefghijklmnop/".to_owned()),
        NativeHostPlatform::Windows,
    )
    .await
    .expect_err("sidecar env pair is selected before invalid Windows config");

    assert_eq!(error, NativeHostError::SidecarUrl);
}

#[tokio::test]
async fn non_windows_environment_ignores_invalid_native_host_config_env() {
    let _guard = ENV_LOCK.lock().await;
    let temp = tempfile::tempdir().expect("temp dir is created");
    let invalid_config = temp.path().join("native-host-config.json");
    std::fs::write(&invalid_config, "{not-json").expect("invalid config is written");
    let _xdg_data_home = EnvVarGuard::set("XDG_DATA_HOME", temp.path());
    let _sidecar_url = EnvVarGuard::unset(SCREEN_SIDEKICK_DAEMON_WS_URL_ENV);
    let _sidecar_token = EnvVarGuard::unset(SCREEN_SIDEKICK_DAEMON_TOKEN_ENV);
    let _native_config = EnvVarGuard::set(SCREEN_SIDEKICK_NATIVE_HOST_CONFIG_ENV, &invalid_config);
    let (input_writer, input_reader) = duplex(64);
    let (output_writer, _output_reader) = duplex(64);
    drop(input_writer);

    run_from_environment_on_platform(
        input_reader,
        output_writer,
        Some("chrome-extension://abcdefghijklmnop/".to_owned()),
        NativeHostPlatform::Other,
    )
    .await
    .expect("non-Windows host ignores Windows native-host config and exits after stdin closes");
}

fn assert_setup_required_response(response: &Value, id: &str) {
    assert_eq!(response["id"], json!(id));
    assert_eq!(response["error"]["code"], json!("setup_required"));
    assert_eq!(response["error"]["message"], json!(SETUP_REQUIRED_MESSAGE));
    assert_eq!(response["error"]["data"]["retryable"], json!(false));
    assert_eq!(
        response["error"]["data"]["user_action"],
        json!(SETUP_REQUIRED_USER_ACTION)
    );
}

fn valid_wsl_config() -> String {
    json!({
        "schema_version": NATIVE_HOST_CONFIG_SCHEMA_VERSION,
        "mode": "wsl_auto",
        "wsl_distro": "Ubuntu-24.04",
        "wsl_workdir": "/home/susu/screen-sidekick",
        "wsl_daemon_binary": "/home/susu/screen-sidekick/target/debug/screen-sidekick-daemon"
    })
    .to_string()
}

struct EnvVarGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, previous }
    }

    fn unset(key: &'static str) -> Self {
        let previous = std::env::var_os(key);
        std::env::remove_var(key);
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}
