use protocol::{BlockEventEvent, WorldBootstrap, WorldEvent};
use render_model::{BellAttachment, ChestModel, ChestPair, ChestVariant, Facing, ItemFrameModel};

use super::*;

/// A stream, registries, font and textured scene for resolving templates outside the system.
struct Fixture {
    stream: chunk_pipeline::WorldStream,
    collisions: PhysicsCollisionRegistries,
    font: Arc<RuntimeFontCatalog>,
    runtime: BlockEntityRuntime,
    scene: BlockEntityScene,
}

impl Fixture {
    fn new() -> Self {
        let breg = assets::pinned_block_registry_bytes();
        let records = assets::read_registry_for_protocol(breg, 2193).unwrap();
        let collisions = PhysicsCollisionRegistries::from_assets(
            breg,
            &records,
            include_bytes!("../../../../crates/assets/data/block-physics-v2193.bin"),
            2193,
        )
        .unwrap();
        let atlas =
            assets::encode_block_entity_catalog(b"{}", 1024, 256, &vec![255; 1024 * 256 * 4], &[])
                .unwrap();
        Self {
            stream: chunk_pipeline::WorldStream::new(WorldBootstrap {
                dimension: 0,
                local_player_runtime_id: 1,
                local_player_unique_id: 1,
                player_position: [0.0, 70.0, 0.0],
                world_spawn_position: [0, 70, 0],
                air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
                block_network_ids_are_hashes: false,
            }),
            collisions,
            font: ui_engine::test_support::fixture_font(),
            runtime: BlockEntityRuntime::new(),
            scene: block_entity_scene(Some(&RuntimeBlockEntityAssets::decode(&atlas).unwrap())),
        }
    }

    /// Resolves `template` at `position` in a frame `delta` seconds long at `now`.
    fn resolve(
        &mut self,
        template: &Template,
        position: [i32; 3],
        (now, delta): (f64, f32),
        hidden: bool,
    ) -> Option<BlockEntityKind> {
        let context = FrameContext {
            stream: &self.stream,
            collisions: &self.collisions,
            eye: Vec3::ZERO,
            delta_seconds: delta,
            now_seconds: now,
            font: &self.font,
        };
        resolve(
            template,
            position,
            &context,
            &mut self.runtime,
            &mut self.scene,
            &mut Vec::new(),
            hidden,
        )
    }
}

/// Hidden containers, bells and frames keep easing, ringing and requesting maps.
#[test]
fn hidden_entities_keep_lids_rings_and_map_requests() {
    let mut fixture = Fixture::new();
    let (chest, bell, frame) = ([0, 64, 0], [4, 64, 0], [8, 64, 0]);
    for (sequence, position) in [(1, chest), (2, bell)] {
        let cue = BlockEventEvent {
            dimension: 0,
            position,
            event_type: 1,
            event_value: 1,
        };
        fixture
            .stream
            .submit(sequence, WorldEvent::BlockEvent(cue))
            .unwrap();
    }
    assert!(fixture.stream.block_event_cue(bell).is_some());
    let chest_template = Template::Chest(ChestModel {
        variant: ChestVariant::Normal,
        facing: Facing::North,
        pair: ChestPair::Single,
        lid: 0.0,
    });
    let bell_template = Template::Bell {
        attachment: BellAttachment::Standing,
        direction: 0,
    };
    let frame_template = Template::ItemFrame {
        model: ItemFrameModel {
            glow: false,
            outward: 2,
            map: None,
        },
        item: None,
        rotation_degrees: 0.0,
        map_id: Some(5),
    };
    for now in [10.0, 10.25] {
        fixture.runtime.lids.begin();
        for (template, position) in [
            (&chest_template, chest),
            (&bell_template, bell),
            (&frame_template, frame),
        ] {
            let hidden = meshing::occlusion::OcclusionBudget::default().hides_box(
                [-10.0, 64.5, 0.5],
                position.map(|axis| axis as f32 - 2.0),
                position.map(|axis| axis as f32 + 3.0),
                |cell, _| cell[0] == -5,
            );
            assert!(hidden);
            assert_eq!(
                fixture.resolve(template, position, (now, 0.25), hidden),
                None
            );
        }
        fixture.runtime.lids.finish();
    }
    assert_eq!(fixture.runtime.missing_maps, [5, 5]);
    fixture.runtime.lids.begin();
    let Some(BlockEntityKind::Chest(model)) =
        fixture.resolve(&chest_template, chest, (10.5, 0.0), false)
    else {
        panic!("a revealed chest draws");
    };
    assert_eq!(
        model.lid, 1.0,
        "two hidden quarter-second frames open the lid"
    );
    let Some(BlockEntityKind::Bell(model)) =
        fixture.resolve(&bell_template, bell, (10.5, 0.0), false)
    else {
        panic!("a revealed bell draws");
    };
    assert_eq!(
        model.seconds_since_ring, 0.5,
        "the ring started while hidden"
    );
}

/// A sign hidden from its first frame shows its text on the frame it is revealed.
#[test]
fn signs_hidden_from_their_first_frame_reveal_with_resident_text() {
    let mut fixture = Fixture::new();
    // More signs than one frame may rasterize.
    let signs: Vec<_> = (0..20)
        .map(|index| {
            let template = Template::Sign {
                mount: render_model::SignMount::Wall(Facing::North),
                front: Some(sign_text::SignTextSpec {
                    text: format!("sign {index}"),
                    color_argb: 0xFF00_0000_u32 as i32,
                    glowing: false,
                    hide_glow_outline: false,
                }),
                back: None,
            };
            ([index, 64, 0], template)
        })
        .collect();
    let hidden_frame = |fixture: &mut Fixture| {
        for (position, template) in &signs {
            assert_eq!(fixture.resolve(template, *position, (0.0, 0.0), true), None);
        }
        fixture
            .scene
            .update(SceneClock::default(), &[], &[])
            .dynamic_revision
    };
    hidden_frame(&mut fixture);
    let revision = hidden_frame(&mut fixture);
    for (position, template) in &signs {
        let kind = fixture.resolve(template, *position, (0.0, 0.0), false);
        assert!(
            matches!(&kind, Some(BlockEntityKind::Sign(model)) if model.front.is_some()),
            "sign at {position:?} revealed as {kind:?}"
        );
    }
    assert_eq!(
        fixture
            .scene
            .update(SceneClock::default(), &[], &[])
            .dynamic_revision,
        revision,
        "revealing rasterized no new canvas"
    );
}
