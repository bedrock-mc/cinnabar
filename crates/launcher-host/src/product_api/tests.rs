use super::*;
use policy::*;
use std::time::Duration;

mod connection;

/// The native policy follows the same pinned product schema and existing generated fallback manifest.
#[test]
fn operating_limits_and_validation_match_the_pinned_product_contract() {
    let schema: Value =
        serde_json::from_str(include_str!("../../../../core/cinnabarapi/schema/v1.json")).unwrap();
    let defs = &schema["$defs"];
    let welcome = &defs["Welcome"]["properties"];
    let p = Parameters::default();
    assert_eq!(welcome["lookup_batch_max"]["default"], p.batch);
    assert_eq!(welcome["lookup_bucket_capacity"]["default"], p.capacity);
    assert_eq!(welcome["lookup_refill_per_min"]["default"], p.refill);
    assert_eq!(
        welcome["cache_ttl_ms"]["default"],
        p.cache_ttl.as_millis() as u64
    );
    assert_eq!(
        welcome["ping_interval_ms"]["default"],
        p.ping.as_millis() as u64
    );
    assert_eq!(
        welcome["reconnect_max_ms"]["default"],
        p.reconnect.as_millis() as u64
    );
    assert_eq!(defs["Build"]["maximum"], MAX_BUILD);
    assert_eq!(
        defs["Emote"]["properties"]["valid_for_ms"]["maximum"],
        EMOTE_TTL.as_millis() as u64
    );
    assert_eq!(defs["CosmeticSlots"]["maxProperties"], 16);
    assert_eq!(defs["CosmeticSlot"]["maxLength"], 48);
    assert_eq!(defs["CosmeticItemId"]["maxLength"], 128);
    assert_eq!(welcome["owned_cosmetics"]["maxItems"], cosmetics::MAX_ITEMS);
    assert_eq!(
        defs["CosmeticOwnership"]["properties"]["owned_cosmetics"]["maxItems"],
        cosmetics::MAX_ITEMS
    );
    assert_eq!(defs["Version"]["maxLength"], 64);
    let channels = defs["Channel"]["enum"].as_array().unwrap();
    assert_eq!(channels.len(), update_manifest::channel::Channel::ALL.len());
    assert!(
        update_manifest::channel::Channel::ALL
            .into_iter()
            .all(|channel| channels.contains(&json!(channel.name())))
    );
    assert_eq!(
        defs["CosmeticSlot"]["pattern"],
        "^([a-z0-9_.-]+:)?[a-z0-9_.-]+$"
    );
    assert_eq!(
        defs["CosmeticItemId"]["pattern"],
        "^[a-z0-9_.-]+:[a-z0-9_./-]+$"
    );
    assert_eq!(defs["EmoteId"]["maxLength"], 128);
    assert!(valid_config(&config()));
    for version in ["1.0", "1.0.0\r\nInjected: value", "1.0.0-", "v1.0.0"] {
        let mut c = config();
        c.client_version = version.into();
        assert!(!valid_config(&c));
    }
    for id in ["bad clip", "UPPER:id", "a/b", "", &"x".repeat(129)] {
        assert!(!valid_emote(id));
    }
    assert!(valid_emote("cinnabar:some-clip.v1"));
    let mut slots = vec![CinnabarCosmeticSlot {
        slot: "head".into(),
        item_id: "cinnabar:item/path".into(),
    }];
    assert!(valid_slots(&slots));
    slots.push(slots[0].clone());
    assert!(!valid_slots(&slots));
    slots.pop();
    slots[0].slot = "a:b:c".into();
    assert!(!valid_slots(&slots));
}

