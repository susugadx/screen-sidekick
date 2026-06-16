#![cfg(unix)]

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use futures_util::StreamExt as _;
use screen_sidekick_codex_client::{
    CodexClientErrorKind, CodexTurnClient, StartTurnOutcome, StartTurnRequest, StdioCodexClient,
};

#[tokio::test]
async fn stdio_client_restarts_after_startup_stdout_eof() {
    let temp_dir = tempfile::tempdir().expect("temp dir is created");
    let count_path = temp_dir.path().join("spawn-count");
    let script_path =
        write_fake_codex_script(temp_dir.path(), &fake_codex_script(&count_path, true));
    let client = StdioCodexClient::new(script_path);

    let first = match client.start_turn(start_request("first")).await {
        Ok(_) => panic!("first process succeeded unexpectedly"),
        Err(error) => error,
    };
    let second = client
        .start_turn(start_request("second"))
        .await
        .expect("second process starts after cache is cleared");

    assert_eq!(first.kind, CodexClientErrorKind::AppServerUnavailable);
    assert_eq!(second.codex_thread_id, "thread_2");
    assert_eq!(second.codex_turn_id.as_deref(), Some("turn_2"));
    assert_eq!(read_spawn_count(&count_path), 2);
}

#[tokio::test]
async fn stdio_client_restarts_after_startup_timeout() {
    let temp_dir = tempfile::tempdir().expect("temp dir is created");
    let count_path = temp_dir.path().join("spawn-count");
    let script_path = write_fake_codex_script(
        temp_dir.path(),
        &fake_hanging_then_success_codex_script(&count_path),
    );
    let client =
        StdioCodexClient::new_with_startup_timeout(script_path, Duration::from_millis(500));

    let first = match client.start_turn(start_request("first")).await {
        Ok(_) => panic!("first process succeeded unexpectedly"),
        Err(error) => error,
    };
    let second = client
        .start_turn(start_request("second"))
        .await
        .expect("second process starts after timeout clears cache");

    assert_eq!(first.kind, CodexClientErrorKind::AppServerUnavailable);
    assert!(first.message.contains("timed out"));
    assert_eq!(second.codex_thread_id, "thread_2");
    assert_eq!(second.codex_turn_id.as_deref(), Some("turn_2"));
    assert_eq!(read_spawn_count(&count_path), 2);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn stdio_client_reaps_process_after_startup_timeout() {
    let temp_dir = tempfile::tempdir().expect("temp dir is created");
    let pid_path = temp_dir.path().join("pid");
    let script_path =
        write_fake_codex_script(temp_dir.path(), &fake_hanging_codex_script(&pid_path));
    let client =
        StdioCodexClient::new_with_startup_timeout(script_path, Duration::from_millis(500));

    let error = match client.start_turn(start_request("first")).await {
        Ok(_) => panic!("hanging process succeeded unexpectedly"),
        Err(error) => error,
    };
    let pid = std::fs::read_to_string(&pid_path)
        .expect("pid is written")
        .parse()
        .expect("pid parses");

    assert_eq!(error.kind, CodexClientErrorKind::AppServerUnavailable);
    assert_linux_process_reaped(pid).await;
}

#[tokio::test]
async fn stdio_client_restarts_after_stream_stdout_eof() {
    let temp_dir = tempfile::tempdir().expect("temp dir is created");
    let count_path = temp_dir.path().join("spawn-count");
    let script_path =
        write_fake_codex_script(temp_dir.path(), &fake_codex_script(&count_path, false));
    let client = StdioCodexClient::new(script_path);

    let mut first = client
        .start_turn(start_request("first"))
        .await
        .expect("first process starts")
        .events;
    let stream_error = first
        .next()
        .await
        .expect("stream emits EOF error")
        .expect_err("EOF is reported as an error");
    let second = start_turn_after_stream_cleanup(&client, start_request("second")).await;

    assert_eq!(
        stream_error.kind,
        CodexClientErrorKind::AppServerUnavailable
    );
    assert_eq!(second.codex_thread_id, "thread_2");
    assert_eq!(second.codex_turn_id.as_deref(), Some("turn_2"));
    assert_eq!(read_spawn_count(&count_path), 2);
}

#[tokio::test]
async fn stdio_client_accepts_immediate_follow_up_after_terminal_event() {
    let temp_dir = tempfile::tempdir().expect("temp dir is created");
    let count_path = temp_dir.path().join("spawn-count");
    let script_path = write_fake_codex_script(
        temp_dir.path(),
        &fake_reusable_terminal_codex_script(&count_path),
    );
    let client = StdioCodexClient::new(script_path);

    let mut first = client
        .start_turn(start_request("first"))
        .await
        .expect("first process starts")
        .events;
    let terminal = first
        .next()
        .await
        .expect("stream emits terminal event")
        .expect("terminal event parses");
    let second = client
        .start_turn(start_request("second"))
        .await
        .expect("second turn starts immediately after terminal event");

    assert!(matches!(
        terminal,
        screen_sidekick_codex_client::CodexEvent::Completed { .. }
    ));
    assert_eq!(second.codex_thread_id, "thread_2");
    assert_eq!(second.codex_turn_id.as_deref(), Some("turn_2"));
    assert_eq!(read_spawn_count(&count_path), 1);
}

#[tokio::test]
async fn stdio_client_resumes_stored_thread_after_process_restart() {
    let temp_dir = tempfile::tempdir().expect("temp dir is created");
    let count_path = temp_dir.path().join("spawn-count");
    let log_path = temp_dir.path().join("requests.log");
    let script_path = write_fake_codex_script(
        temp_dir.path(),
        &fake_resume_codex_script(&count_path, &log_path),
    );
    let client = StdioCodexClient::new(script_path);

    let mut first = client
        .start_turn(start_request("first"))
        .await
        .expect("first process starts")
        .events;
    let stream_error = first
        .next()
        .await
        .expect("stream emits EOF error")
        .expect_err("EOF is reported as an error");

    let mut second_request = start_request("second");
    second_request.codex_thread_id = Some("thread_1".to_owned());
    let second = start_turn_after_stream_cleanup(&client, second_request).await;

    assert_eq!(
        stream_error.kind,
        CodexClientErrorKind::AppServerUnavailable
    );
    assert_eq!(second.codex_thread_id, "thread_1");
    assert_eq!(second.codex_turn_id.as_deref(), Some("turn_2"));
    assert_eq!(read_spawn_count(&count_path), 2);
    assert_eq!(
        read_request_log(&log_path),
        [
            "1 initialize",
            "1 initialized",
            "1 thread/start",
            "1 turn/start",
            "2 initialize",
            "2 initialized",
            "2 thread/resume",
            "2 turn/start",
        ]
    );
}

#[tokio::test]
async fn stdio_client_classifies_unresumable_thread_resume_errors() {
    for message in [
        "thread not found",
        "no rollout found for thread id 018f4c18-9d6d-7000-a000-000000000000",
    ] {
        let temp_dir = tempfile::tempdir().expect("temp dir is created");
        let script_path =
            write_fake_codex_script(temp_dir.path(), &fake_missing_thread_codex_script(message));
        let client = StdioCodexClient::new(script_path);
        let mut request = start_request("missing");
        request.codex_thread_id = Some("missing_thread".to_owned());

        let error = match client.start_turn(request).await {
            Ok(_) => panic!("missing thread resume unexpectedly succeeded"),
            Err(error) => error,
        };

        assert_eq!(error.kind, CodexClientErrorKind::ThreadNotFound);
        assert_eq!(error.message, message);
    }
}

#[tokio::test]
async fn readiness_initializes_app_server_before_reporting_available() {
    let temp_dir = tempfile::tempdir().expect("temp dir is created");
    let count_path = temp_dir.path().join("spawn-count");
    let log_path = temp_dir.path().join("requests.log");
    let script_path = write_fake_codex_script(
        temp_dir.path(),
        &fake_readiness_success_codex_script(&count_path, &log_path),
    );
    let client = StdioCodexClient::new(script_path);

    let readiness = client.readiness().await;

    assert!(readiness.available);
    assert_eq!(readiness.version.as_deref(), Some("codex-fake 1.0.0"));
    assert_eq!(readiness.error, None);
    assert_eq!(read_spawn_count(&count_path), 1);
    assert_eq!(
        wait_for_request_log(&log_path, 2),
        ["1 initialize", "1 initialized"]
    );
}

#[tokio::test]
async fn readiness_reports_unsupported_when_version_succeeds_but_app_server_eofs() {
    let temp_dir = tempfile::tempdir().expect("temp dir is created");
    let count_path = temp_dir.path().join("spawn-count");
    let script_path = write_fake_codex_script(
        temp_dir.path(),
        &fake_readiness_eof_codex_script(&count_path),
    );
    let client = StdioCodexClient::new(script_path);

    let readiness = client.readiness().await;

    assert!(!readiness.available);
    assert_eq!(readiness.version.as_deref(), Some("codex-fake 1.0.0"));
    assert_eq!(
        readiness.error,
        Some(CodexClientErrorKind::UnsupportedVersion)
    );
    assert_eq!(read_spawn_count(&count_path), 1);
}

#[tokio::test]
async fn readiness_reports_unsupported_when_initialize_times_out() {
    let temp_dir = tempfile::tempdir().expect("temp dir is created");
    let count_path = temp_dir.path().join("spawn-count");
    let script_path = write_fake_codex_script(
        temp_dir.path(),
        &fake_readiness_hanging_codex_script(&count_path),
    );
    let client =
        StdioCodexClient::new_with_startup_timeout(script_path, Duration::from_millis(500));

    let readiness = client.readiness().await;

    assert!(!readiness.available);
    assert_eq!(readiness.version.as_deref(), Some("codex-fake 1.0.0"));
    assert_eq!(
        readiness.error,
        Some(CodexClientErrorKind::UnsupportedVersion)
    );
    assert_eq!(read_spawn_count(&count_path), 1);
}

#[tokio::test]
async fn readiness_reports_unavailable_when_version_probe_times_out() {
    let temp_dir = tempfile::tempdir().expect("temp dir is created");
    let pid_path = temp_dir.path().join("version-pid");
    let script_path = write_fake_codex_script(
        temp_dir.path(),
        &fake_hanging_version_codex_script(&pid_path),
    );
    let client =
        StdioCodexClient::new_with_startup_timeout(script_path, Duration::from_millis(300));

    let readiness = tokio::time::timeout(Duration::from_secs(2), client.readiness())
        .await
        .expect("readiness is bounded by the version probe timeout");

    assert!(!readiness.available);
    assert_eq!(readiness.version, None);
    assert_eq!(
        readiness.error,
        Some(CodexClientErrorKind::AppServerUnavailable)
    );

    #[cfg(target_os = "linux")]
    {
        let pid = std::fs::read_to_string(&pid_path)
            .expect("version probe pid is written")
            .parse()
            .expect("version probe pid parses");
        assert_linux_process_reaped(pid).await;
    }
}

#[tokio::test]
async fn readiness_probe_success_is_reused_for_first_turn() {
    let temp_dir = tempfile::tempdir().expect("temp dir is created");
    let count_path = temp_dir.path().join("spawn-count");
    let log_path = temp_dir.path().join("requests.log");
    let script_path = write_fake_codex_script(
        temp_dir.path(),
        &fake_readiness_success_codex_script(&count_path, &log_path),
    );
    let client = StdioCodexClient::new(script_path);

    let readiness = client.readiness().await;
    let outcome = client
        .start_turn(start_request("first"))
        .await
        .expect("first turn uses initialized probe process");

    assert!(readiness.available);
    assert_eq!(outcome.codex_thread_id, "thread_1");
    assert_eq!(outcome.codex_turn_id.as_deref(), Some("turn_1"));
    assert_eq!(read_spawn_count(&count_path), 1);
    assert_eq!(
        read_request_log(&log_path),
        [
            "1 initialize",
            "1 initialized",
            "1 thread/start",
            "1 turn/start",
        ]
    );
}

#[tokio::test]
async fn readiness_reports_ready_while_initialized_stdout_is_streaming() {
    let temp_dir = tempfile::tempdir().expect("temp dir is created");
    let count_path = temp_dir.path().join("spawn-count");
    let script_path = write_fake_codex_script(
        temp_dir.path(),
        &fake_holding_stream_codex_script(&count_path),
    );
    let client = StdioCodexClient::new(script_path);

    let mut events = client
        .start_turn(start_request("streaming"))
        .await
        .expect("turn starts and loans stdout to stream reader")
        .events;
    let readiness = client.readiness().await;
    let stream_error = events
        .next()
        .await
        .expect("stream emits EOF error")
        .expect_err("EOF is reported as an error");

    assert!(readiness.available);
    assert_eq!(readiness.version.as_deref(), Some("codex-fake 1.0.0"));
    assert_eq!(readiness.error, None);
    assert_eq!(
        stream_error.kind,
        CodexClientErrorKind::AppServerUnavailable
    );
    assert_eq!(read_spawn_count(&count_path), 1);
}

#[tokio::test]
async fn readiness_while_streaming_bounds_version_probe_timeout() {
    let temp_dir = tempfile::tempdir().expect("temp dir is created");
    let count_path = temp_dir.path().join("spawn-count");
    let pid_path = temp_dir.path().join("version-pid");
    let script_path = write_fake_codex_script(
        temp_dir.path(),
        &fake_holding_stream_hanging_version_codex_script(&count_path, &pid_path),
    );
    let client =
        StdioCodexClient::new_with_startup_timeout(script_path, Duration::from_millis(300));

    let _events = client
        .start_turn(start_request("streaming"))
        .await
        .expect("turn starts and loans stdout to stream reader")
        .events;
    let readiness = tokio::time::timeout(Duration::from_secs(2), client.readiness())
        .await
        .expect("streaming readiness is bounded by the version probe timeout");

    assert!(readiness.available);
    assert_eq!(readiness.version, None);
    assert_eq!(readiness.error, None);
    assert_eq!(read_spawn_count(&count_path), 1);

    #[cfg(target_os = "linux")]
    {
        let pid = std::fs::read_to_string(&pid_path)
            .expect("version probe pid is written")
            .parse()
            .expect("version probe pid parses");
        assert_linux_process_reaped(pid).await;
    }
}

fn fake_codex_script(count_path: &Path, first_exits_immediately: bool) -> String {
    let first_exit = if first_exits_immediately {
        r#"
if [ "$count" -eq 1 ]; then
  exit 0
fi
"#
    } else {
        ""
    };
    format!(
        r#"#!/bin/sh
count_file="{}"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s' "$count" > "$count_file"
{}
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '{{"id":"sidekick_req_1","result":{{}}}}\n'
      ;;
    *'"method":"thread/start"'*)
      printf '{{"id":"sidekick_req_2","result":{{"thread":{{"id":"thread_%s"}}}}}}\n' "$count"
      ;;
    *'"method":"turn/start"'*)
      printf '{{"id":"sidekick_req_3","result":{{"turn":{{"id":"turn_%s"}}}}}}\n' "$count"
      exit 0
      ;;
  esac
done
"#,
        count_path.display(),
        first_exit
    )
}

