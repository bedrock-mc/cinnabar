use std::sync::Arc;

use super::*;
use crate::ui_runtime::presentation::player_preview::{PreviewEquipment, PreviewView};

fn source(page: u16) -> super::super::super::IconRef {
    super::super::super::IconRef {
        page,
        uv: [10, 20, 26, 36],
        glint: false,
    }
}

fn model(
    vertices: Vec<render::ActorRigVertex>,
    placement: PreviewHeldPlacement,
) -> PreviewHeldModel {
    PreviewHeldModel {
        source: source(8),
        vertices: vertices.into(),
        placements: [placement; 2],
        // Pack humanoid pivots in native mirrored rig blocks; production reads
        // the actual geometry's named item bones rather than these fixture values.
        hand_pivots: [
            [6.0 / 16.0, 15.0 / 16.0, 1.0 / 16.0],
            [-6.0 / 16.0, 15.0 / 16.0, 1.0 / 16.0],
        ],
    }
}

fn draw(hands: [Option<&PreviewHeldModel>; 2]) -> Arc<ui::UiMesh> {
    super::super::mesh(
        Default::default(),
        PreviewView::default(),
        0.0,
        source(7),
        &PreviewEquipment::default(),
        [None; 4],
        hands,
        true,
    )
    .unwrap()
}

#[test]
fn cube_has_six_real_faces_and_both_hands_use_model_source_pages() {
    let block = model(
        render::textured_cube_vertices([[0.0, 0.0, 1.0, 1.0]; 6]),
        PreviewHeldPlacement::Block,
    );
    let off = PreviewHeldModel {
        source: source(9),
        ..block.clone()
    };
    let mesh = draw([Some(&block), Some(&off)]);
    assert_eq!(mesh.batches().len(), 3);
    let right = &mesh.batches()[1];
    let left = &mesh.batches()[2];
    assert_eq!(right.index_range.end - right.index_range.start, 36);
    assert_eq!(left.index_range.end - left.index_range.start, 36);
    assert_eq!(right.texture_page, block.source.page);
    assert_eq!(left.texture_page, off.source.page);
    for batch in [right, left] {
        let vertices =
            &mesh.vertices()[batch.index_range.start as usize..batch.index_range.end as usize];
        let far = vertices
            .iter()
            .map(|vertex| vertex.clip_z)
            .fold(f32::INFINITY, f32::min);
        let near = vertices
            .iter()
            .map(|vertex| vertex.clip_z)
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(
            near - far > 0.01,
            "cube must not collapse into a GUI-thumbnail plane"
        );
        assert!(
            vertices
                .windows(2)
                .any(|pair| pair[0].model_light != pair[1].model_light)
        );
    }
}

#[test]
fn sprite_extrusion_keeps_original_side_texel_centres() {
    let sprite = model(
        render::held_sprite_vertices(16, 16, &[255; 16 * 16 * 4], [0.0, 0.0, 1.0, 1.0]).unwrap(),
        PreviewHeldPlacement::Sprite {
            hand_equipped: false,
        },
    );
    let mesh = draw([Some(&sprite), None]);
    let batch = &mesh.batches()[1];
    assert!(batch.index_range.end - batch.index_range.start > 12);
    let emitted =
        &mesh.vertices()[batch.index_range.start as usize..batch.index_range.end as usize];
    for (vertex, native) in emitted.iter().zip(&*sprite.vertices) {
        assert_eq!(
            vertex.uv,
            [10.0 + native.uv[0] * 16.0, 20.0 + native.uv[1] * 16.0]
        );
    }
    assert!(
        emitted
            .iter()
            .skip(12)
            .any(|vertex| vertex.uv[0].fract() == 0.5)
    );
}

