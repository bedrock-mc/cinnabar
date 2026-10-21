//! Remote packets must survive pack replacement, publication and native draw preparation.

use crate::runtime::{
    network::{advance_actor_frame, prepare_actor_render_frame},
    world::ClientWorld,
};
use bevy::prelude::{Vec3, World};
use client_presentation::{
    actor_publication::{HandRigBuilder, publish_actor_render_frame},
    presentation::equipment::EquipmentRuntime,
    session_assets::SessionItems,
};
use protocol::{WorldEvent, test_support::remote_equipment as packets};
use render::{EquipmentPreparation, PreparedEquipmentDraw};
use render_model::ActorArtworkPages;
use render_scene::{ActorRenderFrame, ActorRenderScene};
use std::sync::Arc;

mod fixture;
use fixture::*;

const REMOTE: u64 = 900;
const LIST_ID: i64 = -901;
const SPAWN_ID: i64 = -902;
const UUID: [u8; 16] = [9; 16];
const NAME: &str = "Remote Equipment";

/// Owns the live world, immutable startup catalog and persistent render-side state.
struct Session {
    world: World,
    equipment: Arc<assets::RuntimeEquipmentCatalog>,
    prepare: EquipmentPreparation,
    sequence: u64,
}

impl Session {
    /// Builds every required startup input from original, always-present fixture sources.
    fn new() -> Self {
        let mut files = player_files(true);
        files.extend(equipment_files(false, true));
        let compiled = pack_entity_compiler::compile_actor_pack(files)
            .unwrap()
            .unwrap();
        let equipment = Arc::new(
            assets::RuntimeEquipmentCatalog::from_parts(
                compiled.identity,
                compiled.equipment_bindings,
                compiled.equipment_textures,
            )
            .unwrap(),
        );
        let entities =
            Arc::new(assets::RuntimeEntityAssets::from_compiled(compiled.entities).unwrap());
        let base =
            ActorArtworkPages::default().with_pack_artwork(&compiled.textures, &compiled.bindings);
        let icons = Arc::new(
            assets::RuntimeIconCatalog::decode(
                &assets::encode_icon_catalog([1; 32], &[], &[]).unwrap(),
            )
            .unwrap(),
        );
        let (runtime, artwork, equipment_geometries) = EquipmentRuntime::build(
            entities.clone(),
            Some(equipment.clone()),
            icons,
            None,
            None,
            base,
        );
        let scene = ActorRenderScene::with_runtime_entity_assets_and_equipment(
            &entities,
            &equipment_geometries,
        )
        .unwrap();
        let mut client = ClientWorld::new_with_entity_assets(
            Arc::new(assets::RuntimeAssets::diagnostic()),
            entities.clone(),
        );
        client.stream = Some(super::actor_rest_presentation::stream(entities.clone()));
        let attachable = PIECES.last().unwrap();
        let registry = protocol::test_support::items::startup_with_item_icons(&[
            (SPRITE, SPRITE_NETWORK_ID, Some("fixture_sprite")),
            (&attachable.identifier(), attachable.network_id(), None),
        ])
        .items
        .registry
        .unwrap();
        let mut entries = registry.entries.to_vec();
        for piece in &PIECES {
            if let Some(slot) = piece.slot {
                entries.extend(
                    packets::wearable_registry(&piece.identifier(), piece.network_id(), slot)
                        .entries
                        .iter()
                        .cloned(),
                );
            }
        }
        let registry = protocol::ItemRegistryEvent {
            entries: entries.into(),
        };
        let components = registry
            .entries
            .iter()
            .filter_map(|entry| {
                entry
                    .components
                    .as_deref()
                    .map(|components| (entry.identifier.clone(), components.clone()))
            })
            .collect();
        client.session_items = Some(Arc::new(SessionItems {
            components: Arc::new(components),
            icons: Some(Arc::new(
                client_ui::ui_runtime::presentation::SessionIcons {
                    icons: vec![client_ui::ui_runtime::presentation::SessionIcon {
                        identifier: SPRITE.into(),
                        metadata: 0,
                        width: 16,
                        height: 16,
                        rgba8: SPRITE_COLOR.repeat(16 * 16).into(),
                    }],
                    ..Default::default()
                },
            )),
        }));
        assert!(client.stream.as_mut().unwrap().seed_item_registry(registry));
        let mut world = super::actor_frame_allocations::actor_frame_world(
            client,
            scene,
            artwork,
            HandRigBuilder::from_runtime_assets(&entities).unwrap(),
            (Vec3::new(0.0, 66.0, -12.0), Vec3::new(0.0, 65.0, 0.0)),
        );
        world.insert_resource(runtime);
        Self {
            world,
            equipment,
            prepare: EquipmentPreparation::noop(),
            sequence: 0,
        }
    }

