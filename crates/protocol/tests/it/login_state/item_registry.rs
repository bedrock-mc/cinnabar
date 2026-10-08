use super::*;

const REGISTRY_SENTINEL_TIME: i32 = 65_432;
const DRAIN_LIMIT: usize = 32;

async fn connected(
    cache_enabled: bool,
) -> (
    protocol::PlaySession<ScriptTransport>,
    Arc<Mutex<ServerScript>>,
) {
    let transport = ScriptTransport::new_with_options(
        CompressionMode::Deflate,
        SpawnOrder::RadiusThenSpawn,
        false,
        false,
        cache_enabled,
        CachePlayScript::ResolveValid,
    );
    let script = Arc::clone(&transport.script);
    let (session, data) = if cache_enabled {
        LoginSequence::connect_transport_with_blob_cache(
            transport,
            "RustClient",
            ClientBlobCache::default(),
        )
        .await
    } else {
        LoginSequence::connect_transport(transport, "RustClient").await
    }
    .expect("scripted login");
    assert_eq!(data.item_registry.item_data.len(), 1);
    assert_eq!(
        data.item_registry.item_data[0].item_name,
        "minecraft:shield"
    );
    (session, script)
}

// Dragonfly sends its full table during StartGame and then a custom-only table
// after spawning. With no custom items the second table is empty. Neither it
// nor a nonempty repeat may reach the ledger or world and erase/rebind identities.
#[tokio::test]
async fn repeated_item_registries_never_reach_world_or_inventory_in_either_cache_mode() {
    for cache_enabled in [false, true] {
        let (mut session, script) = connected(cache_enabled).await;
        script.lock().expect("script lock").enqueue_encrypted(&[
            ItemRegistryPacket::default().into(),
            ItemRegistryPacket {
                item_data: vec![ItemData {
                    item_name: "test:replacement_shield".into(),
                    item_id: 355,
                    ..Default::default()
                }],
            }
            .into(),
            SetTimePacket {
                time: REGISTRY_SENTINEL_TIME,
            }
            .into(),
        ]);
        let mut reached_sentinel = false;
        for _ in 0..DRAIN_LIMIT {
            let event = tokio::time::timeout(
                std::time::Duration::from_secs(1),
                session.recv_world_event(0),
            )
            .await
            .expect("registry repeats must not stall ingress")
            .expect("complete repeat remains nonfatal");
            assert!(
                !matches!(
                    event,
                    WorldEvent::ItemActor(protocol::ItemActorEvent::Registry(_))
                ),
                "registry repeat must not mutate either consumer"
            );
            if event
                == WorldEvent::SetTime(protocol::SetTimeEvent {
                    time: REGISTRY_SENTINEL_TIME,
                })
            {
                reached_sentinel = true;
                break;
            }
        }
        assert!(
            reached_sentinel,
            "ordinary traffic must survive registry repeats"
        );
        assert_eq!(session.decode_error_count(), 0);
    }
}

#[tokio::test]
async fn truncated_repeated_item_registry_is_still_fatal_in_either_cache_mode() {
    for cache_enabled in [false, true] {
        let (mut session, script) = connected(cache_enabled).await;
        // One declared entry with no body is truncated wire, not an empty table.
        script
            .lock()
            .expect("script lock")
            .enqueue_encrypted_raw_packet(McpePacketName::ItemRegistryPacket, &[1]);
        let mut failed = false;
        for _ in 0..DRAIN_LIMIT {
            match tokio::time::timeout(
                std::time::Duration::from_secs(1),
                session.recv_world_event(0),
            )
            .await
            .expect("registry wire error must not stall ingress")
            {
                Ok(_) => {}
                Err(error) => {
                    assert!(matches!(error, ProtocolError::Session(_)));
                    assert_eq!(session.decode_error_count(), 1);
                    failed = true;
                    break;
                }
            }
        }
        assert!(failed, "malformed repeats must still end the session");
    }
}