/// A welcome changes operating parameters without resetting spent identifiers or elapsed refill time.
#[test]
fn negotiated_budget_charges_both_namespaces_and_survives_reconnect() {
    let message=Inbound::parse(r#"{"type":"welcome","lookup_batch_max":2,"lookup_bucket_capacity":4,"lookup_refill_per_min":2,"cache_ttl_ms":500,"ping_interval_ms":1000,"reconnect_max_ms":2000}"#).unwrap();
    let p = message.parameters.normalized();
    assert_eq!(p.batch, 2);
    assert_eq!(p.cache_ttl, Duration::from_millis(500));
    let now = Instant::now();
    let mut budget = Budget::default();
    assert!(budget.take(now, 4, p));
    assert!(!budget.take(now, 1, p));
    let again = Inbound::parse(
        r#"{"type":"welcome","lookup_bucket_capacity":4,"lookup_refill_per_min":2}"#,
    )
    .unwrap()
    .parameters
    .normalized();
    assert!(!budget.take(now, 1, again));
    assert!(budget.take(now + Duration::from_secs(30), 1, again));
    assert!(!budget.take(now + Duration::from_secs(30), 1, again));
    let xuids = vec!["11".into(); CINNABAR_API_MAX_LOOKUP_XUIDS];
    assert!(!valid_watches(
        &xuids,
        &["00112233-4455-6677-8899-aabbccddeeff".into()]
    ));
    assert!(valid_watches(&[], &[]));
}

/// Playback leases, watch coalescing and persistent revisions remain independent under caller time.
#[test]
fn nearby_exchange_preserves_leases_watches_and_loadout_revision_order() {
    let now = Instant::now();
    let uuid = "00112233-4455-6677-8899-aabbccddeeff";
    let local = CinnabarPlayerState {
        xuids: vec!["11".into()],
        uuids: vec![uuid.into()],
        emote_id: "fixture:clip".into(),
        elapsed_ms: 30,
        playback_id: 1,
        slots: vec![CinnabarCosmeticSlot {
            slot: "head".into(),
            item_id: "fixture:item".into(),
        }],
    };
    let mut players = Players::default();
    let mut revision = 2;
    assert!(
        players
            .sync(local.clone(), now, 700, &mut revision)
            .emotes
            .is_empty()
    );
    let loadout = players.next_payload().unwrap();
    assert_eq!(loadout["revision"], 700);
    let playback = players.next_payload().unwrap();
    assert_eq!(playback["xuids"], json!(["11"]));
    assert_eq!(playback["uuids"], json!([uuid]));
    players.receive(Inbound::parse(&json!({"type":"emote","xuid":"11","uuid":uuid,"emote_id":"fixture:clip","elapsed_ms":42,"valid_for_ms":15000}).to_string()).unwrap(),now);
    players.receive(Inbound::parse(&json!({"type":"loadout","xuid":"11","uuid":uuid,"revision":3,"slots":{"head":"fixture:new"}}).to_string()).unwrap(),now);
    players.receive(
        Inbound::parse(
            &json!({"type":"loadout","xuid":"11","uuid":uuid,"revision":2,"slots":{}}).to_string(),
        )
        .unwrap(),
        now,
    );
    let updates = players.sync(
        local.clone(),
        now + Duration::from_secs(1),
        701,
        &mut revision,
    );
    assert_eq!(updates.emotes[0].elapsed_ms, 1042);
    assert_eq!(updates.emotes[0].valid_for_ms, 14000);
    assert_eq!(updates.loadouts[0].revision, 3);
    assert_eq!(revision, 700);
    assert!(players.next_payload().is_none());
    players.receive(
        Inbound::parse(
            &json!({"type":"loadout","xuid":"11","uuid":uuid,"revision":3,"slots":{}}).to_string(),
        )
        .unwrap(),
        now,
    );
    let updates = players.sync(local.clone(), now + EMOTE_TTL, 702, &mut revision);
    assert!(updates.emotes.is_empty() && updates.loadouts.is_empty());
    let renewed = players.next_payload().unwrap();
    assert!(renewed.get("xuids").is_none());
    players.visibility_changed();
    players.sync(local, now + EMOTE_TTL + EMOTE_SPACING, 703, &mut revision);
    assert_eq!(players.next_payload().unwrap()["revision"], 703);
}

/// Late executor polling cannot dispatch a ready operation after its original absolute cutoff.
#[tokio::test(start_paused = true)]
async fn expired_io_is_not_polled_and_native_none_stays_offline() {
    let polled = std::cell::Cell::new(false);
    let until = tokio::time::Instant::now() + REQUEST_TIMEOUT;
    tokio::time::advance(REQUEST_TIMEOUT).await;
    let result = socket::bounded(until, async { polled.set(true) }).await;
    assert!(result.is_err());
    assert!(!polled.get());

    let api = Client::default();
    api.configure(config());
    api.run(None).await;
    assert!(api.lookup(&["11".into()]).await.is_err());
    assert!(matches!(
        api.sync(CinnabarPlayerState::default()),
        PlayerSync::Unavailable
    ));
    assert!(api.take_release().is_none());
}

/// Uses the shipped application identity shape without introducing a separate version/channel table.
fn config() -> CinnabarApiConfig {
    CinnabarApiConfig {
        client_version: "1.0.0".into(),
        client_build: 42,
        channel: update_manifest::channel::Channel::Beta.name().into(),
        visible: true,
    }
}

/// Local grants replace whole, with the same null/default and bounded identifier policy as the wire contract.
#[test]
fn ownership_snapshots_replace_and_revoke_only_welcome_bound_grants() {
    let now = Instant::now();
    let mut revision = 0;
    let mut players = Players::welcomed(
        "11".into(),
        vec!["fixture:capes/paid".into(), "fixture:capes/paid".into()],
    );
    let snapshot = players.sync(CinnabarPlayerState::default(), now, 0, &mut revision);
    assert_eq!(snapshot.ownership_xuid, "11");
    assert_eq!(snapshot.owned_cosmetics, ["fixture:capes/paid"]);
    for (ids, expected) in [
        (json!(["fixture:replacement"]), vec!["fixture:replacement"]),
        (json!(["fixture:valid", "invalid/item"]), vec![]),
        (json!(["fixture:again"]), vec!["fixture:again"]),
        (Value::Null, vec![]),
        (
            json!(vec!["fixture:too-many"; cosmetics::MAX_ITEMS + 1]),
            vec![],
        ),
        (json!([]), vec![]),
    ] {
        players.receive(
            Inbound::parse(&json!({"type":"cosmetic_ownership","owned_cosmetics":ids}).to_string())
                .unwrap(),
            now,
        );
        let snapshot = players.sync(CinnabarPlayerState::default(), now, 0, &mut revision);
        assert_eq!(snapshot.ownership_xuid, "11");
        assert_eq!(snapshot.owned_cosmetics, expected);
    }
    for message in [
        json!({"type":"cosmetic_ownership","owned_cosmetics":["fixture:again"]}),
        json!({"type":"cosmetic_ownership"}),
    ] {
        players.receive(Inbound::parse(&message.to_string()).unwrap(), now);
    }
    assert!(
        players
            .sync(CinnabarPlayerState::default(), now, 0, &mut revision)
            .owned_cosmetics
            .is_empty()
    );
    for invalid in [json!({}), json!("fixture:paid"), json!([false])] {
        assert!(
            Inbound::parse(
                &json!({"type":"cosmetic_ownership","owned_cosmetics":invalid}).to_string()
            )
            .is_none()
        );
    }
    let mut unbound = Players::welcomed("display-name".into(), vec!["fixture:paid".into()]);
    let snapshot = unbound.sync(CinnabarPlayerState::default(), now, 0, &mut revision);
    assert!(snapshot.ownership_xuid.is_empty() && snapshot.owned_cosmetics.is_empty());
}

/// A null string element reaches whole-snapshot revocation instead of hiding the ownership update.
#[test]
fn ownership_null_elements_revoke_the_whole_prior_snapshot() {
    let now = Instant::now();
    let mut observed = Vec::new();
    for ids in [json!([null]), json!(["fixture:valid", null])] {
        let mut players = Players::welcomed("11".into(), vec!["fixture:paid".into()]);
        let mut revision = 0;
        let prior = players.sync(CinnabarPlayerState::default(), now, 0, &mut revision);
        assert_eq!(prior.owned_cosmetics, ["fixture:paid"]);
        let text = json!({"type":"cosmetic_ownership","owned_cosmetics":ids}).to_string();
        if let Some(message) = Inbound::parse(&text) {
            players.receive(message, now);
        }
        let snapshot = players.sync(CinnabarPlayerState::default(), now, 0, &mut revision);
        assert_eq!(snapshot.ownership_xuid, "11");
        observed.push(snapshot.owned_cosmetics);
    }
    assert!(
        observed.iter().all(|ids| ids.is_empty()),
        "null ownership elements left earlier paid grants active: {observed:?}"
    );
}