#[test]
fn authored_bind_pivot_is_removed_once_and_no_generic_grip_is_applied() {
    let pivot = [1.0, 2.0, 3.0];
    let bone = RenderBoneTransform {
        rotation: Quat::IDENTITY.to_array(),
        translation_scale: [1.1, 2.2, 3.3, 1.0],
        axis_scale: render::UNIT_AXIS_SCALE,
    };
    let authored = model(
        render::textured_cube_vertices([[0.0, 0.0, 1.0, 1.0]; 6]),
        PreviewHeldPlacement::Authored { bone, pivot },
    );
    let (placed, origin) = placement(&authored, 0).unwrap();
    assert_eq!(origin, Vec3::from_array(pivot));
    assert_eq!(placed.rotation, bone.rotation);
    assert_eq!(placed.axis_scale, bone.axis_scale);
    for axis in 0..3 {
        assert_eq!(
            placed.translation_scale[axis],
            bone.translation_scale[axis] + authored.hand_pivots[0][axis]
        );
    }
}

#[test]
fn native_offhand_grip_is_not_a_main_hand_mirror() {
    let sprite = model(
        render::held_sprite_vertices(16, 16, &[255; 16 * 16 * 4], [0.0, 0.0, 1.0, 1.0]).unwrap(),
        PreviewHeldPlacement::Sprite {
            hand_equipped: true,
        },
    );
    let (main, _) = placement(&sprite, 0).unwrap();
    let (off, _) = placement(&sprite, 1).unwrap();
    let main_offset = main.translation_scale[0] - sprite.hand_pivots[0][0];
    let off_offset = off.translation_scale[0] - sprite.hand_pivots[1][0];
    // The world-rig conversion reverses reference X. The offhand reference
    // translation -.125 and hand-equipped translation difference -.1 total +.025 here.
    assert!((off_offset - main_offset - 0.025).abs() < 1e-6);
}

#[test]
fn offhand_raises_only_its_own_arm_in_the_live_controller() {
    let bare = Rig::new(Default::default(), PreviewView::default(), 0.0, [false; 2]);
    let off = Rig::new(
        Default::default(),
        PreviewView::default(),
        0.0,
        [false, true],
    );
    for vertex in render::standard_biped_vertices() {
        if vertex.part == 2 {
            assert_eq!(bare.project(vertex).world, off.project(vertex).world);
        }
    }
    assert!(
        render::standard_biped_vertices()
            .into_iter()
            .filter(|vertex| vertex.part == 3)
            .any(|vertex| bare.project(vertex).world != off.project(vertex).world)
    );
}

#[test]
fn expression_binding_corrects_root_origin_without_changing_mesh_pivot() {
    let pivot = [0.1, 0.9, 0.2];
    let bone = RenderBoneTransform {
        rotation: Quat::IDENTITY.to_array(),
        translation_scale: [0.2, 1.5, 0.3, 1.0],
        axis_scale: render::UNIT_AXIS_SCALE,
    };
    for bound in [false, true] {
        let PreviewHeldPlacement::Authored {
            bone: actual,
            pivot: bind,
        } = PreviewHeldPlacement::authored(bone, pivot, bound)
        else {
            panic!("authored placement")
        };
        assert_eq!(bind, pivot);
        assert_eq!(actual.rotation, bone.rotation);
        assert_eq!(actual.axis_scale, bone.axis_scale);
        let offset = if bound {
            client_world::MODEL_PART_ORIGIN_Y / 16.0
        } else {
            0.0
        };
        assert_eq!(
            actual.translation_scale[1],
            bone.translation_scale[1] - offset
        );
        assert_eq!(actual.translation_scale[0], bone.translation_scale[0]);
        assert_eq!(actual.translation_scale[2], bone.translation_scale[2]);
    }
}

