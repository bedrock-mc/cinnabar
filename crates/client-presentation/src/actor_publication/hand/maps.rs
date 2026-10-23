//! Retained server images and camera placements for filled maps.
use super::*;
use bevy::math::Vec3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ImageKey {
    session: u64,
    id: i64,
    revision: u64,
}

/// Each actual held slot retains only its last image, with no per-frame pixel scans.
#[derive(Default)]
pub(super) struct MapCache {
    images: [Option<(ImageKey, HandItemAtlas)>; 2],
}

impl MapCache {
    /// Reuses bytes only within the same map incarnation and authoritative session.
    pub(super) fn image(
        &mut self,
        stream: &WorldStream,
        hand: usize,
        id: Option<i64>,
    ) -> Option<HandItemAtlas> {
        let Some((id, image)) = id.and_then(|id| stream.map_image(id).map(|image| (id, image)))
        else {
            self.images[hand] = None;
            return None;
        };
        let key = ImageKey {
            session: stream.authority().actor_session_id(),
            id,
            revision: image.revision,
        };
        if self.images[hand]
            .as_ref()
            .is_none_or(|(old, _)| *old != key)
        {
            let side = protocol::MAP_IMAGE_SIDE as usize;
            let rgba8: Arc<[u8]> = image
                .pixels
                .iter()
                .enumerate()
                .flat_map(|(index, pixel)| {
                    if *pixel == 0 {
                        let odd = (index / side + index % side) & 1 != 0;
                        [0, 0, 0, if odd { 24 } else { 16 }]
                    } else {
                        pixel.to_le_bytes()
                    }
                })
                .collect();
            self.images[hand] = Some((
                key,
                HandItemAtlas {
                    width: protocol::MAP_IMAGE_SIDE as u16,
                    height: protocol::MAP_IMAGE_SIDE as u16,
                    layers: 1,
                    rgba8,
                },
            ));
        }
        self.images[hand].as_ref().map(|(_, atlas)| atlas.clone())
    }
}

/// Paper and pixels use separate submissions and bindings, preserving pack resolution.
#[derive(Default)]
pub(in super::super) struct MapLayers {
    pub(super) paper: [Option<(ActorRigSubmission, Mat4)>; 2],
    pub(super) background: Option<HandItemAtlas>,
}

/// Selects map images independently for the displayed main stack and the owner's offhand.
pub(super) fn layers(
    inputs: &HandInputs<'_>,
    equipment: &mut EquipmentRuntime,
    hand: FirstPersonHand,
    cache: &mut MapCache,
    items: &mut [Option<(FirstPersonItem, HandItemAtlas)>; 2],
) -> MapLayers {
    let background = equipment.map_background();
    let mut paper = [None, None];
    for (index, item) in [
        inputs.equipment_input.main.as_ref(),
        inputs.equipment_input.off.as_ref(),
    ]
    .into_iter()
    .enumerate()
    {
        let map =
            item.filter(|item| item.identifier.as_ref() == assets::facts::FILLED_MAP_IDENTIFIER);
        let atlas = cache.image(inputs.stream, index, map.and_then(|item| item.map_id));
        if map.is_none() {
            continue;
        }
        items[index] = None;
        let Some(atlas) = atlas.filter(|_| background.is_some()) else {
            continue;
        };
        let [paper_mesh, image_mesh] = equipment.map_meshes();
        let single = index == 1
            || inputs.equipment_input.off.as_ref().is_some_and(|off| {
                off.identifier.as_ref() == assets::facts::FILLED_MAP_IDENTIFIER
                    || off.identifier.as_ref() == assets::facts::VanillaItem::Shield.identifier()
            });
        let camera = map_camera(
            hand,
            inputs.sampling_camera.map_or(0.0, |(look, _)| look[0]),
            index == 1,
            single,
        );
        static IDENTITY: std::sync::OnceLock<Arc<[render_model::RenderBoneTransform]>> =
            std::sync::OnceLock::new();
        let identity = IDENTITY.get_or_init(|| {
            Arc::from([render_model::RenderBoneTransform {
                rotation: [0.0, 0.0, 0.0, 1.0],
                translation_scale: [0.0, 0.0, 0.0, 1.0],
                axis_scale: render_model::UNIT_AXIS_SCALE,
            }])
        });
        let mut submission = inputs.presentation.submission.clone();
        submission.input.rig = paper_mesh;
        submission.input.identity.layer = if index == 0 { 40 } else { 42 };
        submission.input.previous_bones = Arc::clone(identity);
        submission.input.current_bones = Arc::clone(identity);
        submission.world_from_actor = camera_space_rows(inputs.motion * camera);
        submission.texture_layer = render_model::HAND_ITEM_LAYER_FLAG
            | render_model::hand::HAND_MAP_BACKGROUND_FLAG
            | if index == 1 {
                render_model::HAND_OFFHAND_LAYER_FLAG
            } else {
                0
            }
            | render_model::HandItemAlphaMode::Cutout.texture_layer_flag();
        submission.route = render_model::ActorRigRoute::Compiled;
        submission.material = Default::default();
        submission.tint = u32::MAX;
        submission.uv_anim = render_model::IDENTITY_UV_ANIM;
        submission.overlay_rgba8 = 0;
        paper[index] = Some((submission.clone(), camera));
        submission.input.rig = image_mesh;
        submission.input.identity.layer += 1;
        items[index] = Some((
            FirstPersonItem {
                presentation: crate::presentation::equipment::EquipmentPresentation {
                    submission,
                    location: render_model::ActorArtworkLocation {
                        page: 0,
                        layer: 0,
                        pose_mode: assets::ActorPoseMode::CompiledLiteral,
                        multitexture: None,
                    },
                },
                camera_space: true,
                alpha_mode: render_model::HandItemAlphaMode::Blend,
                java_camera: Some(camera),
                java_normal_axis: Vec3::Z,
            },
            atlas,
        ));
    }
    MapLayers { paper, background }
}

