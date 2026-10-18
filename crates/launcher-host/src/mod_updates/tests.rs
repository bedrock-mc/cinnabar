use super::worker::Release;
use super::*;

/// A GitHub response with a deliberately unrelated HTML URL; links must use the declared repo.
fn release(tag: &str, draft: bool, prerelease: bool) -> Release {
    serde_json::from_value(serde_json::json!({
        "tag_name": tag, "draft": draft, "prerelease": prerelease,
        "html_url": "https://unrelated.example/download"
    }))
    .unwrap()
}

/// An installed component with an optional author-provided update source.
fn installed(version: &str, repository: Option<&str>) -> InstalledMod {
    let mut item = InstalledMod {
        id: 1,
        file: "zoom.wasm".into(),
        bytes: 1,
        permissions: Vec::new(),
        metadata: Default::default(),
    };
    item.metadata.version = Some(version.into());
    item.metadata.update_repository = repository.and_then(|repository| repository.parse().ok());
    item
}

/// A checker using only channels, with no network worker.
fn checker() -> (Checker, Receiver<Request>, Sender<Response>) {
    let (requests, incoming) = unbounded();
    let (outgoing, responses) = unbounded();
    (
        Checker {
            requests,
            responses,
            generation: Arc::new(AtomicU64::new(0)),
            candidates: Vec::new(),
            permission: Arc::new(AtomicU64::new(0)),
            automatic: false,
            enabled: false,
            sources: HashMap::new(),
            progress: progress::Progress::default(),
            pending: None,
            message: None,
            updates: Vec::new(),
            downloads: Vec::new(),
        },
        incoming,
        outgoing,
    )
}

#[test]
fn absent_sources_disabled_checks_and_invalid_versions_never_queue_requests() {
    let (mut checker, incoming, _) = checker();
    checker.configure(&[installed("1.0.0", None)], true, true);
    checker.configure(
        &[installed(
            "unknown",
            Some("https://github.com/example/zoom"),
        )],
        true,
        true,
    );
    checker.configure(
        &[installed("1.0.0", Some("https://github.com/example/zoom"))],
        false,
        true,
    );
    assert!(incoming.try_recv().is_err());
}

#[test]
fn disabling_checks_discards_in_flight_notices() {
    let (mut checker, incoming, outgoing) = checker();
    let items = [installed("1.0.0", Some("https://github.com/example/zoom"))];
    checker.configure(&items, true, true);
    let request = incoming.recv().unwrap();
    checker.configure(&items, false, true);
    outgoing
        .send(Response {
            manual: None,
            message: None,
            generation: request.generation,
            downloads: Vec::new(),
            updates: vec![AvailableUpdate {
                id: 1,
                version: "1.1.0".into(),
                release_url: String::new(),
            }],
        })
        .unwrap();
    assert!(checker.updates().is_empty());
    checker.configure(&items, true, true);
    let _ = incoming.recv().unwrap(); // Disable snapshot.
    let request = incoming.recv().unwrap();
    assert_ne!(request.generation, 1);
}

#[test]
fn only_newer_published_semantic_versions_offer_updates() {
    for (tag, draft, prerelease, expected) in [
        ("v1.10.0", false, false, true),
        ("1.2.0", false, false, false),
        ("1.1.9", false, false, false),
        ("1.2.0+rebuild", false, false, false),
        ("v2.0.0", true, false, false),
        ("v2.0.0-beta.1", false, true, false),
        ("v2.0.0-beta.1", false, false, false),
        ("nightly", false, false, false),
    ] {
        let (requests, incoming) = unbounded();
        let (outgoing, responses) = unbounded();
        requests
            .send(Request {
                manual: None,
                generation: 1,
                permission: 0,
                candidates: vec![Candidate {
                    id: 3,
                    repository: "example/zoom".into(),
                    asset: None,
                    version: "1.2.0".into(),
                }],
            })
            .unwrap();
        drop(requests);
        run(incoming, outgoing, &AtomicU64::new(1), |_| {
            Ok(Some(release(tag, draft, prerelease)))
        });
        let response = responses.recv().unwrap();
        assert_eq!(!response.updates.is_empty(), expected, "{tag}");
        if expected {
            assert_eq!(
                response.updates[0].release_url,
                format!("https://github.com/example/zoom/releases/tag/{tag}")
            );
        }
    }
}

