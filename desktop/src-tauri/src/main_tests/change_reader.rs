use super::*;

fn change_reader_update(
    qa_enabled: bool,
    voice_enabled: bool,
    api_key: Option<&str>,
) -> DesktopRuntimeChangeReaderSettingsUpdate {
    DesktopRuntimeChangeReaderSettingsUpdate {
        enabled: true,
        scope: DesktopChangeReaderScope::SummaryOnly,
        qa_enabled,
        max_context_chars: 12_000,
        voice: DesktopRuntimeChangeReaderVoiceSettingsUpdate {
            enabled: voice_enabled,
            provider: DesktopChangeReaderVoiceProvider::Openai,
            model: "gpt-4o-mini-tts".to_string(),
            voice: "alloy".to_string(),
            api_key: api_key.map(str::to_string),
        },
    }
}

#[test]
fn desktop_change_reader_requires_enabled_setting() {
    let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
    let temp_home = unique_temp_dir("opensession-desktop-change-reader-disabled");
    let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());

    let result =
        tauri::async_runtime::block_on(desktop_read_session_changes(DesktopChangeReadRequest {
            session_id: "session-1".to_string(),
            scope: None,
        }));
    let error = result.expect_err("disabled change reader should fail");
    assert_eq!(error.status, 422);
    assert_eq!(error.code, "desktop.change_reader_disabled");

    let _ = std::fs::remove_dir_all(&temp_home);
}

#[test]
fn require_non_empty_request_field_trims_surrounding_whitespace() {
    let value = require_non_empty_request_field(
        "  session-1 \n",
        "desktop.test_invalid_request",
        "session_id",
    )
    .expect("trimmed field should be accepted");
    assert_eq!(value, "session-1");
}

#[test]
fn require_non_empty_request_field_rejects_blank_values() {
    let error =
        require_non_empty_request_field(" \n\t ", "desktop.test_invalid_request", "session_id")
            .expect_err("blank field should be rejected");
    assert_eq!(error.status, 400);
    assert_eq!(error.code, "desktop.test_invalid_request");
    assert_eq!(error.message, "session_id is required");
}

#[test]
fn desktop_change_reader_qa_respects_toggle() {
    let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
    let temp_home = unique_temp_dir("opensession-desktop-change-reader-qa-disabled");
    let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());

    desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
        session_default_view: None,
        summary: None,
        vector_search: None,
        change_reader: Some(change_reader_update(false, false, None)),
        lifecycle: None,
    })
    .expect("enable change reader with qa disabled");

    let result =
        tauri::async_runtime::block_on(desktop_ask_session_changes(DesktopChangeQuestionRequest {
            session_id: "session-1".to_string(),
            question: "무엇이 바뀌었나요?".to_string(),
            scope: None,
        }));
    let error = result.expect_err("qa disabled should fail");
    assert_eq!(error.status, 422);
    assert_eq!(error.code, "desktop.change_reader_qa_disabled");

    let _ = std::fs::remove_dir_all(&temp_home);
}

#[test]
fn desktop_runtime_settings_rejects_voice_playback_without_api_key() {
    let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
    let temp_home = unique_temp_dir("opensession-desktop-change-reader-voice-key-required");
    let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());

    let result = desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
        session_default_view: None,
        summary: None,
        vector_search: None,
        change_reader: Some(change_reader_update(true, true, None)),
        lifecycle: None,
    });

    let error = result.expect_err("voice playback without api key should fail");
    assert_eq!(error.status, 422);
    assert_eq!(
        error.code,
        "desktop.runtime_settings_change_reader_voice_api_key_required"
    );

    let _ = std::fs::remove_dir_all(&temp_home);
}

#[test]
fn desktop_runtime_settings_allows_voice_playback_with_existing_api_key() {
    let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
    let temp_home = unique_temp_dir("opensession-desktop-change-reader-voice-key-existing");
    let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());

    desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
        session_default_view: None,
        summary: None,
        vector_search: None,
        change_reader: Some(change_reader_update(true, false, Some("sk-existing-voice-key"))),
        lifecycle: None,
    })
    .expect("store existing voice api key");

    let updated = desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
        session_default_view: None,
        summary: None,
        vector_search: None,
        change_reader: Some(change_reader_update(true, true, None)),
        lifecycle: None,
    })
    .expect("enable voice playback with existing api key");

    assert!(updated.change_reader.voice.enabled);
    assert!(updated.change_reader.voice.api_key_configured);

    let _ = std::fs::remove_dir_all(&temp_home);
}

#[test]
fn desktop_change_reader_tts_requires_voice_enable() {
    let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
    let temp_home = unique_temp_dir("opensession-desktop-change-reader-tts-disabled");
    let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());

    desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
        session_default_view: None,
        summary: None,
        vector_search: None,
        change_reader: Some(change_reader_update(true, false, None)),
        lifecycle: None,
    })
    .expect("enable change reader with voice disabled");

    let result = desktop_change_reader_tts(DesktopChangeReaderTtsRequest {
        text: "변경 내용을 읽어줘".to_string(),
        session_id: None,
        scope: None,
    });
    let error = result.expect_err("voice disabled should fail");
    assert_eq!(error.status, 422);
    assert_eq!(error.code, "desktop.change_reader_tts_disabled");

    let _ = std::fs::remove_dir_all(&temp_home);
}
