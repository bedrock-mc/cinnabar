//! Offline first-person held-item frames, rasterized on the CPU from the real rig pose, the
//! near-camera placement and the held-item display, the way the hand pass draws them.

use std::{path::PathBuf, sync::Arc};

use assets::{RuntimeAssets, RuntimeEntityAssets};
use bevy::math::{Mat4, Vec3, Vec4};
use chunk_pipeline::WorldStream;
use client_world::LocalPlayerFeed;
use protocol::{PlayerSkin, WorldBootstrap};
use render::{ActorRigFrameBuilder, ActorRigVertex, RenderBoneTransform};

use super::display::{
    FirstPersonHand, FirstPersonShape, attach_to_bone, first_person_display, held_block_display,
    held_sprite_display, is_hand_equipped, is_mirrored_art, view_bone,
};
use crate::presentation::actors::{actor_rig_presentation, rig_world_from_actor};

const WIDTH: usize = 854;
const REST: FirstPersonHand = FirstPersonHand {
    swing: 0.0,
    equip: 1.0,
    consume: None,
};
const HEIGHT: usize = 480;

struct Image {
    width: usize,
    height: usize,
    rgba8: Vec<u8>,
}

impl Image {
    /// Loads a local texture, skipping an absent fixture while rejecting malformed images.
    fn load(path: &std::path::Path) -> Option<Self> {
        let image = match image::open(path) {
            Ok(image) => image.to_rgba8(),
            Err(image::ImageError::IoError(error))
                if error.kind() == std::io::ErrorKind::NotFound =>
            {
                eprintln!("skipping frame fixture: missing {}", path.display());
                return None;
            }
            Err(error) => panic!("{}: {error}", path.display()),
        };
        Some(Self {
            width: image.width() as usize,
            height: image.height() as usize,
            rgba8: image.into_raw(),
        })
    }

    fn sample(&self, [u, v]: [f32; 2]) -> [u8; 4] {
        let x = ((u * self.width as f32) as usize).min(self.width - 1);
        let y = ((v * self.height as f32) as usize).min(self.height - 1);
        let offset = (y * self.width + x) * 4;
        std::array::from_fn(|channel| self.rgba8[offset + channel])
    }
}

struct Target {
    color: Vec<u8>,
    depth: Vec<f32>,
}

/// One camera-space vertex with both face UVs.
#[derive(Clone, Copy)]
struct Placed {
    view: Vec3,
    uv: [f32; 2],
    back_uv: [f32; 2],
}

fn project(view: Vec3) -> Option<Vec3> {
    if view.z > -0.025 {
        return None;
    }
    let focal = 1.0 / (crate::actor_publication::HAND_FOV_DEGREES.to_radians() * 0.5).tan();
    let aspect = WIDTH as f32 / HEIGHT as f32;
    let (x, y) = (focal / aspect * view.x / -view.z, focal * view.y / -view.z);
    Some(Vec3::new(
        (x + 1.0) * 0.5 * WIDTH as f32,
        (1.0 - y) * 0.5 * HEIGHT as f32,
        -view.z,
    ))
}