    /// Commits decoded packets in server FIFO order without hand-constructing appearance state.
    fn submit(&mut self, events: impl IntoIterator<Item = WorldEvent>) {
        let mut client = self.world.resource_mut::<ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        for event in events {
            self.sequence += 1;
            stream.submit(self.sequence, event).unwrap();
        }
        // The roster screen consumes these notifications separately from actor publication.
        let _ = stream.take_committed_controls();
    }

    /// Sends the roster and spawn independently, including distinct list and spawn unique IDs.
    fn spawn(&mut self, runtime_id: u64, feet: [f32; 3]) {
        let mut uuid = UUID;
        uuid[..8].copy_from_slice(&runtime_id.to_le_bytes());
        let (list_id, spawn_id) = if runtime_id == REMOTE {
            (LIST_ID, SPAWN_ID)
        } else {
            (-(runtime_id as i64) * 2, -(runtime_id as i64) * 2 - 1)
        };
        self.submit(packets::player(
            runtime_id,
            list_id,
            spawn_id,
            uuid,
            NAME,
            feet,
            &skin(runtime_id),
        ));
    }

    /// Equips or clears all worn slots and both hands through the packet adapters.
    fn equip(&mut self, equipped: bool) {
        let armor = std::array::from_fn(|index| {
            if equipped {
                PIECES[index].network_id()
            } else {
                0
            }
        });
        self.submit(packets::equipment(
            REMOTE,
            armor,
            if equipped {
                [SPRITE_NETWORK_ID, PIECES.last().unwrap().network_id()]
            } else {
                [0; 2]
            },
        ));
    }

    /// Replaces the admitted pack without recreating equipment or the spawned player.
    fn pack(&mut self, files: Option<Files>) {
        let pack = files.map(|files| session_pack(&files, &self.equipment));
        let mut client = self.world.resource_mut::<ClientWorld>();
        client
            .stream
            .as_mut()
            .unwrap()
            .set_pack_entities(pack.as_ref().map(|pack| {
                (
                    pack.assets.clone(),
                    pack.bindings
                        .iter()
                        .map(|binding| binding.geometry_candidate)
                        .collect(),
                )
            }));
        client.pack_entities = pack;
    }

    /// Runs the production actor stages and then the actual render preparation consumer.
    fn frame(&mut self) -> (ActorRenderFrame, Vec<PreparedEquipmentDraw>) {
        let mut frame = ActorRenderFrame::default();
        test_time::eventually("remote cape worker publication", || {
            self.world
                .resource_mut::<ClientWorld>()
                .stream
                .as_mut()
                .unwrap()
                .prepare_actor_appearance_fixture();
            self.world.run_system_cached(advance_actor_frame).unwrap();
            self.world
                .run_system_cached(prepare_actor_render_frame)
                .unwrap();
            self.world
                .run_system_cached(publish_actor_render_frame)
                .unwrap();
            frame = self.world.resource::<ActorRenderFrame>().clone();
            let present = self
                .world
                .resource::<ClientWorld>()
                .stream
                .as_ref()
                .unwrap()
                .authority()
                .actor(REMOTE)
                .is_some();
            !present
                || frame.rig.manifest.iter().any(|entry| {
                    entry.identity.runtime_id == REMOTE
                        && entry.identity.layer
                            == client_presentation::presentation::cape::ACTOR_LAYER_CAPE
                })
        });
        let draws = self.prepare.prepare(&frame);
        (frame, draws)
    }

    /// Builds emitted nametag quads and atlas ink from this actor's replicated display name.
    fn assert_nametag(&self) {
        use client_ui::ui_runtime::presentation::{nametag_atlas, nametags};
        let stream = self
            .world
            .resource::<ClientWorld>()
            .stream
            .as_ref()
            .unwrap();
        let actor = stream.authority().actor(REMOTE).unwrap();
        let name = stream
            .authority()
            .actor_display_name(actor.unique_id)
            .unwrap();
        assert_eq!(name.as_ref(), NAME);
        let anchor = nametags::extract_nametag(
            actor,
            nametags::NametagView {
                camera_position: Vec3::new(0.0, 66.0, -12.0),
                camera_forward: Vec3::Z,
            },
            Some(REMOTE),
            name,
            &ui::ScoreboardStore::default(),
            1.0,
        )
        .expect("remote nametag admitted");
        let font = ui_engine::test_support::fixture_font();
        let scene = nametags::build_nametag_scene(
            &[anchor],
            None,
            &font,
            &mut ui::TextLayoutCache::new(8, 1 << 20),
            &mut nametag_atlas::NametagAtlas::default(),
            &|page| nametag_atlas::font_page(&font, page),
        );
        assert!(
            scene
                .records
                .iter()
                .any(|record| record.text != 0 && record.rect[2] > record.rect[0])
        );
        assert!(scene.atlas.iter().any(|cell| {
            cell.rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] != 0)
        }));
    }
}