fn fake_readiness_success_codex_script(count_path: &Path, log_path: &Path) -> String {
    format!(
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf 'codex-fake 1.0.0\n'
  exit 0
fi
count_file="{}"
log_file="{}"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s' "$count" > "$count_file"
while IFS= read -r line; do
  id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s initialize\n' "$count" >> "$log_file"
      printf '{{"id":"%s","result":{{}}}}\n' "$id"
      ;;
    *'"method":"initialized"'*)
      printf '%s initialized\n' "$count" >> "$log_file"
      ;;
    *'"method":"thread/start"'*)
      printf '%s thread/start\n' "$count" >> "$log_file"
      printf '{{"id":"%s","result":{{"thread":{{"id":"thread_%s"}}}}}}\n' "$id" "$count"
      ;;
    *'"method":"turn/start"'*)
      printf '%s turn/start\n' "$count" >> "$log_file"
      printf '{{"id":"%s","result":{{"turn":{{"id":"turn_%s"}}}}}}\n' "$id" "$count"
      exit 0
      ;;
  esac
done
"#,
        count_path.display(),
        log_path.display()
    )
}

fn fake_readiness_eof_codex_script(count_path: &Path) -> String {
    format!(
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf 'codex-fake 1.0.0\n'
  exit 0
fi
count_file="{}"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s' "$count" > "$count_file"
exit 0
"#,
        count_path.display()
    )
}

