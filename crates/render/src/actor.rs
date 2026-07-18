use std::sync::Arc;

use bevy::{
    math::{Mat4, Vec3, Vec4},
    prelude::Resource,
    render::extract_resource::ExtractResource,
};
use bytemuck::{Pod, Zeroable};

#[path = "actor/geometry.rs"]
mod geometry;
#[path = "actor/gpu.rs"]
pub(crate) mod gpu;
#[path = "actor/rig.rs"]
mod rig;

pub use gpu::{
    ActorDrawFrame, ActorPresentationGate, ActorPresentedFrameAck,
    MAX_ACTOR_PRESENTED_ACKNOWLEDGEMENTS,
};
pub use rig::{
    ACTOR_BONE_MATRIX_BYTES, ActorDrawManifestEntry, ActorGpuInstance, ActorRenderIdentity,
    ActorRigFrameBuilder, ActorRigGeometry, ActorRigGeometryError, ActorRigGeometrySpan,
    ActorRigRejects, ActorRigRenderFrame, ActorRigRenderInput, ActorRigRoute, ActorRigSubmission,
    ActorRigVertex, EntityRigId, MAX_ACTOR_BONE_ARENA_BYTES, MAX_ACTOR_RIG_VERTICES,
    MAX_RENDER_BONES_PER_ACTOR, RenderBoneTransform,
};

