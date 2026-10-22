//! Placed chalkboards draw the active pack's model and world texture.

use assets::{
    RuntimeEntityAssets,
    block_entity_geometry::{chalkboard::ChalkboardSize, heads::HeadModel},
};
use glam::{Mat4, Vec3};

use super::{
    atlas::BlockEntityAtlas,
    mesh::{Layer, MeshBuilder, WHITE},
};

const HORIZONTAL_SCALE: f32 = 0.998;

/// One base block owns the complete chalkboard model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChalkboardModel {
    pub size: ChalkboardSize,
    pub direction: u8,
    pub on_ground: bool,
}

#[derive(Clone, Debug, Default)]
struct Geometry {
    body: Option<HeadModel>,
    legs: [Option<HeadModel>; 2],
}

#[derive(Clone, Debug, Default)]
pub(super) struct ChalkboardModels {
    sizes: [Geometry; ChalkboardSize::ALL.len()],
}

/// Keeps each cube in its nearest named part while preserving ancestor transforms.
fn part_model(geometry: &assets::EntityGeometry, part: &str) -> Option<HeadModel> {
    let mut selected = geometry.clone();
    let mut bones = geometry.bones.to_vec();
    for bone in &mut bones {
        let mut ancestor = Some(bone.name.as_ref());
        let mut owner = None;
        for _ in 0..geometry.bones.len() {
            let Some(name) = ancestor else { break };
            if ["body", "leftLeg", "rightLeg"]
                .iter()
                .any(|part| name.eq_ignore_ascii_case(part))
            {
                owner = Some(name);
                break;
            }
            ancestor = geometry
                .bones
                .iter()
                .find(|bone| bone.name.eq_ignore_ascii_case(name))
                .and_then(|bone| bone.parent.as_deref());
        }
        if !owner.is_some_and(|name| name.eq_ignore_ascii_case(part)) {
            bone.cubes = Box::default();
        }
    }
    selected.bones = bones.into();
    HeadModel::build_tree(&selected, Some(part), 1.0, false)
}

impl ChalkboardModels {
    /// Replaces supplied sizes while preserving startup models omitted by the active stack.
    pub(super) fn overlay(&self, assets: &RuntimeEntityAssets) -> Self {
        let mut models = self.clone();
        let mut supplied = Self::from_assets(assets);
        for size in ChalkboardSize::ALL {
            if assets
                .geometries()
                .iter()
                .any(|geometry| geometry.identifier.as_ref() == size.geometry())
            {
                models.sizes[size.index()] = std::mem::take(&mut supplied.sizes[size.index()]);
            }
        }
        models
    }

    /// Resolves named parts once when the entity catalog changes.
    pub(super) fn from_assets(assets: &RuntimeEntityAssets) -> Self {
        Self {
            sizes: ChalkboardSize::ALL.map(|size| {
                let geometry = assets
                    .geometries()
                    .iter()
                    .position(|geometry| geometry.identifier.as_ref() == size.geometry())
                    .and_then(|index| {
                        let mut geometry = assets.geometries()[index].clone();
                        geometry.bones = crate::resolve_geometry_bones(assets, index).ok()?.into();
                        Some(geometry)
                    });
                let part = |name| {
                    geometry
                        .as_ref()
                        .and_then(|geometry| part_model(geometry, name))
                };
                Geometry {
                    body: part("body"),
                    legs: [part("leftLeg"), part("rightLeg")],
                }
            }),
        }
    }
}

impl ChalkboardModel {
    /// Maps the pack's mirrored-X authoring frame into this board's world frame.
    fn matrix(self, block: [i32; 3]) -> Mat4 {
        let step = if self.on_ground && self.size == ChalkboardSize::Small {
            22.5_f32
        } else {
            90.0
        };
        let yaw = if self.on_ground || self.direction < 4 {
            -f32::from(self.direction) * step
        } else {
            0.0
        };
        let offset = Vec3::new(
            if self.size == ChalkboardSize::Medium {
                0.5
            } else {
                0.0
            },
            0.0,
            if self.on_ground { 0.0 } else { -7.0 / 16.0 },
        );
        Mat4::from_translation(
            Vec3::from_array(block.map(|value| value as f32)) + Vec3::new(0.5, 0.0, 0.5),
        ) * Mat4::from_rotation_y(yaw.to_radians())
            * Mat4::from_translation(offset)
            * Mat4::from_rotation_y(std::f32::consts::PI)
            * Mat4::from_scale(Vec3::new(HORIZONTAL_SCALE / 16.0, 1.0 / 16.0, 1.0 / 16.0))
    }
}

