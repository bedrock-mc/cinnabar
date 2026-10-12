//! Presentation adapter for read-only loaded-block highlights.
use bevy::prelude::*;
use client_shell::ui_resources::UiRuntimeResource;
use render_scene::ModRenderScene;
use {
    crate::runtime::world::ClientWorld, client_presentation::camera::FlyCamera,
    client_shell::menu::MenuRuntime, client_state::ClientFrameSet,
};

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct BlockHighlights;

pub(super) fn configure(app: &mut App) {
    app.add_systems(
        Update,
        publish
            .in_set(BlockHighlights)
            .after(ClientFrameSet::NetworkSend)
            .before(ClientFrameSet::UiPreparation),
    );
}

#[derive(Default)]
struct Discovery {
    session: u64,
    assets: usize,
    registry_revision: u64,
    mode: Option<assets::NetworkIdMode>,
    identifiers: Vec<String>,
    ids: Vec<u32>,
    scan: world::BlockHighlightScan,
}

/// Resolves every requested name from the admitted session and reports names with no states.
fn resolve_ids(
    authority: &client_world::WorldAuthority,
    names: &[String],
) -> (Vec<u32>, Vec<String>) {
    let mut ids = Vec::new();
    let mut unresolved = Vec::new();
    for name in names {
        let states = authority.block_ids_for_name(name);
        if states.is_empty() {
            unresolved.push(name.clone());
        } else {
            ids.extend_from_slice(states);
        }
    }
    ids.sort_unstable();
    ids.dedup();
    (ids, unresolved)
}

#[allow(clippy::too_many_arguments)]
fn publish(
    extension: Option<Res<super::ModRuntime>>,
    world: Option<Res<ClientWorld>>,
    menu: Option<Res<MenuRuntime>>,
    ui: Option<Res<UiRuntimeResource>>,
    player: Option<Res<client_state::PlayerRuntime>>,
    cameras: Query<&Transform, With<FlyCamera>>,
    mut discovery: Local<Discovery>,
    mut scene: ResMut<ModRenderScene>,
) {
    let visible = !menu.as_deref().is_some_and(MenuRuntime::is_visible)
        && ui
            .as_deref()
            .zip(player.as_deref())
            .is_some_and(|(ui, player)| !ui.ui_focused(player));
    let stream = world
        .as_deref()
        .filter(|world| world.fatal_error.is_none())
        .and_then(|world| world.stream.as_ref());
    let spec = extension
        .as_deref()
        .filter(|runtime| !runtime.suspended)
        .and_then(|runtime| {
            (0..runtime.hosts.host_count())
                .find_map(|index| runtime.hosts.host(index).block_highlights())
        });
    let camera = cameras.single().ok();
    let (Some(stream), Some(spec), Some(camera)) = (stream, spec, camera) else {
        discovery.scan.clear();
        discovery.session = 0;
        scene.set_block_highlights(&[], [0.0; 4]);
        return;
    };
    let session = stream.authority().actor_session_id();
    let session_changed = discovery.session != session;
    if session_changed {
        discovery.scan.clear();
        discovery.session = session;
    }
    let asset_identity = std::sync::Arc::as_ptr(stream.runtime_assets()) as usize;
    let mode = stream.network_id_mode();
    let registry_revision = stream.authority().block_name_revision();
    if session_changed
        || discovery.registry_revision != registry_revision
        || discovery.assets != asset_identity
        || discovery.mode != Some(mode)
        || discovery.identifiers != spec.identifiers
    {
        discovery.assets = asset_identity;
        discovery.registry_revision = registry_revision;
        discovery.mode = Some(mode);
        discovery.identifiers.clone_from(&spec.identifiers);
        let (ids, unresolved) = resolve_ids(stream.authority(), &spec.identifiers);
        discovery.ids = ids;
        if !unresolved.is_empty() {
            bevy::log::warn!(
                ?unresolved,
                "block highlights requested unresolved session identifiers"
            );
        }
        bevy::log::debug!(
            identifiers = ?spec.identifiers,
            id_count = discovery.ids.len(),
            ?mode,
            "block highlight identities resolved"
        );
        discovery.scan.clear();
    }
    let Discovery { scan, ids, .. } = &mut *discovery;
    let positions = scan.update(
        stream.collision_store(),
        stream.current_dimension(),
        camera.translation.to_array(),
        spec.range,
        ids,
        mod_api::MAX_BLOCK_HIGHLIGHTS,
    );
    scene.set_block_highlights(if visible { positions } else { &[] }, spec.color);
}
#[cfg(test)]
mod tests {
    use super::*;
    /// Encodes a signed runtime palette value for a received-subchunk fixture.
    fn signed_varint(value: u32, out: &mut Vec<u8>) {
        let signed = value as i32;
        let mut zigzag = ((signed << 1) ^ (signed >> 31)) as u32;
        while zigzag >= 128 {
            out.push((zigzag as u8) | 128);
            zigzag >>= 7;
        }
        out.push(zigzag as u8);
    }
    #[test]
    fn pinned_finder_ids_discover_received_packed_blocks_in_both_modes() {
        let records = assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap();
        let targets: Vec<_> = records
            .iter()
            .filter(|record| {
                ["minecraft:ancient_debris", "minecraft:netherite_block"]
                    .contains(&record.name.as_ref())
            })
            .collect();
        assert_eq!(targets.len(), 2);
        for mode in [
            assets::NetworkIdMode::Sequential,
            assets::NetworkIdMode::Hashed,
        ] {
            for record in &targets {
                let id = match mode {
                    assets::NetworkIdMode::Sequential => record.sequential_id,
                    assets::NetworkIdMode::Hashed => record.network_hash,
                };
                let mut bytes = vec![8, 1, 3];
                let index = (7usize << 8) | (12usize << 4) | 4;
                let mut words = [0u32; 128];
                words[index / 32] |= 1 << (index % 32);
                for word in words {
                    bytes.extend(word.to_le_bytes());
                }
                signed_varint(2, &mut bytes);
                signed_varint(0, &mut bytes);
                signed_varint(id, &mut bytes);
                let mut store = world::ChunkStore::new();
                let key = world::SubChunkKey::new(1, -1, 0, -1);
                store
                    .apply_sub_chunk(key, &bytes, &world::RawBlockIds { air: 0 })
                    .unwrap();
                assert!(!store.is_sub_chunk_loaded(key));
                assert_eq!(
                    store.sub_chunk(key).unwrap().runtime_id(0, 7, 4, 12),
                    Some(id)
                );
                let mut scan = world::BlockHighlightScan::default();
                for _ in 0..30 {
                    scan.update(&store, 1, [-9.0, 4.0, -4.0], 32.0, &[id], 1024);
                }
                assert_eq!(
                    scan.update(&store, 1, [-9.0, 4.0, -4.0], 32.0, &[id], 1024),
                    &[[-9, 4, -4]],
                    "{} {mode:?}",
                    record.name
                );
            }
        }
    }

