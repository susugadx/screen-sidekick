#![forbid(unsafe_code)]

mod http_boundary;
mod protocol;

use std::{
    error::Error,
    fmt, fs, io,
    net::{Ipv4Addr, TcpListener},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

use axum::{
    body::Body,
    extract::{ws::WebSocketUpgrade, DefaultBodyLimit, State},
    http::{HeaderMap, Response},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use directories::ProjectDirs;
use http_boundary::{
    legacy_capture, legacy_preflight, validate_extension_origin_for_optional_header,
    DaemonRejection,
};
use protocol::websocket_loop;
use screen_sidekick_codex_client::{CodexTurnClient, StdioCodexClient};
use screen_sidekick_session::{SessionStore, SessionStoreError};
use screen_sidekick_sidekick_protocol::{JsonRpcNotification, ProtocolLimits};
use serde::Serialize;
use tokio::sync::{broadcast, oneshot};
use uuid::Uuid;

pub const DAEMON_STATUS_SCHEMA_VERSION: &str = "sidekick_daemon_status.v0.1";
pub const MAX_CAPTURE_BODY_BYTES: usize = 128 * 1024;
pub const MAX_WS_MESSAGE_BYTES: usize = 256 * 1024;
pub const MAX_ATTACHMENT_BYTES: usize = 128 * 1024;
pub const DEFAULT_EVENT_BUFFER_CAPACITY: usize = 256;
pub const DEFAULT_CODEX_START_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DaemonStatus {
    pub schema_version: String,
    pub url: String,
    pub ws_url: String,
    pub token: String,
    pub status: DaemonRuntimeStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonRuntimeStatus {
    Running,
}

#[derive(Clone)]
pub struct DaemonState {
    token: String,
    store: SessionStore,
    codex: Arc<dyn CodexTurnClient>,
    events: broadcast::Sender<JsonRpcNotification>,
    websocket_shutdown: broadcast::Sender<()>,
    limits: ProtocolLimits,
    codex_start_timeout: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DaemonOptions {
    pub event_buffer_capacity: usize,
    pub codex_start_timeout: Duration,
}

impl Default for DaemonOptions {
    fn default() -> Self {
        Self {
            event_buffer_capacity: DEFAULT_EVENT_BUFFER_CAPACITY,
            codex_start_timeout: DEFAULT_CODEX_START_TIMEOUT,
        }
    }
}

impl DaemonState {
    #[must_use]
    pub fn new(
        token: impl Into<String>,
        store: SessionStore,
        codex: Arc<dyn CodexTurnClient>,
    ) -> Self {
        Self::new_with_options(token, store, codex, DaemonOptions::default())
    }

    #[must_use]
    pub fn new_with_options(
        token: impl Into<String>,
        store: SessionStore,
        codex: Arc<dyn CodexTurnClient>,
        options: DaemonOptions,
    ) -> Self {
        let (events, _) = broadcast::channel(options.event_buffer_capacity.max(1));
        let (websocket_shutdown, _) = broadcast::channel(1);
        Self {
            token: token.into(),
            store,
            codex,
            events,
            websocket_shutdown,
            limits: ProtocolLimits {
                max_message_bytes: MAX_WS_MESSAGE_BYTES,
                max_attachment_bytes: MAX_ATTACHMENT_BYTES,
            },
            codex_start_timeout: options.codex_start_timeout,
        }
    }

    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }
}

pub struct DaemonRuntime {
    shutdown: Mutex<Option<oneshot::Sender<()>>>,
    websocket_shutdown: broadcast::Sender<()>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl DaemonRuntime {
    pub fn start() -> Result<(Self, DaemonStatus), DaemonStartError> {
        let token = Uuid::new_v4().to_string();
        let db_path = default_database_path().map_err(DaemonStartError::DatabasePath)?;
        let store = SessionStore::open(db_path).map_err(DaemonStartError::SessionStore)?;
        let codex = Arc::new(StdioCodexClient::default());
        Self::start_with_state(DaemonState::new(token, store, codex))
    }

    pub fn start_with_state(state: DaemonState) -> Result<(Self, DaemonStatus), DaemonStartError> {
        state
            .store
            .recover_interrupted_active_turns()
            .map_err(DaemonStartError::SessionStore)?;
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(DaemonStartError::Bind)?;
        listener
            .set_nonblocking(true)
            .map_err(DaemonStartError::SetNonblocking)?;
        let address = listener.local_addr().map_err(DaemonStartError::LocalAddr)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(DaemonStartError::Runtime)?;
        let status = DaemonStatus {
            schema_version: DAEMON_STATUS_SCHEMA_VERSION.to_owned(),
            url: format!("http://{address}"),
            ws_url: format!("ws://{address}/v0/ws"),
            token: state.token.clone(),
            status: DaemonRuntimeStatus::Running,
        };
        let websocket_shutdown = state.websocket_shutdown.clone();
        let router = build_daemon_router(state);
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let thread = spawn_daemon_thread(runtime, listener, router, shutdown_receiver);
        Ok((
            Self {
                shutdown: Mutex::new(Some(shutdown_sender)),
                websocket_shutdown,
                thread: Mutex::new(Some(thread)),
            },
            status,
        ))
    }
}

impl Drop for DaemonRuntime {
    fn drop(&mut self) {
        if let Ok(mut shutdown) = self.shutdown.lock() {
            if let Some(sender) = shutdown.take() {
                let _ = sender.send(());
            }
        }
        let _ = self.websocket_shutdown.send(());

        if let Ok(mut thread) = self.thread.lock() {
            if let Some(handle) = thread.take() {
                let _ = handle.join();
            }
        }
    }
}

#[derive(Debug)]
pub enum DaemonStartError {
    Bind(io::Error),
    SetNonblocking(io::Error),
    LocalAddr(io::Error),
    Runtime(io::Error),
    DatabasePath(io::Error),
    SessionStore(SessionStoreError),
}

impl fmt::Display for DaemonStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bind(_) => formatter.write_str("failed to bind loopback daemon"),
            Self::SetNonblocking(_) => formatter.write_str("failed to prepare loopback listener"),
            Self::LocalAddr(_) => formatter.write_str("failed to read loopback daemon address"),
            Self::Runtime(_) => formatter.write_str("failed to start loopback daemon runtime"),
            Self::DatabasePath(_) => formatter.write_str("failed to prepare daemon database path"),
            Self::SessionStore(_) => formatter.write_str("failed to start session store"),
        }
    }
}

impl Error for DaemonStartError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Bind(error)
            | Self::SetNonblocking(error)
            | Self::LocalAddr(error)
            | Self::Runtime(error)
            | Self::DatabasePath(error) => Some(error),
            Self::SessionStore(error) => Some(error),
        }
    }
}

