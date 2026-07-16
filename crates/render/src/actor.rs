use std::{collections::BTreeMap, sync::Arc};

use bevy::{prelude::Resource, render::extract_resource::ExtractResource};
use bytemuck::{Pod, Zeroable};

pub const MAX_RENDERED_PLAYERS: usize = 128;
pub const ACTOR_INTERPOLATION_DELAY_SECONDS: f64 = 0.1;
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
    pub position: [f32; 3],
    pub pitch_degrees: f32,
    pub yaw_degrees: f32,
    pub head_yaw_degrees: f32,
    pub teleported: bool,
    pub skin: Option<ActorSkinPixels>,
}

impl ActorRenderSource {
    fn is_finite(&self) -> bool {
        self.position.iter().all(|value| value.is_finite())
            && self.pitch_degrees.is_finite()
            && self.yaw_degrees.is_finite()
            && self.head_yaw_degrees.is_finite()
    }
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
}

impl Default for ActorRenderFrame {
    fn default() -> Self {
        Self {
            instances: Arc::from([]),
            skins_rgba8: Arc::from([]),
            instance_revision: 0,
            skin_revision: 0,
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

impl From<&ActorRenderSource> for Pose {
    fn from(source: &ActorRenderSource) -> Self {
        Self {
            position: source.position,
            pitch_degrees: source.pitch_degrees,
            yaw_degrees: source.yaw_degrees,
            head_yaw_degrees: source.head_yaw_degrees,
        }
    }
}

#[derive(Debug, Clone)]
struct TimedPose {
    seconds: f64,
    pose: Pose,
}

#[derive(Debug, Clone)]
struct ActorTrack {
    previous: TimedPose,
    current: TimedPose,
    skin: Option<ActorSkinPixels>,
}

#[derive(Debug, Default, Resource)]
pub struct ActorRenderScene {
    tracks: BTreeMap<u64, ActorTrack>,
    frame: ActorRenderFrame,
}

impl ActorRenderScene {
    pub fn reset(&mut self) {
        self.tracks.clear();
        if !self.frame.instances.is_empty() {
            self.frame.instance_revision = self.frame.instance_revision.wrapping_add(1);
            self.frame.instances = Arc::from([]);
        }
        if !self.frame.skins_rgba8.is_empty() {
            self.frame.skin_revision = self.frame.skin_revision.wrapping_add(1);
            self.frame.skins_rgba8 = Arc::from([]);
        }
    }

    pub fn update(
        &mut self,
        now_seconds: f64,
        sources: impl IntoIterator<Item = ActorRenderSource>,
    ) -> &ActorRenderFrame {
        let now_seconds = if now_seconds.is_finite() {
            now_seconds.max(0.0)
        } else {
            0.0
        };
        let mut sources = sources
            .into_iter()
            .filter(ActorRenderSource::is_finite)
            .collect::<Vec<_>>();
        sources.sort_unstable_by_key(|source| source.runtime_id);
        sources.dedup_by_key(|source| source.runtime_id);
        sources.truncate(MAX_RENDERED_PLAYERS);

        let active = sources
            .iter()
            .map(|source| source.runtime_id)
            .collect::<std::collections::BTreeSet<_>>();
        self.tracks
            .retain(|runtime_id, _| active.contains(runtime_id));
        for source in &sources {
            let pose = Pose::from(source);
            match self.tracks.get_mut(&source.runtime_id) {
                Some(track) => {
                    if source.teleported {
                        let timed = TimedPose {
                            seconds: now_seconds,
                            pose,
                        };
                        track.previous = timed.clone();
                        track.current = timed;
                    } else if track.current.pose != pose {
                        track.previous = track.current.clone();
                        track.current = TimedPose {
                            seconds: now_seconds,
                            pose,
                        };
                    }
                    track.skin = source.skin.clone();
                }
                None => {
                    let timed = TimedPose {
                        seconds: now_seconds,
                        pose,
                    };
                    self.tracks.insert(
                        source.runtime_id,
                        ActorTrack {
                            previous: timed.clone(),
                            current: timed,
                            skin: source.skin.clone(),
                        },
                    );
                }
            }
        }

        let sample_seconds = now_seconds - ACTOR_INTERPOLATION_DELAY_SECONDS;
        let mut instances = Vec::with_capacity(self.tracks.len());
        let mut skins = Vec::with_capacity(self.tracks.len() * STANDARD_SKIN_BYTES);
        for (&runtime_id, track) in &self.tracks {
            let pose = sample_pose(track, sample_seconds);
            let skin_layer = u32::try_from(instances.len()).expect("bounded actor layer count");
            instances.push(ActorRenderInstance {
                runtime_id,
                position: pose.position,
                pitch_radians: wrap_degrees(pose.pitch_degrees).to_radians(),
                yaw_radians: wrap_degrees(pose.yaw_degrees).to_radians(),
                head_yaw_radians: wrap_degrees(pose.head_yaw_degrees).to_radians(),
                skin_layer,
            });
            skins.extend_from_slice(&normalize_skin(track.skin.as_ref()));
        }

        if self.frame.instances.as_ref() != instances.as_slice() {
            self.frame.instance_revision = self.frame.instance_revision.wrapping_add(1);
            self.frame.instances = Arc::from(instances);
        }
        if self.frame.skins_rgba8.as_ref() != skins.as_slice() {
            self.frame.skin_revision = self.frame.skin_revision.wrapping_add(1);
            self.frame.skins_rgba8 = Arc::from(skins);
        }
        &self.frame
    }

    #[must_use]
    pub fn frame(&self) -> &ActorRenderFrame {
        &self.frame
    }
}

fn sample_pose(track: &ActorTrack, seconds: f64) -> Pose {
    let duration = track.current.seconds - track.previous.seconds;
    if duration <= f64::EPSILON {
        return track.current.pose.clone();
    }
    let alpha = ((seconds - track.previous.seconds) / duration).clamp(0.0, 1.0) as f32;
    Pose {
        position: std::array::from_fn(|axis| {
            track.previous.pose.position[axis]
                + (track.current.pose.position[axis] - track.previous.pose.position[axis]) * alpha
        }),
        pitch_degrees: lerp_degrees(
            track.previous.pose.pitch_degrees,
            track.current.pose.pitch_degrees,
            alpha,
        ),
        yaw_degrees: lerp_degrees(
            track.previous.pose.yaw_degrees,
            track.current.pose.yaw_degrees,
            alpha,
        ),
        head_yaw_degrees: lerp_degrees(
            track.previous.pose.head_yaw_degrees,
            track.current.pose.head_yaw_degrees,
            alpha,
        ),
    }
}

fn lerp_degrees(start: f32, end: f32, alpha: f32) -> f32 {
    wrap_degrees(start + wrap_degrees(end - start) * alpha)
}

fn wrap_degrees(degrees: f32) -> f32 {
    (degrees + 180.0).rem_euclid(360.0) - 180.0
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
        ),
        (
            [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]],
            [u + dz, v + dz, dx, dy],
        ),
        (
            [[x0, y0, z1], [x0, y0, z0], [x0, y1, z0], [x0, y1, z1]],
            [u + dz + dx, v + dz, dz, dy],
        ),
        (
            [[x1, y0, z0], [x0, y0, z0], [x0, y1, z0], [x1, y1, z0]],
            [u + dz + dx + dz, v + dz, dx, dy],
        ),
        (
            [[x0, y1, z1], [x1, y1, z1], [x1, y1, z0], [x0, y1, z0]],
            [u + dz, v, dx, dz],
        ),
        (
            [[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]],
            [u + dz + dx, v, dx, dz],
        ),
    ];
    for (positions, [face_u, face_v, face_width, face_height]) in faces {
        let u0 = face_u / 64.0;
        let v0 = face_v / 64.0;
        let u1 = (face_u + face_width) / 64.0;
        let v1 = (face_v + face_height) / 64.0;
        let uvs = [[u0, v1], [u1, v1], [u1, v0], [u0, v0]];
        for index in [0, 1, 2, 0, 2, 3] {
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

    use super::{
        ActorRenderScene, ActorRenderSource, ActorSkinPixels, DEFAULT_SKIN_PROVENANCE,
        MAX_RENDERED_PLAYERS, STANDARD_BIPED_VERTEX_COUNT, standard_biped_vertices,
    };

    fn source(runtime_id: u64, x: f32, yaw_degrees: f32) -> ActorRenderSource {
        ActorRenderSource {
            runtime_id,
            position: [x, 64.0, 0.0],
            pitch_degrees: 0.0,
            yaw_degrees,
            head_yaw_degrees: yaw_degrees,
            teleported: false,
            skin: None,
        }
    }

    #[test]
    fn scene_interpolates_position_and_shortest_angles_at_a_time_delay() {
        let mut scene = ActorRenderScene::default();
        scene.update(0.0, [source(7, 0.0, 350.0)]);
        scene.update(0.1, [source(7, 10.0, 10.0)]);
        let frame = scene.update(0.15, [source(7, 10.0, 10.0)]);

        assert_eq!(frame.instances.len(), 1);
        assert!((frame.instances[0].position[0] - 5.0).abs() < 1e-5);
        assert!(frame.instances[0].yaw_radians.abs() < 1e-5);
    }

    #[test]
    fn teleport_replaces_interpolation_endpoints() {
        let mut scene = ActorRenderScene::default();
        scene.update(0.0, [source(7, 0.0, 0.0)]);
        let mut teleported = source(7, 100.0, 90.0);
        teleported.teleported = true;
        let frame = scene.update(0.05, [teleported]);

        assert_eq!(frame.instances[0].position[0], 100.0);
        assert!((frame.instances[0].yaw_radians - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    }

    #[test]
    fn scene_reset_discards_same_runtime_history() {
        let mut scene = ActorRenderScene::default();
        scene.update(0.0, [source(7, 0.0, 0.0)]);
        scene.update(0.1, [source(7, 10.0, 0.0)]);
        scene.reset();
        let frame = scene.update(0.15, [source(7, 100.0, 0.0)]);

        assert_eq!(frame.instances[0].position[0], 100.0);
    }

    #[test]
    fn scene_rejects_non_finite_sources_and_truncates_stably() {
        let mut sources = (0..u64::try_from(MAX_RENDERED_PLAYERS + 2).unwrap())
            .rev()
            .map(|id| source(id, id as f32, 0.0))
            .collect::<Vec<_>>();
        sources.push(source(u64::MAX, f32::NAN, 0.0));
        let mut scene = ActorRenderScene::default();
        let frame = scene.update(0.0, sources);

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
        let frame = scene.update(0.0, [first, second]);

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
}
