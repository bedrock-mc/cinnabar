use super::*;
use crate::{BannerMount, ChestPair, ChestVariant, Facing, SignFace, SignMount};

/// An atlas holding every texture the anchored models below sample.
fn scene() -> BlockEntityScene {
    let mut placements = [
        ("textures/entity/chest/normal", 0, 64, 64),
        ("textures/entity/chest/double_normal", 64, 128, 64),
        (assets::block_entity_geometry::BANNER_TEXTURE.0, 192, 64, 64),
        ("textures/entity/beacon_beam", 256, 16, 16),
        ("textures/environment/end_portal_colors", 272, 4, 4),
        ("textures/entity/end_portal", 288, 256, 256),
    ]
    .map(|(name, x, width, height)| assets::BlockEntityPlacement {
        name: name.into(),
        x,
        y: 0,
        width,
        height,
    });
    placements.sort_by(|a, b| a.name.cmp(&b.name));
    let bytes = assets::encode_block_entity_catalog(
        b"{}",
        1024,
        256,
        &vec![255; 1024 * 256 * 4],
        &placements,
    )
    .unwrap();
    let mut scene = BlockEntityScene::default();
    scene.install_assets(&assets::RuntimeBlockEntityAssets::decode(&bytes).unwrap());
    scene
}

/// Every vertex position in `frame`, across its draw layers.
fn positions(frame: &BlockEntityFrame) -> Vec<[f32; 3]> {
    [
        &frame.solid,
        &frame.overlay,
        &frame.crack,
        &frame.portal,
        &frame.additive,
    ]
    .into_iter()
    .flat_map(|layer| layer.iter().map(|vertex| vertex.position))
    .collect()
}

/// Culling by block reach must never drop a vertex that a block-anchored model draws.
#[test]
fn anchored_models_stay_within_the_cull_reach_and_beams_leave_it() {
    let mut scene = scene();
    let block = [5, 64, -7];
    let rect = scene.text_rect(1, || vec![255; 96 * 48 * 4]).unwrap();
    let chest = |pair| {
        BlockEntityKind::Chest(ChestModel {
            variant: ChestVariant::Normal,
            facing: Facing::East,
            pair,
            lid: 1.0,
        })
    };
    let banner = |mount| {
        BlockEntityKind::Banner(BannerModel {
            mount,
            base: [0.5; 3],
            layers: Vec::new(),
        })
    };
    let anchored = [
        chest(ChestPair::Single),
        chest(ChestPair::Lead {
            partner: [6, 64, -7],
        }),
        banner(BannerMount::Standing {
            rotation_degrees: 45.0,
        }),
        banner(BannerMount::Wall(Facing::South)),
        BlockEntityKind::Sign(SignModel {
            mount: SignMount::Wall(Facing::North),
            front: Some(SignFace {
                rect,
                glowing: false,
            }),
            back: None,
        }),
        BlockEntityKind::EndPortal,
        BlockEntityKind::EndGateway,
    ];
    let reach = BLOCK_ENTITY_REACH_BLOCKS as f32;
    for kind in anchored {
        assert!(!kind.renders_globally(), "{kind:?}");
        for ticks in [0.0, 37.0, 74.0] {
            let submission = BlockEntitySubmission {
                block,
                light: 1.0.into(),
                kind: kind.clone(),
            };
            let drawn = positions(scene.update(SceneClock { ticks }, &[], &[submission]));
            assert!(!drawn.is_empty(), "{kind:?} drew nothing");
            for position in drawn {
                for (axis, (value, origin)) in position.into_iter().zip(block).enumerate() {
                    let offset = value - origin as f32;
                    assert!(
                        (-reach..=1.0 + reach).contains(&offset),
                        "{kind:?} reaches {offset} blocks along axis {axis}"
                    );
                }
            }
        }
    }
    let beacon = BlockEntityKind::Beacon(BeaconModel {
        height: 24,
        tint: [1.0; 3],
    });
    assert!(beacon.renders_globally());
    let submission = BlockEntitySubmission {
        block,
        light: 1.0.into(),
        kind: beacon,
    };
    let drawn = positions(scene.update(SceneClock::default(), &[], &[submission]));
    assert!(
        drawn
            .iter()
            .any(|position| position[1] - block[1] as f32 > 1.0 + reach)
    );
}