fn fake_readiness_hanging_codex_script(count_path: &Path) -> String {
    format!(
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf 'codex-fake 1.0.0\n'
  exit 0
fi
count_file="{}"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s' "$count" > "$count_file"
while IFS= read -r _line; do
  :
done
"#,
        count_path.display()
    )
}

fn fake_hanging_version_codex_script(pid_path: &Path) -> String {
    format!(
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf '%s' "$$" > "{}"
  while :; do
    sleep 1
  done
fi
exit 0
"#,
        pid_path.display()
    )
}

fn fake_holding_stream_codex_script(count_path: &Path) -> String {
    format!(
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf 'codex-fake 1.0.0\n'
  exit 0
fi
count_file="{}"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s' "$count" > "$count_file"
while IFS= read -r line; do
  id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{{"id":"%s","result":{{}}}}\n' "$id"
      ;;
    *'"method":"thread/start"'*)
      printf '{{"id":"%s","result":{{"thread":{{"id":"thread_%s"}}}}}}\n' "$id" "$count"
      ;;
    *'"method":"turn/start"'*)
      printf '{{"id":"%s","result":{{"turn":{{"id":"turn_%s"}}}}}}\n' "$id" "$count"
      sleep 1
      exit 0
      ;;
  esac
done
"#,
        count_path.display()
    )
}

