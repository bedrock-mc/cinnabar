//! Bounded cosmetic-only rotation clips and continuous span flex in authored mesh coordinates.
use super::*;
use serde::Deserialize;
use std::collections::BTreeMap;

const MAX_FLEX_JOINTS: usize = 512;

#[derive(Default, Deserialize)]
pub(super) struct Animation {
    pub wind: Option<Wind>,
    #[serde(default)]
    pub animations: BTreeMap<String, Clip>,
    pub tip_flex: Option<TipFlex>,
}
#[derive(Deserialize)]
pub(super) struct Wind {
    pub axis: String,
    pub max_degrees: f32,
    pub speed: f32,
    pub rise: f32,
    pub fall: f32,
    pub joints: BTreeMap<String, f32>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Clip {
    pub r#loop: bool,
    pub animation_length: f32,
    pub bones: BTreeMap<String, Rotation>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Rotation {
    pub rotation: BTreeMap<String, [f32; 3]>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TipFlex {
    start: f32,
    span: f32,
    base_degrees: f32,
    amplitude_degrees: f32,
    phase_delay: f32,
    joints: BTreeMap<String, i8>,
}
pub(super) struct Playback {
    pub length: f32,
    tracks: Vec<Option<Vec<(f32, Vec3)>>>,
    flex: Option<TipFlex>,
    spans: Vec<Option<Span>>,
}
struct Span {
    bone: usize,
    distance: f32,
    sign: f32,
}
impl Playback {
    /// Resolves validated names and creates shared span joints once, without per-frame mesh uploads.
    pub fn build(
        definition: &mut Animation,
        names: &[&str],
        geometry: &mut ActorRigGeometry,
        parents: &mut Vec<Option<usize>>,
    ) -> Option<Option<Self>> {
        if definition.animations.is_empty() {
            return definition.tip_flex.is_none().then_some(None);
        }
        if definition.animations.len() != 1 {
            return None;
        }
        let clip = definition.animations.pop_first()?.1;
        let length = clip.animation_length;
        if !clip.r#loop
            || !length.is_finite()
            || !(0.1..=60.0).contains(&length)
            || clip.bones.is_empty()
            || clip.bones.len() > 32
        {
            return None;
        }
        let mut tracks = Vec::new();
        for (name, rotation) in clip.bones {
            let index = names.iter().position(|n| *n == name)?;
            if !(2..=256).contains(&rotation.rotation.len()) {
                return None;
            }
            let mut keys = Vec::new();
            for (time, angles) in rotation.rotation {
                let time: f32 = time.parse().ok()?;
                if !time.is_finite()
                    || !(0.0..=length).contains(&time)
                    || angles.iter().any(|v| !v.is_finite() || v.abs() > 180.0)
                {
                    return None;
                }
                keys.push((time, Vec3::from_array(angles.map(f32::to_radians))));
            }
            keys.sort_by(|a, b| a.0.total_cmp(&b.0));
            if keys.first()?.0 != 0.0
                || keys.last()?.0 != length
                || keys.first()?.1 != keys.last()?.1
                || keys.windows(2).any(|k| k[0].0 >= k[1].0)
            {
                return None;
            }
            tracks.push((index, keys));
        }
        let mut pivots = geometry.bone_pivots.to_vec();
        let mut vertices = geometry.vertices.to_vec();
        let mut spans = Vec::new();
        if let Some(flex) = &definition.tip_flex {
            if ![
                flex.start,
                flex.span,
                flex.base_degrees,
                flex.amplitude_degrees,
                flex.phase_delay,
            ]
            .iter()
            .all(|v| v.is_finite())
                || !(0.0..=32.0).contains(&flex.start)
                || !(1.0..=32.0).contains(&flex.span)
                || flex.base_degrees.abs() + flex.amplitude_degrees.abs() > 45.0
                || flex.phase_delay.abs() > std::f32::consts::TAU
                || flex.joints.is_empty()
                || flex.joints.len() > 8
            {
                return None;
            }
            let mut indices = BTreeMap::new();
            for (name, sign) in &flex.joints {
                let parent = names.iter().position(|n| *n == name)?;
                if ![-1, 1].contains(sign)
                    || !tracks.iter().any(|(i, _)| *i == parent)
                    || parents.contains(&Some(parent))
                {
                    return None;
                }
                indices.insert(parent, f32::from(*sign));
            }
            let mut joints = BTreeMap::new();
            for vertex in &mut vertices {
                let parent = vertex.bone_index as usize;
                let Some(&sign) = indices.get(&parent) else {
                    continue;
                };
                let x = vertex.position[0];
                let distance = -sign * (x - pivots[parent][0]) - flex.start / 16.0;
                if distance <= 0.0 {
                    continue;
                }
                if distance > 2.0 * flex.span / 16.0 {
                    return None;
                }
                let key = (parent, x.to_bits());
                let bone = if let Some(&bone) = joints.get(&key) {
                    bone
                } else {
                    if spans.len() >= MAX_FLEX_JOINTS
                        || pivots.len() >= crate::MAX_RENDER_BONES_PER_ACTOR
                    {
                        return None;
                    }
                    let bone = pivots.len();
                    pivots.push([x, pivots[parent][1], pivots[parent][2]]);
                    parents.push(Some(parent));
                    spans.push(Span {
                        bone,
                        distance,
                        sign,
                    });
                    joints.insert(key, bone);
                    bone
                };
                vertex.bone_index = bone as u32;
            }
            if spans.is_empty() {
                return None;
            }
        }
        *geometry = ActorRigGeometry::new(geometry.id, vertices, pivots).ok()?;
        let mut indexed_tracks = vec![None; parents.len()];
        for (index, keys) in tracks {
            indexed_tracks[index] = Some(keys);
        }
        let mut indexed_spans: Vec<Option<Span>> = (0..parents.len()).map(|_| None).collect();
        for span in spans {
            let index = span.bone;
            indexed_spans[index] = Some(span);
        }
        Some(Some(Self {
            length,
            tracks: indexed_tracks,
            flex: definition.tip_flex.take(),
            spans: indexed_spans,
        }))
    }

    /// Samples mirrored authored rotations, reflecting mesh X into the render frame.
    pub fn rotation(&self, bone: usize, seconds: f32) -> Quat {
        let Some(keys) = self.tracks.get(bone).and_then(Option::as_ref) else {
            return Quat::IDENTITY;
        };
        let time = seconds.rem_euclid(self.length);
        let upper = keys
            .partition_point(|key| key.0 <= time)
            .min(keys.len() - 1);
        let (a, b) = (keys[upper.saturating_sub(1)], keys[upper]);
        let angles = if a.0 == b.0 {
            a.1
        } else {
            a.1.lerp(b.1, (time - a.0) / (b.0 - a.0))
        };
        Quat::from_euler(glam::EulerRot::ZYX, -angles.z, -angles.y, angles.x)
    }

    /// Evaluates the shared curvature once per pose, in radians per block.
    pub fn curvature(&self, seconds: f32) -> f32 {
        let Some(flex) = &self.flex else {
            return 0.0;
        };
        let phase = std::f32::consts::TAU * seconds / self.length;
        (flex.base_degrees + flex.amplitude_degrees * (phase - flex.phase_delay).sin()).to_radians()
            / (flex.span / 16.0)
    }

    /// Bends each shared span around the same analytic curve, including solid support thickness.
    pub fn span(&self, bone: usize, curvature: f32) -> (Vec3, Quat) {
        let Some(span) = self.spans.get(bone).and_then(Option::as_ref) else {
            return (Vec3::ZERO, Quat::IDENTITY);
        };
        let angle = curvature * span.distance;
        let delta = if curvature.abs() < 1e-6 {
            Vec3::ZERO
        } else {
            Vec3::new(
                -span.sign * (angle.sin() / curvature - span.distance),
                0.0,
                (1.0 - angle.cos()) / curvature,
            )
        };
        (delta, Quat::from_rotation_y(span.sign * angle))
    }
}
