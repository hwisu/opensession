use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn create_session_fixture(root: &Path, turns: usize) -> PathBuf {
    let session_dir = root.join(".codex").join("sessions");
    fs::create_dir_all(&session_dir).expect("create fixture session dir");
    let session_path = session_dir.join("timeline-linear-test.jsonl");

    let mut lines = Vec::new();
    for i in 0..turns {
        let t = i + 1;
        let base = i * 2;
        lines.push(format!(
            r#"{{"type":"user","uuid":"u{}","sessionId":"timeline-linear-test","timestamp":"2026-02-14T00:00:{:02}Z","message":{{"role":"user","content":"turn {} user prompt"}}}}"#,
            t,
            base + 1,
            t
        ));
        lines.push(format!(
            r#"{{"type":"assistant","uuid":"a{}","sessionId":"timeline-linear-test","timestamp":"2026-02-14T00:00:{:02}Z","message":{{"role":"assistant","model":"claude-opus-4-6","content":[{{"type":"text","text":"turn {} assistant response"}}]}}}}"#,
            t,
            base + 2,
            t
        ));
    }

    fs::write(&session_path, lines.join("\n")).expect("write fixture session");
    session_path
}

#[cfg(unix)]
fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).expect("write executable");
    let mut perms = fs::metadata(path).expect("metadata").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).expect("chmod");
}

fn base_timeline_command(home: &Path, session_path: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_opensession"));
    cmd.arg("session")
        .arg("timeline")
        .arg(session_path)
        .arg("--format")
        .arg("json")
        .arg("--view")
        .arg("linear")
        .env("HOME", home)
        .env("NO_COLOR", "1");

    for key in [
        "OPS_TL_SUM_CLI_ARGS",
        "OPS_TL_SUM_MODEL",
        "ANTHROPIC_API_KEY",
        "OPENAI_API_KEY",
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "OPS_TL_SUM_ENDPOINT",
        "OPS_TL_SUM_BASE",
        "OPS_TL_SUM_KEY",
    ] {
        cmd.env_remove(key);
    }

    cmd
}

fn run_with_elapsed(mut cmd: Command) -> (Output, Duration) {
    let started = Instant::now();
    let output = cmd.output().expect("run opensession session timeline");
    (output, started.elapsed())
}

fn parse_json_output(output: &Output) -> Value {
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!(
            "timeline command failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status, stdout, stderr
        );
    }
    serde_json::from_slice(&output.stdout).expect("timeline json output")
}

#[cfg(unix)]
#[test]
fn summary_calls_are_linear_with_max_inflight_one() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    fs::create_dir_all(home.join(".config").join("opensession")).expect("home config dir");
    let session_path = create_session_fixture(temp.path(), 3);

    let lock_dir = temp.path().join("summary-lock");
    let concurrent_marker = temp.path().join("concurrent-calls.marker");
    fs::remove_file(&concurrent_marker).ok();

    let cli_script = temp.path().join("fake-summary-slow.sh");
    write_executable(
        &cli_script,
        r#"#!/usr/bin/env bash
set -eu

LOCK_DIR="${OPS_TL_SUM_LOCK_DIR}"
CONCURRENT_MARKER="${OPS_TL_SUM_CONCURRENT_MARKER}"

while ! mkdir "$LOCK_DIR" 2>/dev/null; do
  echo "CONCURRENT" >> "$CONCURRENT_MARKER"
  sleep 0.01
done
trap 'rmdir "$LOCK_DIR" 2>/dev/null || true' EXIT

sleep 1
echo '{"kind":"turn-summary","version":"2.0","scope":"turn","turn_meta":{"turn_index":0,"anchor_event_index":0,"event_span":{"start":0,"end":0}},"prompt":{"text":"prompt","intent":"test intent","constraints":[]},"outcome":{"status":"completed","summary":"step complete"},"evidence":{"modified_files":[],"key_implementations":[],"agent_quotes":[],"agent_plan":[],"tool_actions":[],"errors":[]},"cards":[{"type":"overview","title":"Overview","lines":["step complete"],"severity":"info"}],"next_steps":["continue"]}'
"#,
    );

    let mut cmd = base_timeline_command(&home, &session_path);
    cmd.arg("--summaries")
        .arg("--summary-provider")
        .arg("auto")
        .env("OPS_TL_SUM_CLI_BIN", cli_script)
        .env("OPS_TL_SUM_LOCK_DIR", &lock_dir)
        .env("OPS_TL_SUM_CONCURRENT_MARKER", &concurrent_marker);

    let (output, _elapsed) = run_with_elapsed(cmd);
    let json = parse_json_output(&output);

    let generated = json
        .get("generated_summaries")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(
        generated >= 3,
        "expected at least 3 generated summaries for 3 turns, got {generated}"
    );

    let marker = std::fs::read_to_string(&concurrent_marker).unwrap_or_default();
    assert!(
        !marker.contains("CONCURRENT"),
        "no parallel summary CLI calls expected; found marker {marker}"
    );
    assert!(
        !lock_dir.exists(),
        "lock directory should be released after test run"
    );

    let lines = json
        .get("lines")
        .and_then(|v| v.as_array())
        .expect("lines array");
    let has_turn_summary = lines.iter().any(|line| {
        line.as_str()
            .is_some_and(|text| text.contains("[turn-summary:"))
    });
    assert!(has_turn_summary, "expected rendered turn-summary lines");
}