fn fake_holding_stream_hanging_version_codex_script(count_path: &Path, pid_path: &Path) -> String {
    format!(
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf '%s' "$$" > "{}"
  while :; do
    sleep 1
  done
fi
count_file="{}"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s' "$count" > "$count_file"
while IFS= read -r line; do
  id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{{"id":"%s","result":{{}}}}\n' "$id"
      ;;
    *'"method":"thread/start"'*)
      printf '{{"id":"%s","result":{{"thread":{{"id":"thread_%s"}}}}}}\n' "$id" "$count"
      ;;
    *'"method":"turn/start"'*)
      printf '{{"id":"%s","result":{{"turn":{{"id":"turn_%s"}}}}}}\n' "$id" "$count"
      sleep 2
      exit 0
      ;;
  esac
done
"#,
        pid_path.display(),
        count_path.display()
    )
}

fn fake_hanging_then_success_codex_script(count_path: &Path) -> String {
    format!(
        r#"#!/bin/sh
count_file="{}"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s' "$count" > "$count_file"
if [ "$count" -eq 1 ]; then
  while IFS= read -r _line; do
    :
  done
  exit 0
fi
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '{{"id":"sidekick_req_1","result":{{}}}}\n'
      ;;
    *'"method":"thread/start"'*)
      printf '{{"id":"sidekick_req_2","result":{{"thread":{{"id":"thread_%s"}}}}}}\n' "$count"
      ;;
    *'"method":"turn/start"'*)
      printf '{{"id":"sidekick_req_3","result":{{"turn":{{"id":"turn_%s"}}}}}}\n' "$count"
      exit 0
      ;;
  esac