/// Places a map's pixel coordinate plane in the native first-person camera frame.
pub(super) fn map_camera(hand: FirstPersonHand, pitch: f32, off: bool, single: bool) -> Mat4 {
    let swing = hand.swing;
    let root = swing.sqrt();
    let sin = |value: f32| sim::minecraft_sin(f64::from(value * std::f32::consts::PI)) as f32;
    let translate = |x, y, z| Mat4::from_translation(Vec3::new(x, y, z));
    let ry = Mat4::from_rotation_y;
    let rz = Mat4::from_rotation_z;
    let rx = Mat4::from_rotation_x;
    let mut camera = Mat4::IDENTITY;
    if !off {
        camera *= translate(
            if single { 0.0 } else { -0.4 * sin(root) },
            0.2 * sin(2.0 * root),
            -0.2 * sin(swing),
        );
    }
    camera *= translate(0.0, 0.04, -0.72);
    if !off {
        camera *= translate(0.0, -1.2 * (1.0 - hand.equip), 0.0);
    }
    if !single {
        let angle = (pitch / -45.0 + 1.0 + 0.1).clamp(0.0, 1.0) * std::f32::consts::PI;
        let tilt = 0.5 - 0.5 * sim::minecraft_cos(f64::from(angle)) as f32;
        camera *= translate(0.0, -0.5 * tilt, 0.0)
            * ry(std::f32::consts::FRAC_PI_2)
            * rz((-85.0 * tilt).to_radians());
        camera *= ry((-20.0 * sin(swing * swing)).to_radians())
            * rz((-20.0 * sin(root)).to_radians())
            * rx((-80.0 * sin(root)).to_radians());
    } else {
        camera *= ry(std::f32::consts::FRAC_PI_2);
    }
    let placement = camera
        * Mat4::from_scale(Vec3::splat(0.38))
        * ry(std::f32::consts::FRAC_PI_2)
        * rz(std::f32::consts::PI);
    if single {
        placement
            * translate(if off { -1.85 } else { 0.85 }, 0.0, -0.25)
            * Mat4::from_scale(Vec3::splat(1.0 / protocol::MAP_IMAGE_SIDE as f32))
    } else {
        placement
            * translate(-1.0, -1.0, 0.0)
            * Mat4::from_scale(Vec3::splat(2.0 / protocol::MAP_IMAGE_SIDE as f32))
    }
}