#[cfg(unix)]
#[test]
fn summary_timeout_is_bounded_via_cli_call() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    fs::create_dir_all(home.join(".config").join("opensession")).expect("home config dir");
    let session_path = create_session_fixture(temp.path(), 1);

    let cli_script = temp.path().join("fake-summary-timeout.sh");
    write_executable(
        &cli_script,
        r#"#!/usr/bin/env bash
set -eu
sleep 5
echo '{"kind":"turn-summary","version":"2.0","scope":"turn","turn_meta":{"turn_index":0,"anchor_event_index":0,"event_span":{"start":0,"end":0}},"prompt":{"text":"prompt","intent":"late","constraints":[]},"outcome":{"status":"in_progress","summary":"late"},"evidence":{"modified_files":[],"key_implementations":[],"agent_quotes":[],"agent_plan":[],"tool_actions":[],"errors":[]},"cards":[{"type":"overview","title":"Overview","lines":["late"],"severity":"warn"}],"next_steps":[]}'
"#,
    );

    let mut cmd = base_timeline_command(&home, &session_path);
    cmd.arg("--summaries")
        .arg("--summary-provider")
        .arg("auto")
        .env("OPS_TL_SUM_CLI_BIN", cli_script)
        .env("OPS_TL_SUM_CLI_TIMEOUT_MS", "1000");

    let (output, _elapsed) = run_with_elapsed(cmd);
    let json = parse_json_output(&output);
    let generated = json
        .get("generated_summaries")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(generated >= 1, "expected at least one attempted summary");
    assert!(
        generated <= 2,
        "expected summary queue to stop quickly after timeout, got {generated} attempts"
    );
}

#[cfg(unix)]
#[test]
fn no_summary_flag_skips_summary_cli_invocation() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    fs::create_dir_all(home.join(".config").join("opensession")).expect("home config dir");
    let session_path = create_session_fixture(temp.path(), 2);

    let marker = temp.path().join("cli-called.marker");
    let cli_script = temp.path().join("fake-summary-marker.sh");
    write_executable(
        &cli_script,
        r#"#!/usr/bin/env bash
set -eu
if [ -n "${OPS_MARKER_FILE:-}" ]; then
  echo "called" >> "${OPS_MARKER_FILE}"
fi
echo '{"kind":"turn-summary","version":"2.0","scope":"turn","turn_meta":{"turn_index":0,"anchor_event_index":0,"event_span":{"start":0,"end":0}},"prompt":{"text":"prompt","intent":"x","constraints":[]},"outcome":{"status":"completed","summary":"x"},"evidence":{"modified_files":[],"key_implementations":[],"agent_quotes":[],"agent_plan":[],"tool_actions":[],"errors":[]},"cards":[{"type":"overview","title":"Overview","lines":["x"],"severity":"info"}],"next_steps":[]}'
"#,
    );

    let mut cmd = base_timeline_command(&home, &session_path);
    cmd.arg("--no-summary")
        .arg("--summary-provider")
        .arg("cli:codex")
        .env("OPS_TL_SUM_CLI_BIN", cli_script)
        .env("OPS_MARKER_FILE", &marker);

    let (output, _elapsed) = run_with_elapsed(cmd);
    let json = parse_json_output(&output);
    let generated = json
        .get("generated_summaries")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert_eq!(generated, 0, "expected zero generated summaries");
    assert!(
        !marker.exists(),
        "summary CLI should not be invoked when --no-summary is set"
    );
}