done
"#,
        count_path.display()
    )
}

fn fake_hanging_codex_script(pid_path: &Path) -> String {
    format!(
        r#"#!/bin/sh
printf '%s' "$$" > "{}"
while IFS= read -r _line; do
  :
done
"#,
        pid_path.display()
    )
}

fn fake_reusable_terminal_codex_script(count_path: &Path) -> String {
    format!(
        r#"#!/bin/sh
count_file="{}"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s' "$count" > "$count_file"
thread=0
turn=0
while IFS= read -r line; do
  id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{{"id":"%s","result":{{}}}}\n' "$id"
      ;;
    *'"method":"thread/start"'*)
      thread=$((thread + 1))
      printf '{{"id":"%s","result":{{"thread":{{"id":"thread_%s"}}}}}}\n' "$id" "$thread"
      ;;
    *'"method":"turn/start"'*)
      turn=$((turn + 1))
      printf '{{"id":"%s","result":{{"turn":{{"id":"turn_%s"}}}}}}\n' "$id" "$turn"
      printf '{{"method":"turn/completed","params":{{"threadId":"thread_%s","turn":{{"id":"turn_%s","items":[],"status":"completed"}}}}}}\n' "$thread" "$turn"
      ;;
  esac
done
"#,
        count_path.display()
    )
}

