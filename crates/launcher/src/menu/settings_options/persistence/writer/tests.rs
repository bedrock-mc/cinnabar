use super::*;
use crate::menu::settings_options::SettingId;
use std::sync::mpsc;

const TEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Builds an immutable slider snapshot for mailbox and disk tests.
fn request(value: i32) -> Request {
    let mut settings = SettingsOptions::default();
    settings.set(SettingId::Gamma.index(), value);
    Request {
        path: PathBuf::from(super::super::SETTINGS_FILE),
        settings: Arc::new(settings),
    }
}

#[test]
fn slider_drag_batches_latest_value_without_extending_crash_loss_window() {
    let now = Instant::now();
    let mut state = State::default();
    for value in 1..=100 {
        let edit_at = now + SAVE_DELAY * (value as u32 - 1) / 100;
        state.submit(request(value), edit_at);
        assert!(state.take_due(edit_at).is_none());
    }
    let latest = state.take_due(now + SAVE_DELAY).unwrap();
    assert_eq!(latest.settings.value(SettingId::Gamma), 100);
    state.complete(latest, Ok(()), now + SAVE_DELAY);
    assert!(state.take_due(now + SAVE_DELAY * 2).is_none());
    assert_eq!(state.completed, Some(Ok(())));
}

#[test]
fn retry_keeps_latest_edits_and_does_not_bypass_backoff() {
    let now = Instant::now();
    let mut state = State::default();
    state.submit(request(60), now);
    let failed = state.take_due(now + SAVE_DELAY).unwrap();
    // An edit can arrive during an in-flight write.
    state.submit(request(70), now + SAVE_DELAY);
    state.complete(failed, Err(io::Error::other("read only")), now + SAVE_DELAY);
    state.submit(request(80), now + SAVE_DELAY * 2);
    assert!(state.take_due(now + SAVE_DELAY + RETRY_DELAY / 2).is_none());
    let retry = state.take_due(now + SAVE_DELAY + RETRY_DELAY).unwrap();
    assert_eq!(retry.settings.value(SettingId::Gamma), 80);
    state.complete(retry, Ok(()), now + SAVE_DELAY + RETRY_DELAY);
    assert!(state.pending.is_none());
    assert_eq!(state.completed, Some(Ok(())));
}

#[test]
fn failed_snapshot_is_retained_and_shutdown_bypasses_retry_delay() {
    let now = Instant::now();
    let mut state = State::default();
    state.submit(request(70), now);
    let failed = state.take_due(now + SAVE_DELAY).unwrap();
    state.complete(failed, Err(io::Error::other("read only")), now + SAVE_DELAY);
    assert!(state.take_due(now + SAVE_DELAY).is_none());
    state.stopping = true;
    let retry = state.take_due(now + SAVE_DELAY).unwrap();
    assert_eq!(retry.settings.value(SettingId::Gamma), 70);
    state.complete(retry, Err(io::Error::other("still read only")), now);
    assert!(state.pending.is_none());
    assert!(state.completed.unwrap().is_err());
}

#[test]
fn slow_storage_cannot_block_submission_and_only_latest_pending_edit_is_saved() {
    let caller = thread::current().id();
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (seen_tx, seen_rx) = mpsc::channel();
    let mut first = true;
    let mut writer = Writer::with_saver(move |settings, _| {
        assert_ne!(thread::current().id(), caller);
        seen_tx.send(settings.value(SettingId::Gamma)).unwrap();
        if first {
            first = false;
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        }
        Ok(())
    })
    .unwrap();
    let initial = request(0);
    writer.submit(initial.path, initial.settings).unwrap();
    started_rx.recv_timeout(TEST_TIMEOUT).unwrap();
    // Release the disk even if a regressed submission blocks, so the test can fail cleanly.
    let (returned_tx, returned_rx) = mpsc::channel();
    let releaser = thread::spawn(move || {
        let responsive = returned_rx.recv_timeout(TEST_TIMEOUT).is_ok();
        release_tx.send(()).unwrap();
        responsive
    });
    for value in 1..=100 {
        let edit = request(value);
        writer.submit(edit.path, edit.settings).unwrap();
    }
    returned_tx.send(()).unwrap();
    assert!(releaser.join().unwrap(), "submission waited for the disk");
    writer.finish().unwrap();
    assert_eq!(seen_rx.try_iter().collect::<Vec<_>>(), [0, 100]);
}

#[test]
fn shutdown_saves_queued_edits_with_unchanged_json_and_reports_storage_errors() {
    let layout = crate::test_support::scratch("settings-writer");
    let path = layout.user_config_root.join(super::super::SETTINGS_FILE);
    let latest = request(80);
    let expected = serde_json::to_vec_pretty(latest.settings.as_ref()).unwrap();
    let mut writer = Writer::new().unwrap();
    writer.submit(path.clone(), latest.settings).unwrap();
    drop(writer);
    assert_eq!(std::fs::read(&path).unwrap(), expected);
    assert_eq!(SettingsOptions::load(&path).value(SettingId::Gamma), 80);
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    let mut writer = Writer::new().unwrap();
    writer.submit(path.clone(), request(90).settings).unwrap();
    assert!(writer.finish().is_err());
    assert!(path.is_dir());
    assert!(writer.submit(path, request(100).settings).is_err());
    std::fs::remove_dir_all(layout.user_config_root).unwrap();
}
