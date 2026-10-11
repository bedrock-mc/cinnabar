use {super::*, launcher::menu::auth::AuthState};

#[test]
fn review_ui_account_changes_wake_catalog_without_repeated_poll_wakes() {
    let (wake, changes) = bounded(1);
    let mut snapshot = Snapshot {
        catalog_wake: Some(wake),
        ..Default::default()
    };
    let account = Account {
        state: CoreAuth::SignedIn,
        gamertag: Some("Alex".into()),
        verification_uri: None,
        user_code: None,
        reason: None,
    };
    snapshot.set_account(account.clone());
    assert_eq!(
        changes.try_recv(),
        Ok(()),
        "the first account must wake catalogs"
    );
    snapshot.set_account(account.clone());
    assert_eq!(
        changes.try_recv(),
        Err(crossbeam_channel::TryRecvError::Empty)
    );
    snapshot.set_account(Account {
        gamertag: Some("Steve".into()),
        ..account
    });
    assert_eq!(
        changes.try_recv(),
        Ok(()),
        "another identity must wake catalogs"
    );
    snapshot.retire_account_data();
    assert_eq!(changes.try_recv(), Ok(()), "sign-out must wake catalogs");
}

#[test]
fn review_ui_catalog_wait_handles_wakes_and_shutdown() {
    let (alive, stop) = bounded(0);
    let (wake, changes) = bounded(1);
    wake.try_send(()).unwrap();
    assert!(wait_catalog(&stop, &changes));
    drop(alive);
    assert!(!wait_catalog(&stop, &changes));
}

// A pinged server that sent no pong reads offline instead of loading.
#[test]
fn a_round_answers_for_every_target() {
    let pong = ServerPing {
        address: "a:1".to_owned(),
        online: true,
        ping_ms: 40,
        ..ServerPing::default()
    };
    let targets = ["a:1".to_owned(), "b:2".to_owned()];
    let round = round_results(&targets, vec![pong.clone()]);
    assert_eq!(round[0], pong);
    assert_eq!(round[1].address, "b:2");
    assert!(!round[1].online);
    assert!(
        round_results(&targets, Vec::new())
            .iter()
            .all(|ping| !ping.online)
    );
}

#[test]
fn tile_images_sort_into_button_layers() {
    let image = |id: &str| bridge::MessageImage {
        id: id.into(),
        url: String::new(),
        path: format!("/art/{id}.img"),
    };
    let message = Message {
        surface: "PlayButton".into(),
        banner: "New".into(),
        images: vec![
            image("background"),
            image("hoverForeground"),
            image("hover"),
        ],
        ..Message::default()
    };
    let home = Home {
        messages: vec![message],
        realm_invites: 2,
        ..Home::default()
    };
    let menu = menu_home(&home, 0);
    let art = menu.play_art.expect("play art");
    assert_eq!(art.default_background, "/art/background.img");
    assert_eq!(art.hover_foreground, "/art/hoverForeground.img");
    assert_eq!(art.hover_background, "/art/hover.img");
    assert_eq!(art.banner, "New");
    assert!(menu.store_art.is_none());
    assert_eq!(menu.realm_invites, 2);
}

#[test]
fn featured_servers_split_into_cards_and_details() {
    let server = FeaturedServer {
        name: "S".into(),
        player_count: Some(12_345),
        address: "a.test:19132".into(),
        news: "Update".into(),
        background: bridge::Artwork {
            url: "https://a.test/bg.png".into(),
            path: "/art/bg.img".into(),
        },
        screenshots: vec![
            bridge::Artwork {
                url: "https://a.test/s.png".into(),
                path: String::new(),
            },
            bridge::Artwork {
                url: "https://a.test/t.png".into(),
                path: "/art/t.img".into(),
            },
        ],
        ..FeaturedServer::default()
    };
    let (card, details) = featured_card(&server);
    assert_eq!(card.address, "a.test:19132");
    assert_eq!(details.news, "Update");
    assert_eq!(details.screenshots, vec!["/art/t.img".to_owned()]);
    assert_eq!(details.banner, "/art/bg.img");
    assert_eq!(details.player_count, Some(12_345));
}

#[test]
fn core_connect_stages_map_to_join_stages() {
    let progress = |stage| ConnectProgress {
        stage,
        packs_done: 1,
        packs_total: 2,
        received_bytes: 3,
        total_bytes: 4,
    };
    assert_eq!(join_stage(&progress(ConnectStage::Realm)), JoinStage::Realm);
    assert_eq!(
        join_stage(&progress(ConnectStage::Connecting)),
        JoinStage::Connecting
    );
    assert_eq!(
        join_stage(&progress(ConnectStage::Packs)),
        JoinStage::Packs {
            done: 1,
            total: 2,
            received_bytes: 3,
            total_bytes: 4
        }
    );
}

#[test]
fn core_account_states_map_to_menu_sign_in_states() {
    let account = |state| Account {
        state,
        verification_uri: Some("https://aka.ms/remoteconnect".into()),
        user_code: Some("ABCD".into()),
        gamertag: None,
        reason: Some("expired".into()),
    };
    assert_eq!(auth_state(&account(CoreAuth::Offline)), None);
    assert_eq!(
        auth_state(&account(CoreAuth::SignedOut)),
        Some(AuthState::SignedOut)
    );
    assert_eq!(
        auth_state(&account(CoreAuth::AwaitingCode)),
        Some(AuthState::AwaitingCode {
            uri: "https://aka.ms/remoteconnect".into(),
            code: "ABCD".into()
        })
    );
    assert_eq!(
        auth_state(&account(CoreAuth::SignedIn)),
        Some(AuthState::Authenticated)
    );
    assert_eq!(
        auth_state(&account(CoreAuth::Failed)),
        Some(AuthState::Failed("expired".into()))
    );
}
#[test]
fn duplicate_ping_targets_keep_the_same_online_result() {
    let targets = vec!["server.test".into(), "server.test".into()];
    let results = round_results(
        &targets,
        vec![ServerPing {
            address: targets[0].clone(),
            online: true,
            ..Default::default()
        }],
    );
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|ping| ping.online));
}
#[test]
fn account_responses_from_before_sign_out_are_discarded() {
    let snapshot = Arc::new(Mutex::new(Snapshot::default()));
    let generation = auth_generation(&snapshot);
    let (sign_out, _requests) = bounded(1);
    let (alive, _stop) = bounded(0);
    let mut account = LauncherAccount {
        snapshot: Arc::clone(&snapshot),
        sign_out,
        profile_refresh: crossbeam_channel::bounded(1).0,
        _alive: alive,
        socket_dir: PathBuf::new(),
        message_reports: crossbeam_channel::unbounded().0,
        invites: crossbeam_channel::unbounded().0,
        realm_membership: tokio::sync::mpsc::unbounded_channel().0,
    };
    assert!(account.sign_out());
    publish_account(&snapshot, generation, |snapshot| {
        snapshot.realms = Some(Vec::new());
        snapshot.friends = Some(Vec::new());
    });
    let retained = snapshot.lock().unwrap();
    assert!(retained.realms.is_none() && retained.friends.is_none());
}