fn fake_resume_codex_script(count_path: &Path, log_path: &Path) -> String {
    format!(
        r#"#!/bin/sh
count_file="{}"
log_file="{}"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf '%s' "$count" > "$count_file"
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s initialize\n' "$count" >> "$log_file"
      printf '{{"id":"sidekick_req_1","result":{{}}}}\n'
      ;;
    *'"method":"initialized"'*)
      printf '%s initialized\n' "$count" >> "$log_file"
      ;;
    *'"method":"thread/start"'*)
      printf '%s thread/start\n' "$count" >> "$log_file"
      printf '{{"id":"sidekick_req_2","result":{{"thread":{{"id":"thread_%s"}}}}}}\n' "$count"
      ;;
    *'"method":"thread/resume"'*)
      printf '%s thread/resume\n' "$count" >> "$log_file"
      printf '{{"id":"sidekick_req_2","result":{{"thread":{{"id":"thread_1"}}}}}}\n'
      ;;
    *'"method":"turn/start"'*)
      printf '%s turn/start\n' "$count" >> "$log_file"
      printf '{{"id":"sidekick_req_3","result":{{"turn":{{"id":"turn_%s"}}}}}}\n' "$count"
      exit 0
      ;;
  esac
done
"#,
        count_path.display(),
        log_path.display()
    )
}

fn fake_missing_thread_codex_script(message: &str) -> String {
    let message = message.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '{{"id":"sidekick_req_1","result":{{}}}}\n'
      ;;
    *'"method":"thread/resume"'*)
      printf '{{"id":"sidekick_req_2","error":{{"message":"{}"}}}}\n'
      ;;
  esac
done
"#,
        message
    )
}

fn write_fake_codex_script(temp_dir: &Path, script: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;

    let script_path = temp_dir.join("fake-codex");
    std::fs::write(&script_path, script).expect("fake codex script is written");
    let mut permissions = std::fs::metadata(&script_path)
        .expect("script metadata loads")
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&script_path, permissions).expect("script is executable");
    std::thread::sleep(Duration::from_millis(20));
    script_path
}

fn read_spawn_count(count_path: &Path) -> usize {
    std::fs::read_to_string(count_path)
        .expect("spawn count is written")
        .parse()
        .expect("spawn count parses")
}

fn read_request_log(log_path: &Path) -> Vec<String> {
    std::fs::read_to_string(log_path)
        .expect("request log is written")
        .lines()
        .map(ToOwned::to_owned)
        .collect()
}

fn wait_for_request_log(log_path: &Path, expected_len: usize) -> Vec<String> {
    for _ in 0..100 {
        if let Ok(text) = std::fs::read_to_string(log_path) {
            let lines: Vec<String> = text.lines().map(ToOwned::to_owned).collect();
            if lines.len() >= expected_len {
                return lines;
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    read_request_log(log_path)
}

#[cfg(target_os = "linux")]
async fn assert_linux_process_reaped(pid: u32) {
    let stat_path = format!("/proc/{pid}/stat");
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if std::fs::read_to_string(&stat_path).is_err() {
            return;
        }
        if Instant::now() >= deadline {
            panic!("codex app-server process {pid} was not reaped");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn start_request(label: &str) -> StartTurnRequest {
    StartTurnRequest {
        session_id: format!("session_{label}"),
        codex_thread_id: None,
        user_message_id: format!("message_{label}"),
        user_text: format!("question {label}"),
        context_text: String::new(),
    }
}

async fn start_turn_after_stream_cleanup(
    client: &StdioCodexClient,
    request: StartTurnRequest,
) -> StartTurnOutcome {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match client.start_turn(request.clone()).await {
                Ok(outcome) => return outcome,
                Err(error) if error.message.contains("already streaming") => {
                    tokio::task::yield_now().await;
                }
                Err(error) => panic!("start_turn failed unexpectedly: {error:?}"),
            }
        }
    })
    .await
    .expect("stream cleanup completes before retry timeout")
}