#[test]
fn repository_checks_are_cached_across_mod_versions_and_library_changes() {
    let (requests, incoming) = unbounded();
    let (outgoing, responses) = unbounded();
    let generation = Arc::new(AtomicU64::new(1));
    let current = generation.clone();
    let worker = std::thread::spawn(move || {
        let mut calls = 0;
        run(incoming, outgoing, &current, |_| {
            calls += 1;
            Ok(Some(release("v2.0.0", false, false)))
        });
        calls
    });
    for (index, version) in ["1.0.0", "2.0.0"].into_iter().enumerate() {
        let revision = index as u64 + 1;
        generation.store(revision, Ordering::SeqCst);
        requests
            .send(Request {
                manual: None,
                generation: revision,
                permission: 0,
                candidates: vec![Candidate {
                    id: revision,
                    repository: "example/zoom".into(),
                    asset: None,
                    version: version.into(),
                }],
            })
            .unwrap();
        let response = responses.recv().unwrap();
        assert_eq!(response.updates.len(), usize::from(index == 0));
    }
    drop(requests);
    assert_eq!(worker.join().unwrap(), 1);
}

#[test]
fn unavailable_repositories_do_not_block_other_mods_and_network_work_is_bounded() {
    let (requests, incoming) = unbounded();
    let (outgoing, responses) = unbounded();
    let mut candidates = vec![Candidate {
        id: 1,
        repository: "example/unavailable".into(),
        asset: None,
        version: "1.0.0".into(),
    }];
    candidates.extend((2..=100).map(|id| Candidate {
        id,
        repository: format!("example/mod-{id}"),
        asset: None,
        version: "1.0.0".into(),
    }));
    requests
        .send(Request {
            manual: None,
            generation: 1,
            permission: 0,
            candidates,
        })
        .unwrap();
    drop(requests);
    let mut calls = 0;
    run(incoming, outgoing, &AtomicU64::new(1), |repository| {
        calls += 1;
        if repository == "example/unavailable" {
            anyhow::bail!("offline repository");
        }
        Ok(Some(release("2.0.0", false, false)))
    });
    let updates = responses.recv().unwrap().updates;
    assert!(calls < 100);
    assert_eq!(updates.len(), calls - 1);
    assert!(updates.iter().all(|update| update.id != 1));
}

/// Runs release-only fixtures while asserting no automatic download is selected.
fn run(
    incoming: Receiver<Request>,
    outgoing: Sender<Response>,
    generation: &AtomicU64,
    fetch: impl FnMut(&str) -> anyhow::Result<Option<Release>>,
) {
    worker::run(incoming, outgoing, generation, fetch, |_, _, _, _| {
        panic!("unexpected download")
    });
}

#[test]
fn direct_asset_opt_in_and_player_preference_control_automatic_downloads() {
    let (mut checker, incoming, _) = checker();
    let mut item = installed("1.0.0", None);
    item.metadata.update_url = Some(
        "https://github.com/example/zoom/releases/latest/download/zoom.wasm"
            .parse()
            .unwrap(),
    );
    checker.configure(&[item.clone()], true, true);
    let automatic = incoming.recv().unwrap();
    assert_eq!(automatic.candidates[0].asset.as_deref(), Some("zoom.wasm"));
    assert_eq!(automatic.candidates[0].repository, "example/zoom");
    let permission = checker.permission.clone();
    let mut neighbor_changed = item.clone();
    neighbor_changed.id += 1;
    checker.configure(&[item.clone(), neighbor_changed], true, true);
    let _ = incoming.recv().unwrap();
    assert_eq!(permission.load(Ordering::SeqCst), automatic.permission);
    checker.configure(&[item.clone()], true, false);
    let manual = incoming.recv().unwrap();
    assert!(manual.candidates[0].asset.is_none());
    assert_ne!(permission.load(Ordering::SeqCst), automatic.permission);
    checker.configure(&[item], false, true);
    assert!(incoming.recv().unwrap().candidates.is_empty());
}

