//! Cosmetic transform playback and continuous span flex in authored mesh coordinates.
use super::*;
use serde::Deserialize;
use std::collections::BTreeMap;

const MAX_FLEX_JOINTS: usize = 512;

#[derive(Default, Deserialize)]
pub(super) struct Animation {
    pub cloth: Option<super::cloth::Definition>,
    pub wind: Option<Wind>,
    #[serde(default)]
    pub animations: BTreeMap<String, clip_playback::Definition>,
    pub playback: Option<clip_playback::Sequence>,
    pub tip_flex: Option<TipFlex>,
    #[serde(default)]
    pub particles: Vec<super::emitters::Definition>,
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
    clips: clip_playback::Clips,
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
            return (definition.tip_flex.is_none() && definition.playback.is_none())
                .then_some(None);
        }
        if definition.tip_flex.is_some() && definition.playback.is_some() {
            return None;
        }
        let clips = clip_playback::Clips::build(
            std::mem::take(&mut definition.animations),
            definition.playback.take(),
            names,
        )?;
        let length = clips.length();
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
                    || !clips.has_track(parent)
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
        let mut indexed_spans: Vec<Option<Span>> = (0..parents.len()).map(|_| None).collect();
        for span in spans {
            let index = span.bone;
            indexed_spans[index] = Some(span);
        }
        Some(Some(Self {
            length,
            clips,
            flex: definition.tip_flex.take(),
            spans: indexed_spans,
        }))
    }

    /// Samples mirrored authored rotations, reflecting mesh X into the render frame.
    pub fn rotation(&self, bone: usize, seconds: f32) -> Quat {
        let (angles, _) = self.clips.sample(bone, seconds);
        let angles = angles * (std::f32::consts::PI / 180.0);
        Quat::from_euler(glam::EulerRot::ZYX, -angles.z, -angles.y, angles.x)
    }

    /// Reflects authored translation pixels into the same local frame as geometry pivots.
    pub fn translation(&self, bone: usize, seconds: f32) -> Vec3 {
        let (_, p) = self.clips.sample(bone, seconds);
        Vec3::new(-p.x, p.y, p.z) / 16.0
    }

    /// Bounds caller-owned elapsed time without replaying a completed introduction.
    pub fn normalize(&self, seconds: f64) -> f32 {
        self.clips.normalize(seconds)
    }

    /// Chooses a perched initial pose for static gallery artwork.
    pub fn rest_time(&self) -> f32 {
        self.clips.rest_time()
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