fn draw(target: &mut Target, triangle: [Placed; 3], texture: &Image) {
    let (Some(a), Some(b), Some(c)) = (
        project(triangle[0].view),
        project(triangle[1].view),
        project(triangle[2].view),
    ) else {
        return;
    };
    let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    if area.abs() < 1.0e-6 {
        return;
    }
    // Screen Y points down, so a counter-clockwise (front) face has negative area here.
    let front = area < 0.0;
    let normal = (triangle[1].view - triangle[0].view)
        .cross(triangle[2].view - triangle[0].view)
        .normalize_or_zero();
    let shade = 0.55 + 0.45 * normal.dot(Vec3::new(0.3, 0.8, 0.5).normalize()).abs();
    let (min_x, max_x) = (a.x.min(b.x).min(c.x).max(0.0), a.x.max(b.x).max(c.x));
    let (min_y, max_y) = (a.y.min(b.y).min(c.y).max(0.0), a.y.max(b.y).max(c.y));
    for y in min_y as usize..(max_y.ceil() as usize).min(HEIGHT) {
        for x in min_x as usize..(max_x.ceil() as usize).min(WIDTH) {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let w0 = ((b.x - px) * (c.y - py) - (b.y - py) * (c.x - px)) / area;
            let w1 = ((c.x - px) * (a.y - py) - (c.y - py) * (a.x - px)) / area;
            let w2 = 1.0 - w0 - w1;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }
            let inverse = w0 / a.z + w1 / b.z + w2 / c.z;
            let depth = 1.0 / inverse;
            let index = y * WIDTH + x;
            if depth >= target.depth[index] {
                continue;
            }
            let uv_of = |vertex: &Placed| if front { vertex.uv } else { vertex.back_uv };
            let uv: [f32; 2] = std::array::from_fn(|axis| {
                (uv_of(&triangle[0])[axis] * w0 / a.z
                    + uv_of(&triangle[1])[axis] * w1 / b.z
                    + uv_of(&triangle[2])[axis] * w2 / c.z)
                    * depth
            });
            let texel = texture.sample(uv);
            if texel[3] < 26 {
                continue;
            }
            target.depth[index] = depth;
            for (pixel, value) in target.color[index * 4..index * 4 + 3].iter_mut().zip(texel) {
                *pixel = (f32::from(value) * shade) as u8;
            }
        }
    }
}

fn draw_mesh(
    target: &mut Target,
    vertices: &[ActorRigVertex],
    to_view: impl Fn(&ActorRigVertex) -> Vec3,
    texture: &Image,
) {
    for triangle in vertices.chunks_exact(3) {
        let placed = std::array::from_fn(|corner| Placed {
            view: to_view(&triangle[corner]),
            uv: triangle[corner].uv,
            back_uv: triangle[corner].back_uv,
        });
        draw(target, placed, texture);
    }
}

fn rows_to_mat(rows: [[f32; 4]; 3]) -> Mat4 {
    Mat4::from_cols(
        Vec4::new(rows[0][0], rows[1][0], rows[2][0], 0.0),
        Vec4::new(rows[0][1], rows[1][1], rows[2][1], 0.0),
        Vec4::new(rows[0][2], rows[1][2], rows[2][2], 0.0),
        Vec4::new(rows[0][3], rows[1][3], rows[2][3], 1.0),
    )
}

fn bone_mat(bone: RenderBoneTransform) -> Mat4 {
    let [x, y, z, w] = bone.rotation;
    let scale = Vec3::from_slice(&bone.axis_scale[..3]) * bone.translation_scale[3];
    Mat4::from_scale_rotation_translation(
        scale,
        bevy::math::Quat::from_xyzw(x, y, z, w).normalize(),
        Vec3::from_slice(&bone.translation_scale[..3]),
    )
}

enum Held {
    Sprite(&'static str),
    Block(&'static str),
}

fn feed(main_hand: &str, first_person: bool) -> LocalPlayerFeed {
    LocalPlayerFeed {
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
#[ignore = "writes PNG frames from local carriers"]
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
            Mat4::look_at_rh(Vec3::new(-3.0, 1.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::Y)
                * rows_to_mat(rig_world_from_actor(
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
                let vertices = render::held_sprite_vertices(
                    icon.width,
                    icon.height,
                    &icon.rgba8,
                    [0.0, 0.0, 1.0, 1.0],
                )
                .unwrap();
                let display = if third {
                    held_sprite_display(is_hand_equipped(identifier))
                } else {
                    first_person_display(
                        FirstPersonShape::Sprite {
                            mirrored_art: is_mirrored_art(identifier),
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
                let vertices =
                    render::textured_cube_vertices(super::blocks::face_rects([0.0, 0.0, 1.0, 1.0]));
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
