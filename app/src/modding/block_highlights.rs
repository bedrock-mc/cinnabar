//! Presentation adapter for read-only loaded-block highlights.
use crate::{
    app::ClientFrameSet, camera::FlyCamera, menu::MenuRuntime, runtime::world::ClientWorld,
};
use bevy::prelude::*;
use client_ui::ui_runtime::UiRuntime;
use render::ModRenderScene;
use std::sync::{
    OnceLock,
    atomic::{AtomicBool, Ordering},
};

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct BlockHighlights;

pub(super) fn configure(app: &mut App) {
    if std::env::var(super::BLOCK_HIGHLIGHTS_ENV).is_ok_and(|value| value == "1") {
        let _ = registry();
    }
    app.add_systems(
        Update,
        publish
            .in_set(BlockHighlights)
            .after(ClientFrameSet::Camera)
            .before(ClientFrameSet::UiPreparation),
    );
}

#[derive(Default)]
struct Discovery {
    session: u64,
    assets: usize,
    mode: Option<assets::NetworkIdMode>,
    identifiers: Vec<String>,
    ids: Vec<u32>,
    scan: world::BlockHighlightScan,
}

fn registry() -> Option<&'static [assets::RegistryRecord]> {
    static RECORDS: OnceLock<Box<[assets::RegistryRecord]>> = OnceLock::new();
    static STARTED: AtomicBool = AtomicBool::new(false);
    if !STARTED.swap(true, Ordering::AcqRel)
        && std::thread::Builder::new()
            .name("block-highlight-identities".into())
            .spawn(|| {
                let records = assets::read_registry_for_protocol(
                    assets::pinned_block_registry_bytes(),
                    assets::active_content_registry_protocol(),
                )
                .unwrap_or_else(|error| {
                    bevy::log::warn!(%error, "block highlight identity registry could not be read");
                    Box::default()
                });
                let _ = RECORDS.set(records);
            })
            .is_err()
    {
        STARTED.store(false, Ordering::Release);
    }
    RECORDS.get().map(|records| records.as_ref())
}

#[allow(clippy::too_many_arguments)]
fn publish(
    extension: Option<Res<super::ModRuntime>>,
    world: Option<Res<ClientWorld>>,
    menu: Option<Res<MenuRuntime>>,
    ui: Option<Res<UiRuntime>>,
    player: Option<Res<crate::player_runtime::PlayerRuntime>>,
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
            (0..runtime.host_count()).find_map(|index| runtime.host(index).block_highlights())
        });
    let camera = cameras.single().ok();
    let (Some(stream), Some(spec), Some(camera)) = (stream, spec, camera) else {
        discovery.scan.clear();
        discovery.session = 0;
        scene.set_block_highlights(&[], [0.0; 4]);
        return;
    };
    let session = stream.authority().actor_session_id();
    if discovery.session != session {
        discovery.scan.clear();
        discovery.session = session;
    }
    let asset_identity = std::sync::Arc::as_ptr(stream.runtime_assets()) as usize;
    let mode = stream.network_id_mode();
    if discovery.assets != asset_identity
        || discovery.mode != Some(mode)
        || discovery.identifiers != spec.identifiers
    {
        let Some(records) = registry() else {
            scene.set_block_highlights(&[], spec.color);
            return;
        };
        discovery.assets = asset_identity;
        discovery.mode = Some(mode);
        discovery.identifiers.clone_from(&spec.identifiers);
        discovery.ids.clear();
        for record in records.iter().filter(|record| {
            spec.identifiers
                .iter()
                .any(|name| name.as_str() == record.name.as_ref())
        }) {
            let id = match mode {
                assets::NetworkIdMode::Sequential => record.sequential_id,
                assets::NetworkIdMode::Hashed => record.network_hash,
            };
            if stream.runtime_assets().is_known(mode, id) {
                discovery.ids.push(id);
            }
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
        render::MAX_BLOCK_HIGHLIGHTS,
    );
    scene.set_block_highlights(if visible { positions } else { &[] }, spec.color);
}
#[cfg(test)]
mod tests {
    use super::*;
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
}
