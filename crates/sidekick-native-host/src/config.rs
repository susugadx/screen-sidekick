use std::{
    env, fmt, fs, io,
    path::{Path, PathBuf},
};

use serde::Deserialize;

pub(crate) const NATIVE_HOST_CONFIG_SCHEMA_VERSION: &str =
    "screen_sidekick_native_host_config.v0.1";
pub(crate) const SCREEN_SIDEKICK_NATIVE_HOST_CONFIG_ENV: &str =
    "SCREEN_SIDEKICK_NATIVE_HOST_CONFIG";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeHostPlatform {
    Windows,
    Other,
}

impl NativeHostPlatform {
    pub(crate) fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Other
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeHostConfig {
    pub(crate) wsl: WslAutoStartConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WslAutoStartConfig {
    pub(crate) distro: String,
    pub(crate) workdir: String,
    pub(crate) daemon_binary: String,
    pub(crate) path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WslCommandSpec {
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RuntimeSelection {
    Sidecar { ws_url: String, token: String },
    WslAuto(WslAutoStartConfig),
    InProcess,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RuntimeSelectionError {
    WindowsConfigRequired,
}

#[derive(Debug)]
pub(crate) enum NativeHostConfigError {
    Read(io::Error),
    Parse(serde_json::Error),
    UnsupportedSchemaVersion,
    UnsupportedMode,
    MissingField(&'static str),
    InvalidDistro,
    InvalidLinuxPath(&'static str),
    MissingAppData,
}

impl fmt::Display for NativeHostConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Read(_) => "failed to read native host config",
            Self::Parse(_) => "failed to parse native host config",
            Self::UnsupportedSchemaVersion => "native host config schema version is unsupported",
            Self::UnsupportedMode => "native host config mode is unsupported",
            Self::MissingField(field) => {
                return write!(formatter, "native host config is missing {field}");
            }
            Self::InvalidDistro => "native host config contains an invalid WSL distro",
            Self::InvalidLinuxPath(field) => {
                return write!(formatter, "native host config contains an invalid {field}");
            }
            Self::MissingAppData => "APPDATA is not available for native host config lookup",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for NativeHostConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read(error) => Some(error),
            Self::Parse(error) => Some(error),
            Self::UnsupportedSchemaVersion
            | Self::UnsupportedMode
            | Self::MissingField(_)
            | Self::InvalidDistro
            | Self::InvalidLinuxPath(_)
            | Self::MissingAppData => None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeHostConfigFile {
    schema_version: String,
    mode: String,
    wsl_distro: Option<String>,
    wsl_workdir: Option<String>,
    wsl_daemon_binary: Option<String>,
    wsl_path: Option<String>,
}

pub(crate) fn parse_native_host_config(
    text: &str,
) -> Result<NativeHostConfig, NativeHostConfigError> {
    let file: NativeHostConfigFile =
        serde_json::from_str(text).map_err(NativeHostConfigError::Parse)?;
    if file.schema_version != NATIVE_HOST_CONFIG_SCHEMA_VERSION {
        return Err(NativeHostConfigError::UnsupportedSchemaVersion);
    }
    if file.mode != "wsl_auto" {
        return Err(NativeHostConfigError::UnsupportedMode);
    }
    let distro = required(file.wsl_distro, "wsl_distro")?;
    let workdir = required(file.wsl_workdir, "wsl_workdir")?;
    let daemon_binary = required(file.wsl_daemon_binary, "wsl_daemon_binary")?;
    validate_wsl_distro(&distro)?;
    validate_linux_path(&workdir, "wsl_workdir", true)?;
    validate_linux_path(&daemon_binary, "wsl_daemon_binary", false)?;
    if let Some(path) = file.wsl_path.as_deref() {
        validate_linux_path_list(path, "wsl_path")?;
    }
    Ok(NativeHostConfig {
        wsl: WslAutoStartConfig {
            distro,
            workdir,
            daemon_binary,
            path: file.wsl_path,
        },
    })
}

pub(crate) fn load_native_host_config_from_environment(
    platform: NativeHostPlatform,
) -> Result<Option<NativeHostConfig>, NativeHostConfigError> {
    let Some(path) = config_path_from_environment(platform)? else {
        return Ok(None);
    };
    if !path.exists() && env::var_os(SCREEN_SIDEKICK_NATIVE_HOST_CONFIG_ENV).is_none() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path).map_err(NativeHostConfigError::Read)?;
    parse_native_host_config(&text).map(Some)
}

pub(crate) fn config_path_from_environment(
    platform: NativeHostPlatform,
) -> Result<Option<PathBuf>, NativeHostConfigError> {
    if platform != NativeHostPlatform::Windows {
        return Ok(None);
    }
    if let Some(path) = env::var_os(SCREEN_SIDEKICK_NATIVE_HOST_CONFIG_ENV) {
        return Ok(Some(PathBuf::from(path)));
    }
    let appdata = env::var_os("APPDATA").ok_or(NativeHostConfigError::MissingAppData)?;
    Ok(Some(
        Path::new(&appdata)
            .join("Screen Sidekick")
            .join("native-host-config.json"),
    ))
}

pub(crate) fn select_runtime(
    sidecar_url: Option<String>,
    sidecar_token: Option<String>,
    platform: NativeHostPlatform,
    config: Option<NativeHostConfig>,
) -> Result<RuntimeSelection, RuntimeSelectionError> {
    if let (Some(ws_url), Some(token)) = (sidecar_url, sidecar_token) {
        return Ok(RuntimeSelection::Sidecar { ws_url, token });
    }
    if platform == NativeHostPlatform::Windows {
        let config = config.ok_or(RuntimeSelectionError::WindowsConfigRequired)?;
        return Ok(RuntimeSelection::WslAuto(config.wsl));
    }
    Ok(RuntimeSelection::InProcess)
}

pub(crate) fn build_wsl_daemon_command(config: &WslAutoStartConfig) -> WslCommandSpec {
    let mut args = vec![
        "-d".to_owned(),
        config.distro.clone(),
        "--cd".to_owned(),
        config.workdir.clone(),
        "--exec".to_owned(),
    ];
    if let Some(path) = &config.path {
        args.extend(["env".to_owned(), format!("PATH={path}")]);
    }
    args.extend([config.daemon_binary.clone(), "--stdio-status".to_owned()]);
    WslCommandSpec {
        program: "wsl.exe".to_owned(),
        args,
    }
}

fn required(value: Option<String>, field: &'static str) -> Result<String, NativeHostConfigError> {
    let value = value.ok_or(NativeHostConfigError::MissingField(field))?;
    if value.trim().is_empty() {
        return Err(NativeHostConfigError::MissingField(field));
    }
    Ok(value)
}

fn validate_wsl_distro(value: &str) -> Result<(), NativeHostConfigError> {
    if value.trim() != value
        || value.len() > 128
        || value
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '/' | '\\' | '"' | '\''))
    {
        return Err(NativeHostConfigError::InvalidDistro);
    }
    Ok(())
}

fn validate_linux_path(
    value: &str,
    field: &'static str,
    allow_root: bool,
) -> Result<(), NativeHostConfigError> {
    if value.trim() != value
        || !value.starts_with('/')
        || (!allow_root && value == "/")
        || value.contains('\\')
        || value.chars().any(char::is_control)
        || value.split('/').any(|component| component == "..")
    {
        return Err(NativeHostConfigError::InvalidLinuxPath(field));
    }
    Ok(())
}

fn validate_linux_path_list(value: &str, field: &'static str) -> Result<(), NativeHostConfigError> {
    if value.trim() != value || value.is_empty() || value.contains('\\') {
        return Err(NativeHostConfigError::InvalidLinuxPath(field));
    }
    for segment in value.split(':') {
        if segment.is_empty() {
            return Err(NativeHostConfigError::InvalidLinuxPath(field));
        }
        validate_linux_path(segment, field, false)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_config() -> String {
        json!({
            "schema_version": NATIVE_HOST_CONFIG_SCHEMA_VERSION,
            "mode": "wsl_auto",
            "wsl_distro": "Ubuntu-24.04",
            "wsl_workdir": "/home/susu/screen sidekick",
            "wsl_daemon_binary": "/home/susu/screen sidekick/target/debug/screen-sidekick-daemon"
        })
        .to_string()
    }

    fn valid_config_with_path() -> String {
        let mut value: serde_json::Value =
            serde_json::from_str(&valid_config()).expect("config json parses");
        value["wsl_path"] = json!("/home/susu/.nvm/versions/node/v22.20.0/bin:/home/susu/.cargo/bin:/usr/local/bin:/usr/bin:/bin");
        value.to_string()
    }

    #[test]
    fn config_parser_accepts_valid_wsl_auto_config() {
        let config = parse_native_host_config(&valid_config()).expect("config parses");

        assert_eq!(config.wsl.distro, "Ubuntu-24.04");
        assert_eq!(config.wsl.workdir, "/home/susu/screen sidekick");
        assert_eq!(
            config.wsl.daemon_binary,
            "/home/susu/screen sidekick/target/debug/screen-sidekick-daemon"
        );
        assert_eq!(config.wsl.path, None);
    }

    #[test]
    fn config_parser_accepts_optional_wsl_path() {
        let config = parse_native_host_config(&valid_config_with_path()).expect("config parses");

        assert_eq!(
            config.wsl.path.as_deref(),
            Some("/home/susu/.nvm/versions/node/v22.20.0/bin:/home/susu/.cargo/bin:/usr/local/bin:/usr/bin:/bin")
        );
    }

    #[test]
    fn config_parser_rejects_missing_required_wsl_fields() {
        let text = json!({
            "schema_version": NATIVE_HOST_CONFIG_SCHEMA_VERSION,
            "mode": "wsl_auto",
            "wsl_distro": "Ubuntu"
        })
        .to_string();

        assert!(matches!(
            parse_native_host_config(&text),
            Err(NativeHostConfigError::MissingField("wsl_workdir"))
        ));
    }

    #[test]
    fn config_parser_rejects_invalid_distro_and_paths() {
        let mut value: serde_json::Value =
            serde_json::from_str(&valid_config()).expect("config json parses");
        value["wsl_distro"] = json!("Ubuntu/evil");
        assert!(matches!(
            parse_native_host_config(&value.to_string()),
            Err(NativeHostConfigError::InvalidDistro)
        ));

        value = serde_json::from_str(&valid_config()).expect("config json parses");
        value["wsl_workdir"] = json!("relative/path");
        assert!(matches!(
            parse_native_host_config(&value.to_string()),
            Err(NativeHostConfigError::InvalidLinuxPath("wsl_workdir"))
        ));

        value = serde_json::from_str(&valid_config()).expect("config json parses");
        value["wsl_daemon_binary"] = json!("/home/susu/../screen-sidekick-daemon");
        assert!(matches!(
            parse_native_host_config(&value.to_string()),
            Err(NativeHostConfigError::InvalidLinuxPath("wsl_daemon_binary"))
        ));

        value = serde_json::from_str(&valid_config()).expect("config json parses");
        value["wsl_path"] = json!("/home/susu/.cargo/bin:relative");
        assert!(matches!(
            parse_native_host_config(&value.to_string()),
            Err(NativeHostConfigError::InvalidLinuxPath("wsl_path"))
        ));

        value = serde_json::from_str(&valid_config()).expect("config json parses");
        value["wsl_path"] = json!("/home/susu/.cargo/bin:");
        assert!(matches!(
            parse_native_host_config(&value.to_string()),
            Err(NativeHostConfigError::InvalidLinuxPath("wsl_path"))
        ));

        value = serde_json::from_str(&valid_config()).expect("config json parses");
        value["wsl_path"] = json!("/home/susu/../bin");
        assert!(matches!(
            parse_native_host_config(&value.to_string()),
            Err(NativeHostConfigError::InvalidLinuxPath("wsl_path"))
        ));
    }

    #[test]
    fn windows_runtime_selection_prefers_sidecar_env_before_wsl_config() {
        let config = parse_native_host_config(&valid_config()).expect("config parses");

        let selection = select_runtime(
            Some("ws://127.0.0.1:43001/v0/ws".to_owned()),
            Some("pairing-token".to_owned()),
            NativeHostPlatform::Windows,
            Some(config),
        )
        .expect("runtime selected");

        assert!(matches!(selection, RuntimeSelection::Sidecar { .. }));
    }

    #[tokio::test]
    async fn non_windows_config_loader_ignores_explicit_env_override() {
        let _guard = crate::ENV_LOCK.lock().await;
        let temp = tempfile::tempdir().expect("temp dir is created");
        let invalid_config = temp.path().join("native-host-config.json");
        std::fs::write(&invalid_config, "{not-json").expect("invalid config is written");
        let _native_config =
            EnvVarGuard::set(SCREEN_SIDEKICK_NATIVE_HOST_CONFIG_ENV, &invalid_config);

        let path =
            config_path_from_environment(NativeHostPlatform::Other).expect("config path resolves");
        let config = load_native_host_config_from_environment(NativeHostPlatform::Other)
            .expect("non-Windows config lookup succeeds");

        assert_eq!(path, None);
        assert_eq!(config, None);
    }

    #[tokio::test]
    async fn windows_config_loader_uses_explicit_env_override() {
        let _guard = crate::ENV_LOCK.lock().await;
        let temp = tempfile::tempdir().expect("temp dir is created");
        let config_path = temp.path().join("native-host-config.json");
        std::fs::write(&config_path, valid_config()).expect("config is written");
        let _native_config = EnvVarGuard::set(SCREEN_SIDEKICK_NATIVE_HOST_CONFIG_ENV, &config_path);
        let _appdata = EnvVarGuard::unset("APPDATA");

        let config = load_native_host_config_from_environment(NativeHostPlatform::Windows)
            .expect("Windows config loads")
            .expect("Windows explicit env config is present");

        assert_eq!(config.wsl.distro, "Ubuntu-24.04");
    }

    #[test]
    fn windows_runtime_selection_rejects_missing_config_without_in_process_fallback() {
        let error = select_runtime(None, None, NativeHostPlatform::Windows, None)
            .expect_err("Windows requires config");

        assert_eq!(error, RuntimeSelectionError::WindowsConfigRequired);
    }

    #[test]
    fn non_windows_runtime_selection_keeps_in_process_default() {
        let selection =
            select_runtime(None, None, NativeHostPlatform::Other, None).expect("runtime selected");

        assert_eq!(selection, RuntimeSelection::InProcess);
    }

    #[test]
    fn wsl_command_builder_uses_argv_without_shell_concatenation() {
        let config = parse_native_host_config(&valid_config()).expect("config parses");

        let command = build_wsl_daemon_command(&config.wsl);

        assert_eq!(command.program, "wsl.exe");
        assert_eq!(
            command.args,
            vec![
                "-d",
                "Ubuntu-24.04",
                "--cd",
                "/home/susu/screen sidekick",
                "--exec",
                "/home/susu/screen sidekick/target/debug/screen-sidekick-daemon",
                "--stdio-status"
            ]
        );
    }

    #[test]
    fn wsl_command_builder_can_apply_explicit_path_without_shell_concatenation() {
        let config = parse_native_host_config(&valid_config_with_path()).expect("config parses");

        let command = build_wsl_daemon_command(&config.wsl);

        assert_eq!(command.program, "wsl.exe");
        assert_eq!(
            command.args,
            vec![
                "-d",
                "Ubuntu-24.04",
                "--cd",
                "/home/susu/screen sidekick",
                "--exec",
                "env",
                "PATH=/home/susu/.nvm/versions/node/v22.20.0/bin:/home/susu/.cargo/bin:/usr/local/bin:/usr/bin:/bin",
                "/home/susu/screen sidekick/target/debug/screen-sidekick-daemon",
                "--stdio-status"
            ]
        );
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
}
