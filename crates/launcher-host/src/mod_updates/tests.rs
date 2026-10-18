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
            updates: Vec::new(),
        },
        incoming,
        outgoing,
    )
}

#[test]
fn absent_sources_disabled_checks_and_invalid_versions_never_queue_requests() {
    let (mut checker, incoming, _) = checker();
    checker.configure(&[installed("1.0.0", None)], true);
    checker.configure(
        &[installed(
            "unknown",
            Some("https://github.com/example/zoom"),
        )],
        true,
    );
    checker.configure(
        &[installed("1.0.0", Some("https://github.com/example/zoom"))],
        false,
    );
    assert!(incoming.try_recv().is_err());
}

#[test]
fn disabling_checks_discards_in_flight_notices() {
    let (mut checker, incoming, outgoing) = checker();
    let items = [installed("1.0.0", Some("https://github.com/example/zoom"))];
    checker.configure(&items, true);
    let request = incoming.recv().unwrap();
    checker.configure(&items, false);
    outgoing
        .send(Response {
            generation: request.generation,
            updates: vec![AvailableUpdate {
                id: 1,
                version: "1.1.0".into(),
                release_url: String::new(),
            }],
        })
        .unwrap();
    assert!(checker.updates().is_empty());
    checker.configure(&items, true);
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
                generation: 1,
                candidates: vec![Candidate {
                    id: 3,
                    repository: "example/zoom".into(),
                    version: "1.2.0".into(),
                }],
            })
            .unwrap();
        drop(requests);
        worker::run(incoming, outgoing, &AtomicU64::new(1), |_| {
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
        worker::run(incoming, outgoing, &current, |_| {
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
                generation: revision,
                candidates: vec![Candidate {
                    id: revision,
                    repository: "example/zoom".into(),
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
        version: "1.0.0".into(),
    }];
    candidates.extend((2..=100).map(|id| Candidate {
        id,
        repository: format!("example/mod-{id}"),
        version: "1.0.0".into(),
    }));
    requests
        .send(Request {
            generation: 1,
            candidates,
        })
        .unwrap();
    drop(requests);
    let mut calls = 0;
    worker::run(incoming, outgoing, &AtomicU64::new(1), |repository| {
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
