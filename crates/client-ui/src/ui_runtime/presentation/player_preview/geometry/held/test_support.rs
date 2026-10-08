//! Held-geometry fixture assertions shared by app carrier tests.
use super::*;
use crate::ui_runtime::presentation::player_preview::{PreviewEquipment, PreviewView};
use assets::{ItemDisplayScalar, RuntimeEntityAssets, RuntimeEquipmentCatalog};
use render_model::equipment::{BoneChannels, attach};
use std::sync::Arc;

/// Creates one deterministic icon reference for geometry assertions.
pub(crate) fn source(page: u16) -> super::super::super::IconRef {
    super::super::super::IconRef {
        page,
        uv: [10, 20, 26, 36],
        glint: false,
    }
}

/// Projects held models using the same preview fixture for all geometry tests.
pub(crate) fn draw(hands: [Option<&PreviewHeldModel>; 2]) -> Arc<ui::UiMesh> {
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

/// Checks both installed shield grips against the authored hand bone positions.
pub fn assert_installed_shield(
    entities: &RuntimeEntityAssets,
    equipment: &RuntimeEquipmentCatalog,
) {
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
    let geometry = render_model::attachable_geometry(
        entities,
        index,
        render_model::item_mesh_rig_id(0),
        texture,
    )
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
    let names = render_model::geometry_bone_names(entities, player).unwrap();
    let origins = render_model::geometry_bone_pivots(entities, player).unwrap();
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
            axis_scale: render_model::UNIT_AXIS_SCALE,
        };
        PreviewHeldPlacement::Authored {
            bone: attach(identity, *pivot, channels, root.binding.is_some()).unwrap(),
            pivot: *pivot,
        }
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