#[test]
fn unchanged_sources_are_downloaded_once_per_launch_and_stale_transfers_can_retry() {
    let (requests, incoming) = unbounded();
    let (outgoing, responses) = unbounded();
    let generation = Arc::new(AtomicU64::new(1));
    let current = generation.clone();
    let worker = std::thread::spawn(move || {
        let mut calls = 0;
        worker::run(
            incoming,
            outgoing,
            &current,
            |_| Ok(Some(release("1.1.0", false, false))),
            |_, _, revision, _| {
                calls += 1;
                if revision == 1 {
                    current.store(2, Ordering::SeqCst);
                }
                None
            },
        );
        calls
    });
    for revision in [1, 2, 3] {
        generation.store(revision, Ordering::SeqCst);
        requests
            .send(Request {
                manual: None,
                generation: revision,
                permission: 0,
                candidates: vec![Candidate {
                    id: 1,
                    repository: "example/zoom".into(),
                    version: "1.0.0".into(),
                    asset: Some("zoom.wasm".into()),
                }],
            })
            .unwrap();
        responses.recv().unwrap();
    }
    drop(requests);
    assert_eq!(worker.join().unwrap(), 2);
}

/// A detected stable release for a mod with an exact download URL and auto-install disabled.
fn manual_checker() -> (Checker, Receiver<Request>, Sender<Response>) {
    let (mut checker, incoming, outgoing) = checker();
    let mut item = installed("1.0.0", None);
    item.metadata.update_url = Some(
        "https://github.com/example/zoom/releases/latest/download/zoom.wasm"
            .parse()
            .unwrap(),
    );
    checker.configure(&[item], true, false);
    let request = incoming.recv().unwrap();
    assert!(request.candidates[0].asset.is_none());
    checker.updates = vec![AvailableUpdate {
        id: 1,
        version: "1.1.0".into(),
        release_url: String::new(),
    }];
    (checker, incoming, outgoing)
}

#[test]
fn update_click_downloads_the_exact_asset_without_enabling_automatic_installation() {
    let (mut checker, incoming, outgoing) = manual_checker();
    assert!(!checker.install(99));
    assert!(checker.install(1));
    assert!(!checker.install(1));
    assert_eq!(checker.pending(), Some(1));
    assert!(!checker.automatic);
    assert!(checker.candidates[0].asset.is_none());
    let request = incoming.recv().unwrap();
    assert_eq!(request.manual, Some(1));
    assert_eq!(request.candidates[0].asset.as_deref(), Some("zoom.wasm"));
    outgoing
        .send(Response {
            manual: Some(1),
            message: None,
            generation: request.generation,
            updates: checker.updates.clone(),
            downloads: vec![Downloaded::fixture(
                1,
                "1.1.0",
                "example/zoom",
                "zoom.wasm",
                b"downloaded component",
            )],
        })
        .unwrap();
    checker.updates();
    assert_eq!(checker.pending(), None);
    assert_eq!(checker.take_downloads().len(), 1);
}

#[test]
fn failed_manual_updates_allow_retry_and_stale_replies_cannot_finish_the_new_request() {
    let (mut checker, incoming, outgoing) = manual_checker();
    assert!(checker.install(1));
    let first = incoming.recv().unwrap();
    outgoing
        .send(Response {
            manual: Some(1),
            message: Some("Download failed".into()),
            generation: first.generation,
            updates: checker.updates.clone(),
            downloads: Vec::new(),
        })
        .unwrap();
    checker.updates();
    assert_eq!(checker.pending(), None);
    assert_eq!(checker.take_message().as_deref(), Some("Download failed"));
    assert!(checker.install(1));
    let _ = incoming.recv().unwrap();
    outgoing
        .send(Response {
            manual: Some(1),
            message: Some("old failure".into()),
            generation: first.generation,
            updates: Vec::new(),
            downloads: Vec::new(),
        })
        .unwrap();
    checker.updates();
    assert_eq!(checker.pending(), Some(1));
    assert!(checker.take_message().is_none());
    let authorization = checker.permission.load(Ordering::SeqCst);
    checker.configure(&[], false, false);
    assert_ne!(checker.permission.load(Ordering::SeqCst), authorization);
    assert_eq!(checker.pending(), None);
    assert!(!checker.install(1));
}