pub const MAX_RENDERED_PLAYERS: usize = 128;
pub const MAX_ACTOR_RENDER_DISTANCE_BLOCKS: f32 = 192.0;
pub const STANDARD_SKIN_SIDE: usize = 64;
pub const STANDARD_SKIN_BYTES: usize = STANDARD_SKIN_SIDE * STANDARD_SKIN_SIDE * 4;
pub const STANDARD_BIPED_VERTEX_COUNT: usize = 6 * 6 * 6;
pub const DEFAULT_SKIN_PROVENANCE: &str = "locally generated Cinnabar Default skin";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorSkinPixels {
    pub width: u32,
    pub height: u32,
    pub rgba8: Arc<[u8]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActorRenderSource {
    pub runtime_id: u64,
    pub unique_id: i64,
    pub spawn_revision: u64,
    pub movement_revision: u64,
    pub previous_position: [f32; 3],
    pub previous_pitch_degrees: f32,
    pub previous_yaw_degrees: f32,
    pub previous_head_yaw_degrees: f32,
    pub position: [f32; 3],
    pub pitch_degrees: f32,
    pub yaw_degrees: f32,
    pub head_yaw_degrees: f32,
    pub teleported: bool,
    pub render_eligible: bool,
    pub skin: Option<ActorSkinPixels>,
}

impl ActorRenderSource {
    fn is_finite(&self) -> bool {
        self.previous_position
            .iter()
            .chain(&self.position)
            .all(|value| value.is_finite())
            && self.previous_pitch_degrees.is_finite()
            && self.previous_yaw_degrees.is_finite()
            && self.previous_head_yaw_degrees.is_finite()
            && self.pitch_degrees.is_finite()
            && self.yaw_degrees.is_finite()
            && self.head_yaw_degrees.is_finite()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActorCullView {
    pub clip_from_world: Mat4,
    pub camera_position: Vec3,
    pub max_distance: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActorRenderInstance {
    pub runtime_id: u64,
    pub position: [f32; 3],
    pub pitch_radians: f32,
    pub yaw_radians: f32,
    pub head_yaw_radians: f32,
    pub skin_layer: u32,
}

#[derive(Debug, Clone, Resource, ExtractResource)]
pub struct ActorRenderFrame {
    pub instances: Arc<[ActorRenderInstance]>,
    pub skins_rgba8: Arc<[u8]>,
    pub instance_revision: u64,
    pub skin_revision: u64,
    pub rig: ActorRigRenderFrame,
}

impl Default for ActorRenderFrame {
    fn default() -> Self {
        Self {
            instances: Arc::from([]),
            skins_rgba8: Arc::from([]),
            instance_revision: 0,
            skin_revision: 0,
            rig: ActorRigRenderFrame::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Pose {
    position: [f32; 3],
    pitch_degrees: f32,
    yaw_degrees: f32,
    head_yaw_degrees: f32,
}

impl Pose {
    fn sample(source: &ActorRenderSource, alpha: f32) -> Self {
        Self {
            position: std::array::from_fn(|axis| {
                source.previous_position[axis]
                    + (source.position[axis] - source.previous_position[axis]) * alpha
            }),
            pitch_degrees: lerp_degrees(source.previous_pitch_degrees, source.pitch_degrees, alpha),
            yaw_degrees: lerp_degrees(source.previous_yaw_degrees, source.yaw_degrees, alpha),
            head_yaw_degrees: lerp_degrees(
                source.previous_head_yaw_degrees,
                source.head_yaw_degrees,
                alpha,
            ),
        }
    }
}

#[derive(Debug, Resource)]
pub struct ActorRenderScene {
    frame: ActorRenderFrame,
    rig_builder: ActorRigFrameBuilder,
}

impl Default for ActorRenderScene {
    fn default() -> Self {
        Self {
            frame: ActorRenderFrame::default(),
            rig_builder: ActorRigFrameBuilder::new([])
                .expect("authored diagnostic actor geometry is valid"),
        }
    }
}

impl ActorRenderScene {
    pub fn with_runtime_entity_assets(
        assets: &assets::RuntimeEntityAssets,
    ) -> Result<Self, ActorRigGeometryError> {
        Ok(Self {
            frame: ActorRenderFrame::default(),
            rig_builder: ActorRigFrameBuilder::from_runtime_assets(assets)?,
        })
    }

    pub fn replace_runtime_entity_assets(
        &mut self,
        assets: &assets::RuntimeEntityAssets,
    ) -> Result<(), ActorRigGeometryError> {
        let replacement = ActorRigFrameBuilder::from_runtime_assets(assets)?;
        self.rig_builder = replacement;
        self.frame = ActorRenderFrame::default();
        Ok(())
    }

    pub fn reset(&mut self) {
        if !self.frame.instances.is_empty() {
            self.frame.instance_revision = self.frame.instance_revision.wrapping_add(1);
            self.frame.instances = Arc::from([]);
        }
        if !self.frame.skins_rgba8.is_empty() {
            self.frame.skin_revision = self.frame.skin_revision.wrapping_add(1);
            self.frame.skins_rgba8 = Arc::from([]);
        }
        self.frame.rig = self.rig_builder.build(0.0, None, []);
    }

    pub fn update(
        &mut self,
        partial_tick: f32,
        view: Option<ActorCullView>,
        sources: impl IntoIterator<Item = ActorRenderSource>,
    ) -> &ActorRenderFrame {
        let partial_tick = if partial_tick.is_finite() {
            partial_tick.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let mut sources = sources
            .into_iter()
            .filter(|source| source.render_eligible)
            .filter(ActorRenderSource::is_finite)
            .collect::<Vec<_>>();
        sources.sort_unstable_by_key(|source| source.runtime_id);
        sources.dedup_by_key(|source| source.runtime_id);
        let visible = sources
            .into_iter()
            .filter_map(|source| {
                let pose = Pose::sample(&source, partial_tick);
                actor_is_visible(&pose, view).then_some((source, pose))
            })
            .take(MAX_RENDERED_PLAYERS)
            .collect::<Vec<_>>();

        let mut instances = Vec::with_capacity(visible.len());
        let mut skins = Vec::with_capacity(visible.len() * STANDARD_SKIN_BYTES);
        let mut rig_submissions = Vec::with_capacity(visible.len());
        for (source, pose) in visible {
            let skin_layer = u32::try_from(instances.len()).expect("bounded actor layer count");
            instances.push(ActorRenderInstance {
                runtime_id: source.runtime_id,
                position: pose.position,
                pitch_radians: wrap_degrees(pose.pitch_degrees).to_radians(),
                yaw_radians: wrap_degrees(pose.yaw_degrees).to_radians(),
                head_yaw_radians: wrap_degrees(pose.head_yaw_degrees).to_radians(),
                skin_layer,
            });
            let head_rotation = quaternion_from_euler_degrees([
                wrap_degrees(pose.pitch_degrees),
                wrap_degrees(pose.head_yaw_degrees - pose.yaw_degrees),
                0.0,
            ]);
            let pivots = [
                [0.0, 1.5, 0.0],
                [0.0, 1.5, 0.0],
                [-0.3125, 1.375, 0.0],
                [-0.11875, 0.75, 0.0],
                [0.3125, 1.375, 0.0],
                [0.11875, 0.75, 0.0],
            ];
            let bones = pivots.map(|pivot| RenderBoneTransform {
                rotation: [0.0, 0.0, 0.0, 1.0],
                translation_scale: [pivot[0], pivot[1], pivot[2], 1.0],
            });
            let mut posed_bones = bones;
            posed_bones[0].rotation = head_rotation;
            let yaw = wrap_degrees(pose.yaw_degrees).to_radians();
            let (sine, cosine) = yaw.sin_cos();
            rig_submissions.push(ActorRigSubmission {
                input: ActorRigRenderInput {
                    identity: ActorRenderIdentity {
                        session_id: 0,
                        dimension: 0,
                        runtime_id: source.runtime_id,
                        spawn_revision: source.spawn_revision,
                        ingress_sequence: source.movement_revision,
                        source_tick: None,
                        movement_revision: source.movement_revision,
                        pose_generation: source.movement_revision,
                    },
                    rig: EntityRigId(u32::MAX),
                    previous_bones: Arc::from(posed_bones),
                    current_bones: Arc::from(posed_bones),
                    completed_tick: source.movement_revision,
                    reset_generation: source.spawn_revision.max(1),
                },
                world_from_actor: [
                    [cosine, 0.0, sine, pose.position[0]],
                    [0.0, 1.0, 0.0, pose.position[1]],
                    [-sine, 0.0, cosine, pose.position[2]],
                ],
                texture_layer: skin_layer,
                route: ActorRigRoute::Diagnostic,
            });
            skins.extend_from_slice(&normalize_skin(source.skin.as_ref()));
        }

        if self.frame.instances.as_ref() != instances.as_slice() {
            self.frame.instance_revision = self.frame.instance_revision.wrapping_add(1);
            self.frame.instances = Arc::from(instances);
        }
        if self.frame.skins_rgba8.as_ref() != skins.as_slice() {
            self.frame.skin_revision = self.frame.skin_revision.wrapping_add(1);
            self.frame.skins_rgba8 = Arc::from(skins);
        }
        self.frame.rig = self.rig_builder.build(1.0, None, rig_submissions);
        &self.frame
    }

    pub fn update_rigs(
        &mut self,
        partial_tick: f32,
        view: Option<ActorCullView>,
        submissions: impl IntoIterator<Item = ActorRigSubmission>,
        skins_rgba8: Arc<[u8]>,
    ) -> &ActorRenderFrame {
        let rig = self.rig_builder.build(partial_tick, view, submissions);
        let expected_skin_bytes = rig.instances.len().saturating_mul(STANDARD_SKIN_BYTES);
        let invalid_skin_layer = rig
            .instances
            .iter()
            .any(|instance| instance.texture_layer as usize >= rig.instances.len());
        if skins_rgba8.len() != expected_skin_bytes || invalid_skin_layer {
            let rejects = rig.rejects;
            self.frame.rig = ActorRigRenderFrame {
                geometry_revision: rig.geometry_revision,
                geometry_vertices: rig.geometry_vertices,
                geometry_spans: rig.geometry_spans,
                frame_generation: rig.frame_generation,
                rejects: ActorRigRejects {
                    invalid_geometry: rejects.invalid_geometry.saturating_add(1),
                    ..rejects
                },
                ..ActorRigRenderFrame::default()
            };
            self.frame.instances = Arc::from([]);
            self.frame.skins_rgba8 = Arc::from([]);
            self.frame.instance_revision = self.frame.instance_revision.wrapping_add(1);
            self.frame.skin_revision = self.frame.skin_revision.wrapping_add(1);
            return &self.frame;
        }
        let compatibility_instances = rig
            .instances
            .iter()
            .zip(rig.manifest.iter())
            .map(|(instance, manifest)| ActorRenderInstance {
                runtime_id: manifest.identity.runtime_id,
                position: [
                    instance.world_from_actor[0][3],
                    instance.world_from_actor[1][3],
                    instance.world_from_actor[2][3],
                ],
                pitch_radians: 0.0,
                yaw_radians: 0.0,
                head_yaw_radians: 0.0,
                skin_layer: instance.texture_layer,
            })
            .collect::<Vec<_>>();
        if self.frame.instances.as_ref() != compatibility_instances.as_slice() {
            self.frame.instance_revision = self.frame.instance_revision.wrapping_add(1);
            self.frame.instances = Arc::from(compatibility_instances);
        }
        if self.frame.skins_rgba8 != skins_rgba8 {
            self.frame.skin_revision = self.frame.skin_revision.wrapping_add(1);
            self.frame.skins_rgba8 = skins_rgba8;
        }
        self.frame.rig = rig;
        &self.frame
    }

    #[must_use]
    pub fn frame(&self) -> &ActorRenderFrame {
        &self.frame
    }
}

fn actor_is_visible(pose: &Pose, view: Option<ActorCullView>) -> bool {
    let Some(view) = view.filter(|view| {
        view.clip_from_world.is_finite()
            && view.camera_position.is_finite()
            && view.max_distance.is_finite()
            && view.max_distance > 0.0
    }) else {
        return true;
    };
    let feet = Vec3::from_array(pose.position);
    let center = feet + Vec3::Y;
    if center.distance_squared(view.camera_position) > view.max_distance * view.max_distance {
        return false;
    }

    const HALF_WIDTH: f32 = 0.5;
    const HEIGHT: f32 = 2.0;
    let corners = [
        Vec3::new(-HALF_WIDTH, 0.0, -HALF_WIDTH),
        Vec3::new(HALF_WIDTH, 0.0, -HALF_WIDTH),
        Vec3::new(-HALF_WIDTH, HEIGHT, -HALF_WIDTH),
        Vec3::new(HALF_WIDTH, HEIGHT, -HALF_WIDTH),
        Vec3::new(-HALF_WIDTH, 0.0, HALF_WIDTH),
        Vec3::new(HALF_WIDTH, 0.0, HALF_WIDTH),
        Vec3::new(-HALF_WIDTH, HEIGHT, HALF_WIDTH),
        Vec3::new(HALF_WIDTH, HEIGHT, HALF_WIDTH),
    ]
    .map(|offset| view.clip_from_world * (feet + offset).extend(1.0));

    !outside_clip_plane(&corners, |clip| clip.x < -clip.w)
        && !outside_clip_plane(&corners, |clip| clip.x > clip.w)
        && !outside_clip_plane(&corners, |clip| clip.y < -clip.w)
        && !outside_clip_plane(&corners, |clip| clip.y > clip.w)
        && !outside_clip_plane(&corners, |clip| clip.z < 0.0)
        && !outside_clip_plane(&corners, |clip| clip.z > clip.w)
        && !outside_clip_plane(&corners, |clip| clip.w <= 0.0)
}

fn outside_clip_plane(corners: &[Vec4; 8], outside: impl Fn(&Vec4) -> bool) -> bool {
    corners.iter().all(outside)
}

fn lerp_degrees(start: f32, end: f32, alpha: f32) -> f32 {
    wrap_degrees(start + wrap_degrees(end - start) * alpha)
}

fn wrap_degrees(degrees: f32) -> f32 {
    (degrees + 180.0).rem_euclid(360.0) - 180.0
}

fn quaternion_from_euler_degrees(rotation: [f32; 3]) -> [f32; 4] {
    let [x, y, z] = rotation.map(|value| value.to_radians() * 0.5);
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    [
        sx * cy * cz - cx * sy * sz,
        cx * sy * cz + sx * cy * sz,
        cx * cy * sz - sx * sy * cz,
        cx * cy * cz + sx * sy * sz,
    ]
}

fn normalize_skin(skin: Option<&ActorSkinPixels>) -> Vec<u8> {
    let Some(skin) = skin else {
        return generated_default_skin();
    };
    if skin.width != skin.height || !matches!(skin.width, 64 | 128 | 256) {
        return generated_default_skin();
    }
    let side = usize::try_from(skin.width).expect("bounded standard skin side");
    if skin.rgba8.len() != side * side * 4 {
        return generated_default_skin();
    }
    if side == STANDARD_SKIN_SIDE {
        return skin.rgba8.to_vec();
    }
    let mut normalized = vec![0; STANDARD_SKIN_BYTES];
    for y in 0..STANDARD_SKIN_SIDE {
        for x in 0..STANDARD_SKIN_SIDE {
            let source_x = x * side / STANDARD_SKIN_SIDE;
            let source_y = y * side / STANDARD_SKIN_SIDE;
            let source = (source_y * side + source_x) * 4;
            let target = (y * STANDARD_SKIN_SIDE + x) * 4;
            normalized[target..target + 4].copy_from_slice(&skin.rgba8[source..source + 4]);
        }
    }
    normalized
}

fn generated_default_skin() -> Vec<u8> {
    let skin_tone = [198, 134, 91, 255];
    let mut rgba8 = skin_tone.repeat(STANDARD_SKIN_SIDE * STANDARD_SKIN_SIDE);
    fill_rect(&mut rgba8, 16, 16, 24, 16, [42, 91, 99, 255]);
    fill_rect(&mut rgba8, 0, 16, 16, 16, [47, 54, 67, 255]);
    fill_rect(&mut rgba8, 16, 48, 16, 16, [47, 54, 67, 255]);
    fill_rect(&mut rgba8, 8, 8, 8, 8, [112, 72, 48, 255]);
    rgba8
}

fn fill_rect(rgba8: &mut [u8], x: usize, y: usize, width: usize, height: usize, color: [u8; 4]) {
    for py in y..y + height {
        for px in x..x + width {
            let offset = (py * STANDARD_SKIN_SIDE + px) * 4;
            rgba8[offset..offset + 4].copy_from_slice(&color);
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct ActorVertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub part: u32,
}

#[derive(Clone, Copy)]
struct Cuboid {
    min: [f32; 3],
    max: [f32; 3],
    uv_origin: [f32; 2],
    dimensions: [f32; 3],
}

#[must_use]
pub fn standard_biped_vertices() -> Vec<ActorVertex> {
    const P: f32 = 1.0 / 16.0;
    let cuboids = [
        Cuboid {
            min: [-4.0 * P, 24.0 * P, -4.0 * P],
            max: [4.0 * P, 32.0 * P, 4.0 * P],
            uv_origin: [0.0, 0.0],
            dimensions: [8.0, 8.0, 8.0],
        },
        Cuboid {
            min: [-4.0 * P, 12.0 * P, -2.0 * P],
            max: [4.0 * P, 24.0 * P, 2.0 * P],
            uv_origin: [16.0, 16.0],
            dimensions: [8.0, 12.0, 4.0],
        },
        Cuboid {
            min: [-8.0 * P, 12.0 * P, -2.0 * P],
            max: [-4.0 * P, 24.0 * P, 2.0 * P],
            uv_origin: [40.0, 16.0],
            dimensions: [4.0, 12.0, 4.0],
        },
        Cuboid {
            min: [4.0 * P, 12.0 * P, -2.0 * P],
            max: [8.0 * P, 24.0 * P, 2.0 * P],
            uv_origin: [32.0, 48.0],
            dimensions: [4.0, 12.0, 4.0],
        },
        Cuboid {
            min: [-4.0 * P, 0.0, -2.0 * P],
            max: [0.0, 12.0 * P, 2.0 * P],
            uv_origin: [0.0, 16.0],
            dimensions: [4.0, 12.0, 4.0],
        },
        Cuboid {
            min: [0.0, 0.0, -2.0 * P],
            max: [4.0 * P, 12.0 * P, 2.0 * P],
            uv_origin: [16.0, 48.0],
            dimensions: [4.0, 12.0, 4.0],
        },
    ];
    let mut vertices = Vec::with_capacity(STANDARD_BIPED_VERTEX_COUNT);
    for (part, cuboid) in cuboids.into_iter().enumerate() {
        append_cuboid(&mut vertices, cuboid, part as u32);
    }
    vertices
}

fn append_cuboid(vertices: &mut Vec<ActorVertex>, cuboid: Cuboid, part: u32) {
    let [x0, y0, z0] = cuboid.min;
    let [x1, y1, z1] = cuboid.max;
    let [u, v] = cuboid.uv_origin;
    let [dx, dy, dz] = cuboid.dimensions;
    let faces = [
        (
            [[x1, y0, z0], [x1, y0, z1], [x1, y1, z1], [x1, y1, z0]],
            [u, v + dz, dz, dy],
            true,
        ),
        (
            [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]],
            [u + dz, v + dz, dx, dy],
            false,
        ),
        (
            [[x0, y0, z1], [x0, y0, z0], [x0, y1, z0], [x0, y1, z1]],
            [u + dz + dx, v + dz, dz, dy],
            true,
        ),
        (
            [[x1, y0, z0], [x0, y0, z0], [x0, y1, z0], [x1, y1, z0]],
            [u + dz + dx + dz, v + dz, dx, dy],
            false,
        ),
        (
            [[x0, y1, z1], [x1, y1, z1], [x1, y1, z0], [x0, y1, z0]],
            [u + dz, v, dx, dz],
            false,
        ),
        (
            [[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]],
            [u + dz + dx, v, dx, dz],
            false,
        ),
    ];
    for (positions, [face_u, face_v, face_width, face_height], reverse_winding) in faces {
        let u0 = face_u / 64.0;
        let v0 = face_v / 64.0;
        let u1 = (face_u + face_width) / 64.0;
        let v1 = (face_v + face_height) / 64.0;
        let uvs = [[u0, v1], [u1, v1], [u1, v0], [u0, v0]];
        let indices = if reverse_winding {
            [0, 2, 1, 0, 3, 2]
        } else {
            [0, 1, 2, 0, 2, 3]
        };
        for index in indices {
            vertices.push(ActorVertex {
                position: positions[index],
                uv: uvs[index],
                part,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bevy::math::{Mat4, Vec3};

    use super::{
        ActorCullView, ActorRenderScene, ActorRenderSource, ActorSkinPixels,
        DEFAULT_SKIN_PROVENANCE, MAX_RENDERED_PLAYERS, STANDARD_BIPED_VERTEX_COUNT,
        standard_biped_vertices,
    };

    fn source(runtime_id: u64, x: f32, yaw_degrees: f32) -> ActorRenderSource {
        ActorRenderSource {
            runtime_id,
            unique_id: i64::try_from(runtime_id).unwrap_or(i64::MAX),
            spawn_revision: 1,
            movement_revision: 0,
            previous_position: [x, 64.0, 0.0],
            previous_pitch_degrees: 0.0,
            previous_yaw_degrees: yaw_degrees,
            previous_head_yaw_degrees: yaw_degrees,
            position: [x, 64.0, 0.0],
            pitch_degrees: 0.0,
            yaw_degrees,
            head_yaw_degrees: yaw_degrees,
            teleported: false,
            render_eligible: true,
            skin: None,
        }
    }

    fn tick_source(
        runtime_id: u64,
        previous_x: f32,
        current_x: f32,
        previous_yaw: f32,
        current_yaw: f32,
    ) -> ActorRenderSource {
        ActorRenderSource {
            previous_position: [previous_x, 64.0, 0.0],
            previous_pitch_degrees: 0.0,
            previous_yaw_degrees: previous_yaw,
            previous_head_yaw_degrees: previous_yaw,
            ..source(runtime_id, current_x, current_yaw)
        }
    }

    fn broad_view(max_distance: f32) -> ActorCullView {
        ActorCullView {
            clip_from_world: Mat4::from_scale(Vec3::splat(0.001)),
            camera_position: Vec3::new(0.0, 65.0, 0.0),
            max_distance,
        }
    }

    #[test]
    fn frame_interpolation_samples_adjacent_actor_ticks() {
        let mut scene = ActorRenderScene::default();
        let frame = scene.update(0.5, None, [tick_source(7, 3.0, 6.0, 0.0, 0.0)]);

        assert_eq!(frame.instances.len(), 1);
        assert!((frame.instances[0].position[0] - 4.5).abs() < 1e-5);
    }

    #[test]
    fn frame_republication_changes_only_with_partial_tick() {
        let source = tick_source(7, 3.0, 6.0, 0.0, 0.0);
        let mut scene = ActorRenderScene::default();
        assert_eq!(
            scene.update(0.0, None, [source.clone()]).instances[0].position[0],
            3.0
        );
        assert_eq!(
            scene.update(0.5, None, [source.clone()]).instances[0].position[0],
            4.5
        );
        assert_eq!(
            scene.update(1.0, None, [source]).instances[0].position[0],
            6.0
        );
    }

    #[test]
    fn frame_angles_take_the_shortest_path_between_tick_poses() {
        let mut scene = ActorRenderScene::default();
        let frame = scene.update(0.5, None, [tick_source(7, 0.0, 0.0, 350.0, 10.0)]);

        assert!(frame.instances[0].yaw_radians.abs() < 1e-5);
    }

    #[test]
    fn teleport_equal_endpoints_never_cross_the_old_position() {
        let mut scene = ActorRenderScene::default();
        for alpha in [0.0, 0.5, 1.0] {
            let frame = scene.update(alpha, None, [tick_source(7, 100.0, 100.0, 90.0, 90.0)]);
            assert_eq!(frame.instances[0].position[0], 100.0);
        }
    }

    #[test]
    fn actor_culling_rejects_wholly_outside_frustum_but_keeps_edge_intersections() {
        let view = ActorCullView {
            clip_from_world: Mat4::from_translation(Vec3::new(0.0, -64.0, 0.0)),
            camera_position: Vec3::new(0.0, 65.0, 0.0),
            max_distance: 192.0,
        };
        let mut scene = ActorRenderScene::default();
        let frame = scene.update(
            1.0,
            Some(view),
            [
                tick_source(1, 0.0, 0.0, 0.0, 0.0),
                tick_source(2, 1.4, 1.4, 0.0, 0.0),
                tick_source(3, 3.0, 3.0, 0.0, 0.0),
            ],
        );

        assert_eq!(
            frame
                .instances
                .iter()
                .map(|actor| actor.runtime_id)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    #[test]
    fn actor_culling_rejects_positions_beyond_the_distance_cap() {
        let mut scene = ActorRenderScene::default();
        let frame = scene.update(
            1.0,
            Some(broad_view(192.0)),
            [
                tick_source(1, 191.0, 191.0, 0.0, 0.0),
                tick_source(2, 193.0, 193.0, 0.0, 0.0),
            ],
        );

        assert_eq!(frame.instances.len(), 1);
        assert_eq!(frame.instances[0].runtime_id, 1);
    }

    #[test]
    fn culling_occurs_before_the_visible_actor_cap() {
        let mut sources = (0..u64::try_from(MAX_RENDERED_PLAYERS).unwrap())
            .map(|id| tick_source(id, 500.0, 500.0, 0.0, 0.0))
            .collect::<Vec<_>>();
        sources.push(tick_source(999, 0.0, 0.0, 0.0, 0.0));
        let mut scene = ActorRenderScene::default();
        let frame = scene.update(1.0, Some(broad_view(192.0)), sources);

        assert_eq!(frame.instances.len(), 1);
        assert_eq!(frame.instances[0].runtime_id, 999);
    }

    #[test]
    fn render_ineligible_actor_is_rejected_when_camera_is_inside_its_aabb() {
        let mut hidden = source(7, 0.0, 0.0);
        hidden.render_eligible = false;
        let mut scene = ActorRenderScene::default();

        let frame = scene.update(1.0, Some(broad_view(192.0)), [hidden, source(8, 0.0, 0.0)]);

        assert_eq!(
            frame
                .instances
                .iter()
                .map(|actor| actor.runtime_id)
                .collect::<Vec<_>>(),
            vec![8]
        );
    }

    #[test]
    fn render_ineligible_actors_do_not_consume_the_visible_actor_cap() {
        let mut sources = (0..u64::try_from(MAX_RENDERED_PLAYERS).unwrap())
            .map(|id| {
                let mut actor = source(id, 0.0, 0.0);
                actor.render_eligible = false;
                actor
            })
            .collect::<Vec<_>>();
        sources.push(source(999, 0.0, 0.0));
        let mut scene = ActorRenderScene::default();

        let frame = scene.update(1.0, Some(broad_view(192.0)), sources);

        assert_eq!(frame.instances.len(), 1);
        assert_eq!(frame.instances[0].runtime_id, 999);
    }

    #[test]
    fn scene_reset_clears_the_published_frame() {
        let mut scene = ActorRenderScene::default();
        scene.update(1.0, None, [source(7, 10.0, 0.0)]);
        scene.reset();
        assert!(scene.frame().instances.is_empty());
    }

    #[test]
    fn scene_rejects_non_finite_sources_and_truncates_stably() {
        let mut sources = (0..u64::try_from(MAX_RENDERED_PLAYERS + 2).unwrap())
            .rev()
            .map(|id| source(id, id as f32, 0.0))
            .collect::<Vec<_>>();
        sources.push(source(u64::MAX, f32::NAN, 0.0));
        let mut scene = ActorRenderScene::default();
        let frame = scene.update(1.0, None, sources);

        assert_eq!(frame.instances.len(), MAX_RENDERED_PLAYERS);
        assert_eq!(frame.instances.first().unwrap().runtime_id, 0);
        assert_eq!(
            frame.instances.last().unwrap().runtime_id,
            u64::try_from(MAX_RENDERED_PLAYERS - 1).unwrap()
        );
    }

    #[test]
    fn high_resolution_standard_skin_is_nearest_sampled_and_invalid_skin_uses_authored_default() {
        let mut rgba8 = vec![0; 128 * 128 * 4];
        rgba8[0..4].copy_from_slice(&[1, 2, 3, 255]);
        let valid = ActorSkinPixels {
            width: 128,
            height: 128,
            rgba8: Arc::from(rgba8),
        };
        let invalid = ActorSkinPixels {
            width: 64,
            height: 64,
            rgba8: Arc::from([0_u8; 4]),
        };
        let mut first = source(1, 0.0, 0.0);
        first.skin = Some(valid);
        let mut second = source(2, 0.0, 0.0);
        second.skin = Some(invalid);
        let mut scene = ActorRenderScene::default();
        let frame = scene.update(1.0, None, [first, second]);

        assert_eq!(&frame.skins_rgba8[0..4], &[1, 2, 3, 255]);
        assert_eq!(frame.skins_rgba8.len(), 2 * 64 * 64 * 4);
        assert_eq!(
            DEFAULT_SKIN_PROVENANCE,
            "locally generated Cinnabar Default skin"
        );
        let default = &frame.skins_rgba8[64 * 64 * 4..];
        assert!(
            default
                .chunks_exact(4)
                .any(|pixel| pixel == [42, 91, 99, 255])
        );
        assert!(
            default
                .chunks_exact(4)
                .any(|pixel| pixel == [198, 134, 91, 255])
        );
    }

    #[test]
    fn standard_biped_is_six_cuboids_with_a_complete_base_layer_uv_mesh() {
        let vertices = standard_biped_vertices();
        assert_eq!(vertices.len(), STANDARD_BIPED_VERTEX_COUNT);
        assert_eq!(STANDARD_BIPED_VERTEX_COUNT, 6 * 6 * 6);
        assert!(vertices.iter().all(|vertex| {
            vertex.position.iter().all(|value| value.is_finite())
                && vertex.uv.iter().all(|value| (0.0..=1.0).contains(value))
        }));
        let min_y = vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::INFINITY, f32::min);
        let max_y = vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::NEG_INFINITY, f32::max);
        assert_eq!([min_y, max_y], [0.0, 2.0]);
    }

    #[test]
    fn standard_biped_faces_have_outward_consistent_winding() {
        let vertices = standard_biped_vertices();
        for cuboid in vertices.chunks_exact(6 * 6) {
            let min = cuboid
                .iter()
                .fold(Vec3::splat(f32::INFINITY), |min, vertex| {
                    min.min(Vec3::from_array(vertex.position))
                });
            let max = cuboid
                .iter()
                .fold(Vec3::splat(f32::NEG_INFINITY), |max, vertex| {
                    max.max(Vec3::from_array(vertex.position))
                });
            let center = (min + max) * 0.5;

            for face in cuboid.chunks_exact(6) {
                let face_center = face
                    .iter()
                    .map(|vertex| Vec3::from_array(vertex.position))
                    .sum::<Vec3>()
                    / face.len() as f32;
                for triangle in face.chunks_exact(3) {
                    let first = Vec3::from_array(triangle[0].position);
                    let second = Vec3::from_array(triangle[1].position);
                    let third = Vec3::from_array(triangle[2].position);
                    let normal = (second - first).cross(third - first);

                    assert!(
                        normal.dot(face_center - center) > 0.0,
                        "part {} has an inward or degenerate face: {face:?}",
                        face[0].part
                    );
                }
            }
        }
    }
}
