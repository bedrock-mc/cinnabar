//! Offline first-person held-item frames, rasterized on the CPU from the real rig pose, the
//! near-camera placement and the held-item display, the way the hand pass draws them.

use std::{path::PathBuf, sync::Arc};

use assets::{RuntimeAssets, RuntimeEntityAssets};
use bevy::math::{Mat4, Vec3};
use chunk_pipeline::WorldStream;
use client_world::LocalPlayerFeed;
use protocol::{PlayerSkin, WorldBootstrap};
use render_model::ActorRigFrameBuilder;

use super::frame_raster::{HEIGHT, Image, Target, WIDTH, bone_mat, draw_mesh, rows_to_mat};
use crate::presentation::actors::{actor_rig_presentation, rig_world_from_actor};
use {
    super::display::{FirstPersonHand, FirstPersonShape, first_person_display, view_bone},
    render_model::equipment::{attach_to_bone, held_block_display, held_sprite_display},
};

const REST: FirstPersonHand = FirstPersonHand {
    swing: 0.0,
    equip: 1.0,
    consume: None,
    aspect: 1.0,
};

enum Held {
    Sprite(&'static str),
    Block(&'static str),
}

fn feed(main_hand: &str, first_person: bool) -> LocalPlayerFeed {
    LocalPlayerFeed {
        game_mode: None,
        prefer_client_skin: false,
        uuid: [5; 16],
        username: "local".into(),
        skin: PlayerSkin::Unavailable(protocol::PlayerSkinUnavailable::InvalidDimensions),
        position: [0.0, 64.0, 0.0],
        velocity: [0.0; 3],
        on_ground: true,
        yaw: 0.0,
        head_yaw: 0.0,
        pitch: 0.0,
        main_hand: Some(Arc::from(main_hand)),
        off_hand: None,
        equipment_count: 0,
        held_network_ids: [0; 2],
        held_variants: Default::default(),
        main_hand_metadata: 0,
        main_hand_slot: 0,
        main_hand_stack_id: None,
        bedrock_swing_ticks: actor_animation::ACTOR_SWING_TICKS,
        java_swing_ticks: actor_animation::ACTOR_SWING_TICKS,
        flying: false,
        gliding: false,
        spinning: false,
        fall_fly_ticks: 0,
        teleported: false,
        first_person,
        view_bobbing: true,
        sneaking: false,
        sprinting: false,
        item_use: Default::default(),
    }
}

/// Reads an explicitly configured local fixture path, skipping unconfigured export runs.
fn fixture_path(name: &str) -> Option<PathBuf> {
    match std::env::var(name) {
        Ok(value) => Some(PathBuf::from(value)),
        Err(_) => {
            eprintln!("skipping frame fixture: {name} is not configured");
            None
        }
    }
}

// Writes to CINNABAR_FP_FRAMES from an entity carrier (CINNABAR_FP_ENTITY) and the vanilla
// resource pack (CINNABAR_FP_SAMPLES); CINNABAR_FP_THIRD renders third person instead.
#[test]
fn render_first_person_held_item_frames() {
    let (Some(out), Some(samples), Some(entity_path)) = (
        fixture_path("CINNABAR_FP_FRAMES"),
        fixture_path("CINNABAR_FP_SAMPLES"),
        fixture_path("CINNABAR_FP_ENTITY"),
    ) else {
        return;
    };
    let custom = std::env::var("CINNABAR_FP_CUSTOM_ICON").ok();
    let third = std::env::var("CINNABAR_FP_THIRD").is_ok();
    let entity_bytes = match std::fs::read(&entity_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("skipping frame fixture: missing {}", entity_path.display());
            return;
        }
        Err(error) => panic!("read {}: {error}", entity_path.display()),
    };
    let entities = Arc::new(RuntimeEntityAssets::decode(&entity_bytes).unwrap());
    let Some(skin) = Image::load(&samples.join("textures/entity/steve.png")) else {
        return;
    };
    let mut builder = ActorRigFrameBuilder::from_runtime_assets(&entities).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    let mut cases = vec![
        (
            "sprite",
            "minecraft:ender_pearl",
            Held::Sprite("textures/items/ender_pearl.png"),
        ),
        (
            "tool",
            "minecraft:diamond_sword",
            Held::Sprite("textures/items/diamond_sword.png"),
        ),
        (
            "block",
            "minecraft:andesite",
            Held::Block("textures/blocks/stone_andesite.png"),
        ),
    ];
    let custom_icon: &'static str = custom.map_or("", |path| Box::leak(path.into_boxed_str()));
    if !custom_icon.is_empty() {
        cases.push(("custom", "zeqa:item.ffa", Held::Sprite(custom_icon)));
    }
    for (name, identifier, held) in cases {
        let mut world = WorldStream::new_with_asset_sets(
            WorldBootstrap {
                local_player_unique_id: 1,
                dimension: 0,
                local_player_runtime_id: 1,
                player_position: [0.0, 64.0, 0.0],
                world_spawn_position: [0, 64, 0],
                air_network_id: 0,
                block_network_ids_are_hashes: false,
            },
            Arc::new(RuntimeAssets::diagnostic()),
            Arc::clone(&entities),
            [0.0, 64.0, 0.0],
            None,
        );
        for _ in 0..40 {
            world.sync_local_player_pose(&feed(identifier, !third));
            world.advance_actor_interpolation_ticks(1);
        }
        let rig = world.authority().actor_rig(1).unwrap();
        let presentation =
            actor_rig_presentation(&rig, world.authority().actor(1).unwrap(), None, 1.0).unwrap();
        let placement = if third {
            // Looks at the model's right side from ahead of it (it faces +Z at yaw 0).
            glam::camera::rh::view::look_at_mat4(
                Vec3::new(-3.0, 1.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::Y,
            ) * rows_to_mat(rig_world_from_actor(
                [0.0; 3],
                0.0,
                presentation.authored_scale,
            ))
        } else {
            rows_to_mat(rig_world_from_actor(
                [
                    0.0,
                    -crate::local_player::LOCAL_AVATAR_EYE_HEIGHT_BLOCKS,
                    0.0,
                ],
                0.0,
                presentation.authored_scale,
            ))
        };
        let mut target = Target {
            color: [120u8, 165, 230, 255].repeat(WIDTH * HEIGHT),
            depth: vec![f32::INFINITY; WIDTH * HEIGHT],
        };
        // The body in third person; first person hides the arm behind a held item, as vanilla.
        let frame = builder.build(1.0, None, [presentation.submission.clone()]);
        let instance = frame.instances[0];
        let span = frame.geometry_spans[instance.geometry_id as usize];
        let body = if third {
            frame.geometry_vertices.span(span).unwrap_or_default()
        } else {
            &[][..]
        };
        let bones = &frame.current_bones[instance.current_bone_base as usize..];
        draw_mesh(
            &mut target,
            body,
            |vertex| {
                let bone = rows_to_mat(bones[vertex.bone_index as usize]);
                placement.transform_point3(bone.transform_point3(Vec3::from(vertex.position)))
            },
            &skin,
        );
        let right_item = rig
            .bone_names
            .iter()
            .position(|bone| bone.eq_ignore_ascii_case("rightItem"))
            .unwrap();
        let hand = presentation.submission.input.current_bones[right_item];
        let (vertices, display, texture) = match held {
            Held::Sprite(path) => {
                let Some(icon) = Image::load(&samples.join(path)) else {
                    return;
                };
                let vertices = render_model::held_sprite_vertices(
                    icon.width,
                    icon.height,
                    &icon.rgba8,
                    [0.0, 0.0, 1.0, 1.0],
                )
                .unwrap();
                let display = if third {
                    held_sprite_display(
                        assets::item::grip::builtin_item_grip(identifier)
                            .upright()
                            .unwrap_or(false),
                    )
                } else {
                    first_person_display(
                        FirstPersonShape::Sprite {
                            mirrored_art: assets::item::grip::builtin_item_grip(identifier)
                                .mirrored_art(),
                        },
                        REST,
                    )
                };
                (vertices, display, icon)
            }
            Held::Block(path) => {
                let Some(face) = Image::load(&samples.join(path)) else {
                    return;
                };
                let mut sheet = vec![0u8; 48 * 32 * 4];
                for tile in 0..6 {
                    let (tx, ty) = ((tile % 3) * 16, (tile / 3) * 16);
                    for row in 0..16 {
                        let target = ((ty + row) * 48 + tx) * 4;
                        sheet[target..target + 64]
                            .copy_from_slice(&face.rgba8[row * 64..(row + 1) * 64]);
                    }
                }
                let texture = Image {
                    width: 48,
                    height: 32,
                    rgba8: sheet,
                };
                let vertices = render_model::textured_cube_vertices(
                    render_model::equipment::blocks::face_rects([0.0, 0.0, 1.0, 1.0]),
                );
                let display = if third {
                    held_block_display()
                } else {
                    first_person_display(FirstPersonShape::Block, REST)
                };
                (vertices, display, texture)
            }
        };
        // First person places the item in camera space; third person rides the hand bone.
        let (item, item_placement) = if third {
            (bone_mat(attach_to_bone(hand, display).unwrap()), placement)
        } else {
            (bone_mat(view_bone(display).unwrap()), Mat4::IDENTITY)
        };
        draw_mesh(
            &mut target,
            &vertices,
            |vertex| {
                item_placement.transform_point3(item.transform_point3(Vec3::from(vertex.position)))
            },
            &texture,
        );
        image::RgbaImage::from_raw(WIDTH as u32, HEIGHT as u32, target.color)
            .unwrap()
            .save(out.join(format!("{name}.png")))
            .unwrap();
    }
}
