use super::*;

fn service(root: &Path, current: Build) -> Service {
    Service::new(
        Config {
            update_dir: root.to_owned(),
            current,
            target: None,
            helper: root.join("missing-helper"),
        },
        || None,
        |_| {},
    )
}

fn build(version: &str, number: u64) -> Build {
    Build {
        version: version.into(),
        number,
    }
}

fn offer() -> Offer {
    Offer {
        version: "2.0.0".into(),
        build: 2,
        channel: Channel::Beta,
        notes_url: None,
        size: 10,
        downgrade: false,
    }
}

#[test]
fn installing_waits_for_every_running_game_process() {
    let root = tempfile::tempdir().unwrap();
    let service = service(root.path(), build("1.0.0", 1));
    let generation = service.machine().begin_check().unwrap();
    service
        .machine()
        .finish_check(generation, Ok(Some(offer())), "t");
    let game = runtime::Session::try_join(root.path()).unwrap();
    let error = service
        .install(|| OffsetDateTime::UNIX_EPOCH)
        .err()
        .unwrap();
    assert!(error.is::<Live>());
    assert!(matches!(
        service.state(),
        UpdateState::Failed {
            retryable: true,
            can_play_installed: true,
            ..
        }
    ));
    drop(game);
    assert!(!runtime::is_live(root.path()));
}

#[test]
fn an_unfinished_handoff_is_reported_once_and_a_finished_one_is_not() {
    let root = tempfile::tempdir().unwrap();
    write_pending(root.path(), &offer()).unwrap();
    let after_failure = service(root.path(), build("1.0.0", 1));
    assert!(matches!(after_failure.state(), UpdateState::Failed { .. }));
    assert_eq!(
        service(root.path(), build("1.0.0", 1)).state(),
        UpdateState::Idle { last_checked: None }
    );
    write_pending(root.path(), &offer()).unwrap();
    assert_eq!(
        service(root.path(), build("2.0.0", 2)).state(),
        UpdateState::Idle { last_checked: None }
    );
}

#[test]
fn channel_choices_persist_and_retire_the_previous_offer() {
    let root = tempfile::tempdir().unwrap();
    let service = service(root.path(), build("1.0.0", 1));
    assert_eq!(service.channel(), Channel::default());
    let generation = service.machine().begin_check().unwrap();
    service
        .machine()
        .finish_check(generation, Ok(Some(offer())), "t");
    service.set_channel(Channel::Nightly).unwrap();
    assert_eq!(service.state(), UpdateState::Idle { last_checked: None });
    assert_eq!(
        preferences::load(root.path(), || panic!("already saved")),
        Channel::Nightly
    );
    assert!(service.set_channel(Channel::Stable).is_err() || Channel::Stable.is_published());
}

#[test]
fn a_build_without_update_keys_is_unavailable_not_failed() {
    let root = tempfile::tempdir().unwrap();
    let mut service = service(root.path(), build("1.0.0", 1));
    service.keys = TrustedKeys::new();
    service.check(OffsetDateTime::UNIX_EPOCH);
    assert!(matches!(service.state(), UpdateState::Unavailable { .. }));
}