/// Resolves the exact artwork selected by a prepared instance, including resident cape skins.
fn pixel(frame: &ActorRenderFrame, draw: &PreparedEquipmentDraw) -> [u8; 4] {
    if draw.page == 0 {
        return frame
            .skins
            .resident(draw.instance.texture_layer)
            .unwrap()
            .texels[..4]
            .try_into()
            .unwrap();
    }
    let page = &frame.artwork_pages().pages()[usize::from(draw.page - 1)];
    let (width, height) = page.dimensions();
    let offset =
        draw.instance.texture_layer as usize * usize::from(width) * usize::from(height) * 4;
    page.pixels()[offset..offset + 4].try_into().unwrap()
}

/// The model and artwork changes expected from each pack transition.
#[derive(Clone, Copy, PartialEq)]
enum Outfit {
    Startup,
    ModelOnly,
    TextureOnly,
    Mixed,
}

/// Every equipment piece must have its own prepared mesh and current artwork.
fn assert_equipped(frame: &ActorRenderFrame, draws: &[PreparedEquipmentDraw], outfit: Outfit) {
    let switched = matches!(outfit, Outfit::TextureOnly | Outfit::Mixed);
    let remote = draws
        .iter()
        .filter(|draw| draw.identity.runtime_id == REMOTE)
        .collect::<Vec<_>>();
    assert_eq!(
        remote.len(),
        PIECES.len() + 3,
        "body, cape, sprite and every attachable must reach prepared draws: {remote:?}"
    );
    for draw in &remote {
        assert!(draw.vertex_count > 0);
        let span = frame.rig.geometry_spans[draw.instance.geometry_id as usize];
        assert!(
            frame
                .rig
                .geometry_vertices
                .span(span)
                .is_some_and(|vertices| vertices.len() == draw.vertex_count as usize)
        );
    }
    for (index, piece) in PIECES.iter().enumerate() {
        let drawn = remote
            .iter()
            .filter(|draw| pixel(frame, draw) == piece.color(switched))
            .collect::<Vec<_>>();
        assert_eq!(
            drawn.len(),
            1,
            "{} artwork must occur exactly once",
            piece.name
        );
        assert_eq!(
            drawn[0].vertex_count,
            ((index + 1 + usize::from(outfit == Outfit::Mixed)) * 36) as u32,
            "{} must draw its own model",
            piece.name
        );
    }
    assert_eq!(
        remote
            .iter()
            .filter(|draw| pixel(frame, draw) == SPRITE_COLOR)
            .count(),
        1
    );
    let cape = remote
        .iter()
        .find(|draw| {
            draw.identity.layer == client_presentation::presentation::cape::ACTOR_LAYER_CAPE
        })
        .expect("cape draw");
    assert_eq!(pixel(frame, cape), CAPE_COLOR);
    let body = remote
        .iter()
        .find(|draw| draw.identity.layer == render_model::ACTOR_LAYER_BODY)
        .expect("body draw");
    assert_eq!(pixel(frame, body), SKIN_COLOR);
    assert_eq!(
        render_model::is_pack_rig_id(body.rig),
        matches!(outfit, Outfit::ModelOnly | Outfit::Mixed)
    );
}

