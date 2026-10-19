//! Recovery through the real registration observer and its bounded admission worker.

use super::*;

/// Waits for a matching worker result while draining earlier registration observations.
fn wait_update(watcher: &Watcher, what: &str, mut matches: impl FnMut(&Update) -> bool) -> Update {
    let mut accepted = None;
    test_time::eventually(what, || {
        while let Ok(update) = watcher.updates.try_recv() {
            if matches(&update) {
                accepted = Some(update);
                return true;
            }
        }
        false
    });
    accepted.unwrap()
}

/// Selects exactly one fixture without changing its grants or reload counter.
fn select_entry(root: &std::path::Path, entry: Entry) {
    fs::write(
        root.join(REGISTRATION_FILE),
        serde_json::to_vec(&Registration {
            version: VERSION,
            request_id: "selected".into(),
            mods: vec![entry],
        })
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn registered_fence_round_trip_restarts_without_a_source_or_registration_edit() {
    let directory = tempfile::tempdir().unwrap();
    let mut prepared = candidate(directory.path(), "drawing.wasm", "Hello");
    let mut entry = prepared.registration.clone();
    entry.component = selection::resolved_component(&entry.component);
    let session = mod_api::drawing::DrawingSession {
        connection: 1,
        ..Default::default()
    };
    prepared
        .frame_host_mut()
        .unwrap()
        .bind_drawing_session(session);
    assert!(
        prepared
            .frame_host_mut()
            .unwrap()
            .reload_if_changed()
            .unwrap()
    );
    let watcher = Watcher::start(directory.path().into()).unwrap();
    watcher.drawing.lock().unwrap().insert(
        entry.component.clone(),
        crate::modding::admission::DrawingContext::capture(prepared.frame_host().unwrap(), None)
            .unwrap(),
    );
    select_entry(directory.path(), entry.clone());
    let update = wait_update(&watcher, "initial drawing admission", |update| {
        !update.changes.is_empty()
    });
    let first = update.changes[0].result.as_ref().unwrap();
    let identity = first.identity;
    watcher.send(Message::Acknowledge {
        generation: update.generation,
        request_id: "fence-admitted".into(),
        installed: BTreeMap::from([(
            entry.component.clone(),
            Admitted {
                source: identity,
                drawing: Some(session),
            },
        )]),
        statuses: Vec::new(),
    });
    test_time::eventually("worker acknowledges the original fence", || {
        fs::read(directory.path().join(STATUS_FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<StatusList>(&bytes).ok())
            .is_some_and(|status| status.request_id == "fence-admitted")
    });
    prepared
        .frame_host_mut()
        .unwrap()
        .bind_drawing_session(mod_api::drawing::DrawingSession {
            dimension: 1,
            ..session
        });
    prepared
        .frame_host_mut()
        .unwrap()
        .bind_drawing_session(session);
    assert!(prepared.frame_host().unwrap().is_revoked());
    watcher.drawing.lock().unwrap().insert(
        entry.component,
        crate::modding::admission::DrawingContext::capture(prepared.frame_host().unwrap(), None)
            .unwrap(),
    );
    let replacement = wait_update(
        &watcher,
        "fence round trip admits a fresh guest",
        |update| !update.changes.is_empty(),
    );
    let fresh = replacement.changes[0].result.as_ref().unwrap();
    assert!(fresh.frame_host().unwrap().is_active());
    assert_eq!(fresh.identity, identity);
    assert_eq!(fresh.frame_host().unwrap().drawing_session(), Some(session));
}

#[test]
fn repaired_disk_settings_retry_a_cached_initial_admission_failure() {
    let directory = tempfile::tempdir().unwrap();
    let prepared = candidate(directory.path(), "settings.wasm", "Hello");
    let mut entry = prepared.registration.clone();
    entry.grants.settings = true;
    let path = mod_host::settings_companion(&entry.component);
    fs::write(&path, [0xff]).unwrap();
    let mut admissions = candidate::Admissions::default();
    assert!(
        admissions
            .prepare(source::source_snapshot(entry.clone()).unwrap(), None)
            .is_err()
    );
    let seed = "{\"repaired\":true}";
    fs::write(path, seed).unwrap();
    let fresh = admissions
        .prepare(source::source_snapshot(entry).unwrap(), None)
        .expect("a repaired settings companion retries the same component");
    assert_eq!(fresh.frame_host().unwrap().settings_snapshot(), Some(seed));
}

#[test]
fn repaired_initial_settings_trigger_admission_without_a_registration_edit() {
    let directory = tempfile::tempdir().unwrap();
    let prepared = candidate(directory.path(), "settings.wasm", "Hello");
    let mut entry = prepared.registration.clone();
    entry.component = selection::resolved_component(&entry.component);
    entry.grants.settings = true;
    let path = mod_host::settings_companion(&entry.component);
    fs::write(&path, [0xff]).unwrap();
    let watcher = Watcher::start(directory.path().into()).unwrap();
    select_entry(directory.path(), entry);
    let rejected = wait_update(&watcher, "initial settings failure", |update| {
        !update.changes.is_empty()
    });
    assert!(rejected.changes[0].result.is_err());
    let seed = "{\"repaired\":true}";
    fs::write(path, seed).unwrap();
    let accepted = wait_update(
        &watcher,
        "repaired settings admit the selected guest",
        |update| !update.changes.is_empty(),
    );
    let fresh = accepted.changes[0].result.as_ref().unwrap();
    assert!(fresh.frame_host().unwrap().is_active());
    assert_eq!(fresh.frame_host().unwrap().settings_snapshot(), Some(seed));
}

#[test]
fn committed_settings_override_an_unreadable_companion_on_retry() {
    let directory = tempfile::tempdir().unwrap();
    let prepared = candidate(directory.path(), "settings.wasm", "Hello");
    let mut entry = prepared.registration.clone();
    entry.grants.settings = true;
    let path = mod_host::settings_companion(&entry.component);
    let seed = "{\"committed\":true}";
    fs::write(&path, seed).unwrap();
    let committed = SettingsSnapshot::read(&entry.component, None).unwrap();
    fs::write(path, [0xff]).unwrap();
    let mut admissions = candidate::Admissions::default();
    assert!(
        admissions
            .prepare(source::source_snapshot(entry.clone()).unwrap(), None)
            .is_err()
    );
    let accepted = admissions
        .prepare(source::source_snapshot(entry).unwrap(), Some(&committed))
        .unwrap();
    assert_eq!(
        accepted.frame_host().unwrap().settings_snapshot(),
        Some(seed)
    );
}

#[test]
fn captured_disk_settings_survive_a_later_companion_edit() {
    let directory = tempfile::tempdir().unwrap();
    let prepared = candidate(directory.path(), "settings.wasm", "Hello");
    let mut entry = prepared.registration.clone();
    entry.grants.settings = true;
    let path = mod_host::settings_companion(&entry.component);
    let seed = "{\"captured\":true}";
    fs::write(&path, seed).unwrap();
    let snapshot = source::source_snapshot(entry).unwrap();
    fs::write(path, [0xff]).unwrap();
    let accepted = build_candidate(snapshot).unwrap();
    assert_eq!(
        accepted.frame_host().unwrap().settings_snapshot(),
        Some(seed)
    );
}

#[test]
fn duplicate_admission_preserves_an_already_current_guests_state() {
    let directory = tempfile::tempdir().unwrap();
    let (mut world, _messages, _retiring) = world();
    let first = candidate(directory.path(), "same.wasm", "Hello");
    let mut entry = first.registration.clone();
    apply(&mut world, vec![entry.clone()], vec![change(first)]);
    {
        let mut runtime = world.resource_mut::<ModRuntime>();
        runtime.hosts.begin_frame();
        runtime.hosts.host_mut(0).frame(true).unwrap();
    }
    assert_eq!(
        world.resource::<ModRuntime>().hosts.host(0).label(),
        Some("ello")
    );
    let duplicate =
        Box::new(build_candidate(source::source_snapshot(entry.clone()).unwrap()).unwrap());
    apply(&mut world, vec![entry.clone()], vec![change(duplicate)]);
    assert_eq!(
        world.resource::<ModRuntime>().hosts.host(0).label(),
        Some("ello")
    );
    entry.reload += 1;
    let explicit =
        Box::new(build_candidate(source::source_snapshot(entry.clone()).unwrap()).unwrap());
    apply(&mut world, vec![entry], vec![change(explicit)]);
    assert_eq!(
        world.resource::<ModRuntime>().hosts.host(0).label(),
        Some("Hello")
    );
}

#[test]
fn unloaded_settings_do_not_override_a_later_disk_seed() {
    let directory = tempfile::tempdir().unwrap();
    let (mut world, _messages, retiring) = world();
    let prepared = candidate(directory.path(), "settings.wasm", "Hello");
    let mut entry = prepared.registration.clone();
    entry.grants.settings = true;
    let path = mod_host::settings_companion(&entry.component);
    fs::write(&path, "{\"previous\":true}").unwrap();
    let first = Box::new(build_candidate(source::source_snapshot(entry.clone()).unwrap()).unwrap());
    apply(&mut world, vec![entry.clone()], vec![change(first)]);
    install::sync_authority(&mut world);
    apply(&mut world, Vec::new(), Vec::new());
    drop(retiring.try_recv().unwrap());
    let seed = "{\"next\":true}";
    fs::write(path, seed).unwrap();
    install::sync_authority(&mut world);
    let current = world
        .resource::<Watcher>()
        .settings
        .lock()
        .unwrap()
        .get(&entry.component)
        .cloned();
    let fresh = candidate::Admissions::default()
        .prepare(source::source_snapshot(entry).unwrap(), current.as_ref())
        .unwrap();
    assert_eq!(fresh.frame_host().unwrap().settings_snapshot(), Some(seed));
}

#[test]
fn changed_committed_settings_retry_a_rejected_guest() {
    let directory = tempfile::tempdir().unwrap();
    let prepared = candidate(directory.path(), "settings.wasm", "Hello");
    let mut entry = prepared.registration.clone();
    entry.component = selection::resolved_component(&entry.component);
    entry.grants.settings = true;
    let broken = fixture(true, "", "Hello").replacen(
        "(func (export \"init\")",
        "(func (export \"init\") unreachable",
        1,
    );
    fs::write(&entry.component, broken).unwrap();
    let watcher = Watcher::start(directory.path().into()).unwrap();
    let mut preferences = SettingsSnapshot {
        path: mod_host::settings_companion(&entry.component),
        json: "{\"value\":1}".into(),
    };
    watcher
        .settings
        .lock()
        .unwrap()
        .insert(entry.component.clone(), preferences.clone());
    select_entry(directory.path(), entry.clone());
    let rejected = wait_update(
        &watcher,
        "rejected guest with committed preferences",
        |update| !update.changes.is_empty(),
    );
    assert!(rejected.changes[0].result.is_err());
    preferences.json = "{\"value\":2}".into();
    watcher
        .settings
        .lock()
        .unwrap()
        .insert(entry.component, preferences);
    let retried = wait_update(
        &watcher,
        "changed preferences retry rejected admission",
        |update| !update.changes.is_empty(),
    );
    assert!(retried.generation > rejected.generation);
    assert!(retried.changes[0].result.is_err());
}

#[test]
fn delayed_admission_keeps_the_observed_settings_input() {
    let directory = tempfile::tempdir().unwrap();
    let prepared = candidate(directory.path(), "settings.wasm", "Hello");
    let mut entry = prepared.registration.clone();
    entry.grants.settings = true;
    let path = mod_host::settings_companion(&entry.component);
    let seed = "{\"observed\":true}";
    fs::write(&path, seed).unwrap();
    let snapshot = source::source_snapshot(entry).unwrap();
    fs::write(path, [0xff]).unwrap();
    let accepted = candidate::Admissions::default()
        .prepare(snapshot, None)
        .expect("admission uses the exact observed seed");
    assert_eq!(
        accepted.frame_host().unwrap().settings_snapshot(),
        Some(seed)
    );
}

#[test]
fn delayed_worker_reobserves_changed_preferences_before_admission() {
    for committed in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let prepared = candidate(directory.path(), "settings.wasm", "Hello");
        let mut entry = prepared.registration.clone();
        entry.grants.settings = true;
        let path = mod_host::settings_companion(&entry.component);
        let seed = "{\"observed\":true}";
        fs::write(&path, seed).unwrap();
        let snapshot = source::source_snapshot(entry.clone()).unwrap();
        let request = |snapshot| LoadRequest {
            generation: 1,
            registration: Registration {
                version: VERSION,
                request_id: "same-selection".into(),
                mods: vec![entry.clone()],
            },
            sources: vec![(entry.clone(), Ok(snapshot))],
        };
        let mut latest = BTreeMap::new();
        if committed {
            let mut current = SettingsSnapshot::read(&entry.component, None).unwrap();
            current.json = "{\"observed\":false}".into();
            latest.insert(entry.component.clone(), current);
        } else {
            fs::write(&path, [0xff]).unwrap();
        }
        let settings = Mutex::new(latest);
        let mut admissions = candidate::Admissions::default();
        assert!(matches!(
            worker::load_request(request(snapshot), &settings, &mut admissions),
            worker::LoadResult::Reobserve(1)
        ));
        // A returns before the next observation, without editing the source or registration.
        settings.lock().unwrap().clear();
        fs::write(&path, seed).unwrap();
        let snapshot = source::source_snapshot(entry.clone()).unwrap();
        let worker::LoadResult::Ready(accepted) =
            worker::load_request(request(snapshot), &settings, &mut admissions)
        else {
            panic!("restored preferences admit the original selection");
        };
        let fresh = accepted.changes[0].result.as_ref().unwrap();
        assert_eq!(fresh.frame_host().unwrap().settings_snapshot(), Some(seed));
    }
}
