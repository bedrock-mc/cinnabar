use super::*;

/// The actual Go discovery fixture checks rank presence, malformed neighbors and artwork roles.
#[test]
fn native_featured_projection_matches_the_existing_go_catalog_fixture() {
    let items = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../core/catalog/testdata/featured_projection.json"
    )))
    .unwrap();
    let rows = projection::rows(items);
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows.iter()
            .map(|row| row.0.name.as_str())
            .collect::<Vec<_>>(),
        ["Ranked", "Zero", "Creator"]
    );
    assert_eq!(rows[1].0.group, "featured", "zero is an explicit rank");
    let creator = &rows[2];
    assert_eq!(creator.0.group, "creator");
    assert_eq!(creator.0.address, "play.example.test:19132");
    assert!(creator.1.is_none(), "a direct row allocated an experience");
    assert_eq!(creator.0.caption, "");
    assert_eq!(creator.0.description, "Full description");
    assert_eq!(creator.0.news, "News body");
    assert_eq!(creator.0.logo.url, "https://cdn.example.test/icon.png");
    assert_eq!(
        creator.0.background.url,
        "https://cdn.example.test/banner.png"
    );
    assert_eq!(
        creator.0.games[0].image.url,
        "https://cdn.example.test/base.png"
    );
    let id = rows[0].1.unwrap();
    assert_eq!(
        rows[0].0.address,
        format!("{}{id}", bridge::contract::GATHERING_TARGET_PREFIX)
    );
}

/// First unavailable and negative populations stay unavailable instead of becoming zero or a later duplicate.
#[test]
fn native_featured_counts_preserve_first_missing_population_and_direct_rows() {
    let id = Uuid::from_u128(1);
    let second = Uuid::from_u128(2);
    let mut servers = vec![
        bridge::FeaturedServer {
            address: format!("{}{id}", bridge::contract::GATHERING_TARGET_PREFIX),
            ..Default::default()
        },
        bridge::FeaturedServer {
            address: format!("{}{second}", bridge::contract::GATHERING_TARGET_PREFIX),
            ..Default::default()
        },
        bridge::FeaturedServer {
            address: "direct.test:19132".into(),
            player_count: Some(42),
            ..Default::default()
        },
    ];
    projection::counts(
        &mut servers,
        &[
            ExperiencePlayerCount {
                experience_id: id,
                player_count: None,
            },
            ExperiencePlayerCount {
                experience_id: id,
                player_count: Some(9),
            },
            ExperiencePlayerCount {
                experience_id: second,
                player_count: Some(-1),
            },
        ],
    );
    assert!(servers.iter().all(|server| server.player_count.is_none()));
    projection::counts(
        &mut servers,
        &[ExperiencePlayerCount {
            experience_id: id,
            player_count: Some(0),
        }],
    );
    assert_eq!(servers[0].player_count, Some(0));
    assert_eq!(servers[1].player_count, None);
    assert_eq!(servers[2].player_count, None);
}

/// Credential contention cannot cause a selection to mint under another account or lose the held lease.
#[tokio::test]
async fn native_gathering_selection_rejects_a_replaced_generation_before_allocating() {
    let (_directory, account) = super::super::profile::tests::account();
    let operation = Arc::clone(&account).acquire_verified().await.unwrap();
    let mut selection = NativeGatheringSelection {
        identity: ReadIdentity::capture(&operation),
        experience: Uuid::from_u128(1),
    };
    selection.identity.generation.push_str("-obsolete");
    drop(operation);
    let result = selection
        .prepare(
            serde_json::json!({}),
            protocol::ClientRandomId::new(7).unwrap(),
        )
        .await;
    assert_eq!(
        result.err().unwrap().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert!(!account.state.lock().unwrap().busy);
    let _lease = account.store.try_native_lease().unwrap();
}