    /// Creates an admitted custom palette independently of rendered block artwork.
    fn custom_authority(mode: assets::NetworkIdMode) -> client_world::WorldAuthority {
        let mut authority = client_world::WorldAuthority::new(
            protocol::WorldBootstrap {
                dimension: 0,
                local_player_unique_id: 1,
                local_player_runtime_id: 1,
                player_position: [0.0; 3],
                world_spawn_position: [0; 3],
                air_network_id: 0,
                block_network_ids_are_hashes: mode == assets::NetworkIdMode::Hashed,
            },
            std::sync::Arc::new(assets::RuntimeAssets::diagnostic()),
            None,
            [0.0; 3],
            None,
        );
        authority.set_custom_block_ids(1..2);
        authority.set_custom_block_identities(&protocol::CustomBlocks {
            blocks: std::sync::Arc::from([protocol::CustomBlock {
                name: "example:highlighted".into(),
                state_count: 1,
                collides: true,
                collision_boxes: None,
                selection: protocol::CustomSelection::Default,
                state_physics: Default::default(),
                tags: Default::default(),
                visual: std::sync::Arc::default(),
            }]),
            ..Default::default()
        });
        authority
    }

    #[test]
    fn custom_highlights_resolve_all_session_states_and_report_missing_names() {
        for mode in [
            assets::NetworkIdMode::Sequential,
            assets::NetworkIdMode::Hashed,
        ] {
            let mut authority = custom_authority(mode);
            let requests = [
                "example:highlighted".into(),
                "example:missing".into(),
                "example:highlighted".into(),
            ];
            let (ids, unresolved) = resolve_ids(&authority, &requests);
            assert_eq!(ids, authority.block_ids_for_name("example:highlighted"));
            assert_eq!(ids.len(), 1);
            assert_eq!(unresolved, ["example:missing"]);
            authority.set_custom_block_identities(&protocol::CustomBlocks::default());
            assert!(resolve_ids(&authority, &requests).0.is_empty());
            let next = custom_authority(mode);
            assert_ne!(next.actor_session_id(), authority.actor_session_id());
            assert_eq!(resolve_ids(&next, &requests).0.len(), 1);
        }
    }
}