#[test]
#[ignore = "requires installed entity and equipment carriers (make assets)"]
fn installed_shield_bound_root_stays_at_each_hand_not_above_head() {
    use crate::asset_startup::{
        DEFAULT_ASSET_PATH, ENTITY_ASSETS_FILENAME, equipment_carrier::equipment_asset_path,
    };
    use crate::presentation::equipment::{BoneChannels, attach};
    use assets::{ItemDisplayScalar, RuntimeEntityAssets, RuntimeEquipmentCatalog};
    let world_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(DEFAULT_ASSET_PATH);
    let entities = std::fs::read(world_path.with_file_name(ENTITY_ASSETS_FILENAME))
        .expect("offline shield check requires the installed entity carrier (make assets)");
    let equipment = std::fs::read(equipment_asset_path(&world_path))
        .expect("offline shield check requires the installed equipment carrier (make assets)");
    let entities = RuntimeEntityAssets::decode(&entities).unwrap();
    let equipment = RuntimeEquipmentCatalog::decode(&equipment).unwrap();
    let shield = equipment.binding("minecraft:shield").unwrap();
    let index = entities
        .geometries()
        .iter()
        .position(|geometry| geometry.identifier == shield.geometry.identifier)
        .unwrap();
    let [root] = &*entities.geometries()[index].bones else {
        panic!("single shield root")
    };
    assert!(
        root.binding.is_some(),
        "native Shield root binds by item-slot expression"
    );
    let texture = equipment.texture(&shield.texture.identifier).unwrap();
    let geometry =
        render::attachable_geometry(&entities, index, render::item_mesh_rig_id(0), texture)
            .unwrap();
    let [pivot] = &*geometry.bone_pivots else {
        panic!("single bind pivot")
    };
    let player = entities
        .rig_bindings()
        .iter()
        .find(|binding| {
            entities
                .symbols()
                .get(binding.entity_symbol as usize)
                .is_some_and(|symbol| {
                    symbol.kind == assets::EntityAssetKind::Entity
                        && &*symbol.identifier == "minecraft:player"
                })
        })
        .unwrap();
    let player = entities.rig_geometries()[player.first_geometry as usize].geometry as usize;
    let names = render::geometry_bone_names(&entities, player).unwrap();
    let origins = render::geometry_bone_pivots(&entities, player).unwrap();
    let hand_pivots = ["rightItem", "leftItem"].map(|name| {
        origins[names
            .iter()
            .position(|bone| bone.eq_ignore_ascii_case(name))
            .unwrap()]
    });
    let placements = ["main_hand", "off_hand"].map(|slot| {
        let pose = shield.pose(&format!("wield_third_person@{slot}")).unwrap();
        let [bone] = &*pose.bones else {
            panic!("single shield pose")
        };
        let values = |channel: Option<[ItemDisplayScalar; 3]>, rest| {
            channel.map_or([rest; 3], |channel| channel.map(ItemDisplayScalar::get))
        };
        let channels = BoneChannels {
            translation: values(bone.translation, 0.0),
            rotation: values(bone.rotation, 0.0),
            scale: values(bone.scale, 1.0),
        };
        let identity = RenderBoneTransform {
            rotation: Quat::IDENTITY.to_array(),
            translation_scale: [0.0, 0.0, 0.0, 1.0],
            axis_scale: render::UNIT_AXIS_SCALE,
        };
        PreviewHeldPlacement::authored(
            attach(identity, *pivot, channels).unwrap(),
            *pivot,
            root.binding.is_some(),
        )
    });
    let shield = PreviewHeldModel {
        source: super::super::super::IconRef {
            uv: [0, 0, texture.width, texture.height],
            ..source(8)
        },
        vertices: geometry.vertices,
        hand_pivots,
        placements,
    };
    let mesh = draw([Some(&shield), Some(&shield)]);
    for (hand, batch) in mesh.batches().iter().skip(1).enumerate() {
        let ys: Vec<_> = mesh.vertices()
            [batch.index_range.start as usize..batch.index_range.end as usize]
            .iter()
            .map(|vertex| {
                (super::super::super::PREVIEW_FEET_Y - vertex.position[1] * PREVIEW_HEIGHT as f32)
                    / super::super::super::PREVIEW_PIXELS_PER_BLOCK
            })
            .collect();
        let high = ys.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let low = ys.iter().copied().fold(f32::INFINITY, f32::min);
        assert!(
            high < 2.3,
            "hand {hand} Shield must not float above head: {low}..{high}"
        );
        let centre = (low + high) * 0.5;
        assert!(
            (centre - shield.hand_pivots[hand][1]).abs() < 0.6,
            "hand {hand} Shield bounds must remain near its native item bone: {low}..{high}"
        );
    }
}
