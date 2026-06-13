#![cfg(unix)]

use std::{
    path::{Path, PathBuf},
    time::Duration,
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
    let client = StdioCodexClient::new_with_startup_timeout(script_path, Duration::from_millis(10));

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
            "1 thread/start",
            "1 turn/start",
            "2 initialize",
            "2 thread/resume",
            "2 turn/start",
        ]
    );
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

fn write_fake_codex_script(temp_dir: &Path, script: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;

    let script_path = temp_dir.join("fake-codex");
    std::fs::write(&script_path, script).expect("fake codex script is written");
    let mut permissions = std::fs::metadata(&script_path)
        .expect("script metadata loads")
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&script_path, permissions).expect("script is executable");
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