#[test]
fn remote_equipment_survives_pack_and_actor_lifecycles() {
    let mut session = Session::new();
    session.spawn(REMOTE, [0.0, 64.0, 0.0]);
    session.equip(true);
    session.pack(Some(player_files(false)));
    let (frame, draws) = session.frame();
    assert_equipped(&frame, &draws, Outfit::ModelOnly);
    session.assert_nametag();
    let original_spawn = draws[0].identity.spawn_revision;

    session.pack(Some(equipment_files(true, false)));
    let (frame, draws) = session.frame();
    assert_equipped(&frame, &draws, Outfit::TextureOnly);
    session.assert_nametag();
    let mut mixed = player_files(false);
    mixed.extend(equipment_files(true, true));
    session.pack(Some(mixed.clone()));
    let (frame, draws) = session.frame();
    assert_equipped(&frame, &draws, Outfit::Mixed);
    session.assert_nametag();
    session.pack(None);
    let (frame, draws) = session.frame();
    assert_equipped(&frame, &draws, Outfit::Startup);
    session.assert_nametag();
    session.pack(Some(mixed));
    let (frame, draws) = session.frame();
    assert_equipped(&frame, &draws, Outfit::Mixed);
    session.equip(false);
    let (frame, draws) = session.frame();
    let remote = draws
        .iter()
        .filter(|draw| draw.identity.runtime_id == REMOTE)
        .collect::<Vec<_>>();
    assert_eq!(
        remote.len(),
        2,
        "clearing equipment retains only the body and cape"
    );
    assert!(
        remote
            .iter()
            .all(|draw| draw.identity.layer == render_model::ACTOR_LAYER_BODY
                || draw.identity.layer
                    == client_presentation::presentation::cape::ACTOR_LAYER_CAPE)
    );
    assert!(remote.iter().all(|draw| {
        !PIECES
            .iter()
            .any(|piece| pixel(&frame, draw) == piece.color(true))
    }));
    session.equip(true);
    let (frame, draws) = session.frame();
    assert_equipped(&frame, &draws, Outfit::Mixed);
    session.submit([packets::remove(SPAWN_ID)]);
    let (_, draws) = session.frame();
    assert!(draws.iter().all(|draw| draw.identity.runtime_id != REMOTE));
    session.spawn(REMOTE, [0.0, 64.0, 0.0]);
    session.equip(true);
    let (frame, draws) = session.frame();
    assert_equipped(&frame, &draws, Outfit::Mixed);
    session.assert_nametag();
    assert!(
        draws
            .iter()
            .filter(|draw| draw.identity.runtime_id == REMOTE)
            .all(|draw| draw.identity.spawn_revision > original_spawn)
    );
    if let Some(pixels) = render::equipment_pixels(&frame) {
        for piece in &PIECES {
            assert_eq!(
                pixels
                    .iter()
                    .filter(|(id, pixel)| id.runtime_id == REMOTE && *pixel == piece.color(true))
                    .count(),
                1,
                "native {} pixels",
                piece.name
            );
        }
        assert!(
            pixels
                .iter()
                .any(|(id, pixel)| id.runtime_id == REMOTE && *pixel == SPRITE_COLOR)
        );
    }
}

#[test]
fn offscreen_crowd_leaves_capacity_for_equipped_remote_players() {
    let mut session = Session::new();
    session.spawn(REMOTE, [0.0, 64.0, 0.0]);
    session.equip(true);
    session.pack(Some(player_files(false)));
    let (frame, draws) = session.frame();
    assert_equipped(&frame, &draws, Outfit::ModelOnly);
    // Confirm a crowd member can be drawn before moving that same lifetime outside the view.
    session.spawn(2, [2.0, 64.0, 0.0]);
    let (frame, draws) = session.frame();
    let neighbour = draws
        .iter()
        .find(|draw| {
            draw.identity.runtime_id == 2 && draw.identity.layer == render_model::ACTOR_LAYER_BODY
        })
        .expect("crowd positive control");
    assert_ne!(pixel(&frame, neighbour), SKIN_COLOR);
    session.submit([protocol::test_support::packets::move_actor_absolute_event(
        protocol::test_support::packets::ActorAbsoluteFixture {
            runtime_id: 2,
            flags: 2,
            position: [10_000.0, 64.0, 0.0],
            ..Default::default()
        },
    )
    .unwrap()
    .unwrap()]);
    for runtime_id in 3..render_model::MAX_RENDERED_PLAYERS as u64 + 4 {
        session.spawn(runtime_id, [10_000.0, 64.0, 0.0]);
    }
    test_time::eventually(
        "crowd appearances admitted before the capacity control",
        || {
            let mut client = session.world.resource_mut::<ClientWorld>();
            let stream = client.stream.as_mut().unwrap();
            stream.prepare_actor_appearance_fixture();
            stream.advance_actor_interpolation_frame(0);
            stream.authority().render_players().len() == render_model::MAX_RENDERED_PLAYERS + 3
        },
    );
    for _ in 0..2 {
        let (frame, draws) = session.frame();
        assert_equipped(&frame, &draws, Outfit::ModelOnly);
        assert!(draws.iter().all(|draw| draw.identity.runtime_id == REMOTE));
        session.assert_nametag();
    }
}