#[test]
fn an_update_click_retries_a_failed_automatic_download_without_rechecking_the_repository() {
    let (requests, incoming) = unbounded();
    let (outgoing, responses) = unbounded();
    let generation = Arc::new(AtomicU64::new(1));
    let current = generation.clone();
    let worker = std::thread::spawn(move || {
        let mut fetches = 0;
        let mut downloads = 0;
        worker::run(
            incoming,
            outgoing,
            &current,
            |_| {
                fetches += 1;
                Ok(Some(release("1.1.0", false, false)))
            },
            |_, _, _, _| {
                downloads += 1;
                None
            },
        );
        (fetches, downloads)
    });
    for (revision, manual) in [(1, None), (2, Some(1)), (3, None)] {
        generation.store(revision, Ordering::SeqCst);
        requests
            .send(Request {
                manual,
                generation: revision,
                permission: 0,
                candidates: vec![Candidate {
                    id: 1,
                    repository: "example/zoom".into(),
                    version: "1.0.0".into(),
                    asset: Some("zoom.wasm".into()),
                }],
            })
            .unwrap();
        let response = responses.recv().unwrap();
        assert_eq!(response.message.is_some(), manual.is_some());
    }
    drop(requests);
    assert_eq!(worker.join().unwrap(), (1, 2));
}

#[test]
fn completed_download_is_handed_to_the_installer_without_a_spurious_failure() {
    let (requests, incoming) = unbounded();
    let (outgoing, responses) = unbounded();
    requests
        .send(Request {
            manual: Some(1),
            generation: 1,
            permission: 0,
            candidates: vec![Candidate {
                id: 1,
                repository: "example/zoom".into(),
                version: "1.0.0".into(),
                asset: Some("zoom.wasm".into()),
            }],
        })
        .unwrap();
    drop(requests);
    worker::run(
        incoming,
        outgoing,
        &AtomicU64::new(1),
        |_| Ok(Some(release("1.1.0", false, false))),
        |_, _, _, _| {
            Some(Downloaded::fixture(
                1,
                "1.1.0",
                "example/zoom",
                "zoom.wasm",
                b"component",
            ))
        },
    );
    let first = responses.recv().unwrap();
    assert_eq!(first.manual, Some(1));
    assert!(first.message.is_none());
    assert_eq!(first.downloads.len(), 1);
    let final_response = responses.recv().unwrap();
    assert!(final_response.manual.is_none() && final_response.message.is_none());
    assert!(final_response.downloads.is_empty());
}

#[test]
fn explicit_update_downloads_after_the_automatic_work_budget_is_exhausted() {
    let limit = worker::MAX_REPOSITORIES;
    let (requests, incoming) = unbounded();
    let (outgoing, responses) = unbounded();
    let generation = Arc::new(AtomicU64::new(1));
    let current = generation.clone();
    let worker = std::thread::spawn(move || {
        let mut attempts = 0;
        worker::run(
            incoming,
            outgoing,
            &current,
            |_| Ok(Some(release("1.1.0", false, false))),
            |candidate, _, _, _| {
                attempts += 1;
                (attempts > limit).then(|| {
                    Downloaded::fixture(
                        candidate.id,
                        "1.1.0",
                        "example/zoom",
                        "zoom.wasm",
                        b"component",
                    )
                })
            },
        );
        attempts
    });
    let candidate = |id| Candidate {
        id,
        repository: "example/zoom".into(),
        version: "1.0.0".into(),
        asset: Some("zoom.wasm".into()),
    };
    requests
        .send(Request {
            manual: None,
            generation: 1,
            permission: 0,
            candidates: (1..=limit as u64).map(candidate).collect(),
        })
        .unwrap();
    responses.recv().unwrap();
    generation.store(2, Ordering::SeqCst);
    requests
        .send(Request {
            manual: Some(1),
            generation: 2,
            permission: 0,
            candidates: vec![candidate(1)],
        })
        .unwrap();
    drop(requests);
    assert_eq!(worker.join().unwrap(), limit + 1);
    let manual = responses.recv().unwrap();
    assert_eq!(manual.manual, Some(1));
    assert_eq!(manual.downloads.len(), 1);
    assert!(manual.message.is_none());
}