pub fn build_daemon_router(state: DaemonState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/v0/ws", get(ws_upgrade))
        .route(
            "/v0/capture",
            post(legacy_capture).options(legacy_preflight),
        )
        .with_state(state)
        .layer(DefaultBodyLimit::max(MAX_CAPTURE_BODY_BYTES))
}

fn spawn_daemon_thread(
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
                    eprintln!("Screen Sidekick daemon failed to accept loopback listener: {error}");
                    return;
                }
            };
            let server = axum::serve(listener, router).with_graceful_shutdown(async {
                let _ = shutdown_receiver.await;
            });
            if let Err(error) = server.await {
                eprintln!("Screen Sidekick daemon stopped: {error}");
            }
        });
    })
}

async fn healthz() -> &'static str {
    "ok"
}

async fn readyz() -> &'static str {
    "ready"
}

async fn ws_upgrade(
    State(state): State<DaemonState>,
    headers: HeaderMap,
    websocket: WebSocketUpgrade,
) -> Result<Response<Body>, DaemonRejection> {
    validate_extension_origin_for_optional_header(&headers)?;
    Ok(websocket
        .max_message_size(MAX_WS_MESSAGE_BYTES)
        .on_upgrade(move |socket| websocket_loop(socket, state))
        .into_response())
}

fn default_database_path() -> io::Result<PathBuf> {
    let project_dirs = ProjectDirs::from("", "", "screen-sidekick").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "could not determine Screen Sidekick app data directory",
        )
    })?;
    prepare_database_path(project_dirs.data_dir())
}

fn prepare_database_path(data_dir: &Path) -> io::Result<PathBuf> {
    create_private_data_dir(data_dir)?;
    let database_path = data_dir.join("screen-sidekick.sqlite3");
    create_private_database_file(&database_path)?;
    Ok(database_path)
}

#[cfg(unix)]
fn create_private_data_dir(data_dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(0o700);
    builder.create(data_dir)?;
    fs::set_permissions(data_dir, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn create_private_data_dir(data_dir: &Path) -> io::Result<()> {
    fs::create_dir_all(data_dir)
}

#[cfg(unix)]
fn create_private_database_file(database_path: &Path) -> io::Result<()> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(database_path)?;
    fs::set_permissions(database_path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn create_private_database_file(database_path: &Path) -> io::Result<()> {
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(database_path)
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn default_database_path_uses_xdg_app_data_directory() {
        let _guard = ENV_LOCK.lock().expect("env lock is not poisoned");
        let temp = tempfile::tempdir().expect("temp dir is created");
        let _xdg_data_home = EnvVarGuard::set("XDG_DATA_HOME", temp.path());

        let path = super::default_database_path().expect("database path is prepared");

        assert!(path.starts_with(temp.path()));
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("screen-sidekick.sqlite3")
        );
        assert!(path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn database_path_prepares_private_unix_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("temp dir is created");
        let data_dir = temp.path().join("screen-sidekick");
        std::fs::create_dir_all(&data_dir).expect("data dir is created");
        std::fs::set_permissions(&data_dir, std::fs::Permissions::from_mode(0o755))
            .expect("data dir mode is widened");
        let database_path = data_dir.join("screen-sidekick.sqlite3");
        std::fs::write(&database_path, b"").expect("database file is created");
        std::fs::set_permissions(&database_path, std::fs::Permissions::from_mode(0o644))
            .expect("database file mode is widened");

        let prepared_path =
            super::prepare_database_path(&data_dir).expect("database path is prepared");

        assert_eq!(prepared_path, database_path);
        assert_eq!(
            std::fs::metadata(&data_dir)
                .expect("data dir metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&prepared_path)
                .expect("database file metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
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