/// Emits only supplied model parts; mounted boards omit both ground legs.
pub(super) fn emit(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    models: &ChalkboardModels,
    block: [i32; 3],
    model: &ChalkboardModel,
) {
    let geometry = &models.sizes[model.size.index()];
    let matrix = model.matrix(block);
    for part in geometry
        .body
        .iter()
        .chain(geometry.legs.iter().flatten().filter(|_| model.on_ground))
    {
        let Some(texture) = builder.required_texture(atlas, model.size.texture(), part.texture)
        else {
            continue;
        };
        for cube in &part.boxes {
            builder.cuboid(
                Layer::Solid,
                &texture,
                matrix * cube.matrix,
                cube.spec,
                WHITE,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assets::block_entity_geometry::{BoxSpec, heads::HeadBox};
    use assets::{BlockEntityPlacement, RuntimeBlockEntityAssets, encode_block_entity_catalog};

    /// Supplies deliberately asymmetric original shapes to expose mounting and rotation errors.
    fn part(origin: [f32; 3]) -> HeadModel {
        HeadModel {
            texture: [32.0; 2],
            boxes: vec![HeadBox {
                matrix: Mat4::IDENTITY,
                spec: BoxSpec::new(origin, [3.0, 5.0, 2.0], [0.0; 2]),
            }],
        }
    }

    /// Packs solid original artwork for the model path without installed game resources.
    fn atlas() -> BlockEntityAtlas {
        let mut placements: Vec<_> = ChalkboardSize::ALL
            .into_iter()
            .enumerate()
            .map(|(index, size)| BlockEntityPlacement {
                name: size.texture().into(),
                x: index as u32 * 32,
                y: 0,
                width: 32,
                height: 32,
            })
            .collect();
        placements.sort_by(|left, right| left.name.cmp(&right.name));
        let bytes =
            encode_block_entity_catalog(b"{}", 128, 32, &vec![255; 128 * 32 * 4], &placements)
                .unwrap();
        BlockEntityAtlas::from_assets(&RuntimeBlockEntityAssets::decode(&bytes).unwrap())
    }

    /// Builds original asymmetric geometry, including supports nested under the board body.
    fn catalog(width: u8) -> RuntimeEntityAssets {
        let bones = serde_json::json!([
            {"name":"body","cubes":[{"origin":[2,7,1],"size":[width,5,2],"uv":[0,0]}]},
            {"name":"leftLeg","parent":"body","cubes":[{"origin":[-4,0,0],"size":[1,4,1],"uv":[0,0]}]},
            {"name":"leftFoot","parent":"leftLeg","cubes":[{"origin":[-4,0,1],"size":[2,1,2],"uv":[0,0]}]},
            {"name":"rightLeg","parent":"body","cubes":[{"origin":[4,0,0],"size":[1,4,1],"uv":[0,0]}]}
        ]);
        let mut source = serde_json::json!({"format_version":"1.8.0"});
        for size in ChalkboardSize::ALL {
            source[size.geometry()] =
                serde_json::json!({"texturewidth":32,"textureheight":32,"bones":bones});
        }
        let compiled = pack_entity_compiler::compile_entity_pack(vec![(
            "models/mobs.json".into(),
            serde_json::to_vec(&source).unwrap(),
        )])
        .unwrap()
        .unwrap();
        RuntimeEntityAssets::from_compiled(compiled.assets).unwrap()
    }

    #[test]
    fn chalkboard_nested_supports_belong_to_one_mount_part() {
        let models = ChalkboardModels::from_assets(&catalog(3));
        let atlas = atlas();
        for size in ChalkboardSize::ALL {
            for on_ground in [false, true] {
                let mut mesh = MeshBuilder::new(atlas.size());
                emit(
                    &mut mesh,
                    &atlas,
                    &models,
                    [0; 3],
                    &ChalkboardModel {
                        size,
                        direction: 1,
                        on_ground,
                    },
                );
                assert_eq!(mesh.solid.len(), if on_ground { 144 } else { 36 });
                let min_y = mesh
                    .solid
                    .iter()
                    .map(|vertex| vertex.position[1])
                    .fold(f32::INFINITY, f32::min);
                assert!((min_y - if on_ground { 0.0 } else { 7.0 / 16.0 }).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn chalkboard_prepared_catalog_publishes_join_reload_and_startup_restoration() {
        use super::super::{BlockEntityKind, BlockEntityScene, BlockEntitySubmission};
        let mut placements = ChalkboardSize::ALL
            .into_iter()
            .enumerate()
            .map(|(i, size)| BlockEntityPlacement {
                name: size.texture().into(),
                x: i as u32 * 32,
                y: 0,
                width: 32,
                height: 32,
            })
            .collect::<Vec<_>>();
        placements.sort_by(|a, b| a.name.cmp(&b.name));
        let width = super::super::atlas::MIN_DYNAMIC_ATLAS_WIDTH;
        let bytes = encode_block_entity_catalog(
            b"{}",
            width,
            32,
            &vec![255; (width * 32 * 4) as usize],
            &placements,
        )
        .unwrap();
        let carrier = RuntimeBlockEntityAssets::decode(&bytes).unwrap();
        let mut scene = BlockEntityScene::default();
        scene.install_entity_assets(&catalog(3));
        let preparer = scene.artwork_preparer();
        let base = preparer.prepare(&carrier).unwrap();
        let submission = BlockEntitySubmission {
            block: [0; 3],
            light: 1.0.into(),
            kind: BlockEntityKind::Chalkboard(ChalkboardModel {
                size: ChalkboardSize::Large,
                direction: 0,
                on_ground: false,
            }),
        };
        for (index, assets, expected_width) in [
            (0, Some(catalog(9)), 9.0),
            (1, Some(catalog(5)), 5.0),
            (2, None, 3.0),
        ] {
            let prepared = preparer.with_chalkboard_assets(&base, assets.as_ref());
            if index == 0 {
                scene.install_prepared_assets(&prepared);
            } else {
                scene.reload_prepared_assets(&prepared);
            }
            scene.update(
                crate::SceneClock::default(),
                &[],
                std::slice::from_ref(&submission),
            );
            let vertices = &scene.frame().solid;
            assert_eq!(vertices.len(), 36);
            let min = vertices
                .iter()
                .map(|v| v.position[0])
                .fold(f32::INFINITY, f32::min);
            let max = vertices
                .iter()
                .map(|v| v.position[0])
                .fold(f32::NEG_INFINITY, f32::max);
            assert!((max - min - expected_width * HORIZONTAL_SCALE / 16.0).abs() < 1e-5);
        }
        let empty = super::super::artwork::BlockEntityArtworkPreparer::default();
        let supplied = empty.with_chalkboard_assets(&base, Some(&catalog(7)));
        scene.install_prepared_assets(&supplied);
        scene.update(
            crate::SceneClock::default(),
            &[],
            std::slice::from_ref(&submission),
        );
        assert_eq!(
            scene.frame().solid.len(),
            36,
            "join must admit a model absent from startup"
        );
        let prepared = empty.with_chalkboard_assets(&base, None);
        scene.reload_prepared_assets(&prepared);
        scene.update(crate::SceneClock::default(), &[], &[submission]);
        assert!(
            scene.frame().solid.is_empty(),
            "removal must not retain the previous stack's model"
        );
    }

    #[test]
    fn chalkboard_wall_directions_keep_the_board_on_the_selected_block_face() {
        for (direction, expected) in [
            (0, [0.5, 0.0, 0.0625]),
            (1, [0.9375, 0.0, 0.5]),
            (2, [0.5, 0.0, 0.9375]),
            (3, [0.0625, 0.0, 0.5]),
        ] {
            let model = ChalkboardModel {
                size: ChalkboardSize::Large,
                direction,
                on_ground: false,
            };
            assert!(
                model
                    .matrix([0; 3])
                    .transform_point3(Vec3::ZERO)
                    .abs_diff_eq(Vec3::from_array(expected), 1.0e-5)
            );
        }
    }

    #[test]
    fn chalkboard_ground_small_uses_sixteen_turns_and_medium_has_its_two_cell_offset() {
        let small = ChalkboardModel {
            size: ChalkboardSize::Small,
            direction: 1,
            on_ground: true,
        };
        let center = Vec3::new(0.5, 0.0, 0.5);
        let turned = small.matrix([0; 3]).transform_point3(Vec3::X * 16.0) - center;
        let angle = 22.5_f32.to_radians();
        assert!(turned.abs_diff_eq(
            Vec3::new(
                -HORIZONTAL_SCALE * angle.cos(),
                0.0,
                -HORIZONTAL_SCALE * angle.sin()
            ),
            1.0e-5
        ));
        let medium = ChalkboardModel {
            size: ChalkboardSize::Medium,
            direction: 0,
            on_ground: true,
        };
        assert!(
            medium
                .matrix([0; 3])
                .transform_point3(Vec3::ZERO)
                .abs_diff_eq(Vec3::new(1.0, 0.0, 0.5), 1.0e-5)
        );
    }

    #[test]
    fn chalkboard_mount_changes_only_the_supplied_leg_parts_for_every_size() {
        let atlas = atlas();
        let models = ChalkboardModels {
            sizes: std::array::from_fn(|_| Geometry {
                body: Some(part([2.0, 7.0, 1.0])),
                legs: [Some(part([-4.0, 0.0, 0.0])), Some(part([4.0, 0.0, 0.0]))],
            }),
        };
        for size in ChalkboardSize::ALL {
            for on_ground in [false, true] {
                let mut mesh = MeshBuilder::new(atlas.size());
                emit(
                    &mut mesh,
                    &atlas,
                    &models,
                    [0; 3],
                    &ChalkboardModel {
                        size,
                        direction: 1,
                        on_ground,
                    },
                );
                assert_eq!(mesh.solid.len(), if on_ground { 108 } else { 36 });
                let lowest = mesh
                    .solid
                    .iter()
                    .map(|vertex| vertex.position[1])
                    .fold(f32::INFINITY, f32::min);
                assert!((lowest - if on_ground { 0.0 } else { 7.0 / 16.0 }).abs() < 1.0e-5);
            }
        }
    }
}
