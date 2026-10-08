use bevy::prelude::Vec3;
use protocol::CameraAimAssistTargetMode;

/// Geometry is sampled from the camera; reach checks use the nearest point of each actor box.
#[derive(Debug, Clone, Copy)]
pub struct AimAssistFrustum {
    pub origin: Vec3,
    pub forward: Vec3,
    right: Vec3,
    up: Vec3,
    half_angle_tangent: [f32; 2],
    distance_squared: f32,
}

impl AimAssistFrustum {
    /// View angles contain full vertical then horizontal angles, measured in degrees.
    #[must_use]
    pub fn new(origin: Vec3, forward: Vec3, view_angle: [f32; 2], distance: f32) -> Option<Self> {
        if !origin.is_finite()
            || !forward.is_finite()
            || forward.length_squared() < 1.0e-10
            || !distance.is_finite()
            || distance <= 0.0
            || view_angle
                .iter()
                .any(|angle| !angle.is_finite() || *angle <= 0.0 || *angle >= 180.0)
        {
            return None;
        }
        let forward = forward.normalize();
        let right = Vec3::Y.cross(forward).try_normalize()?;
        let up = forward.cross(right);
        Some(Self {
            origin,
            forward,
            right,
            up,
            half_angle_tangent: [view_angle[1], view_angle[0]].map(|angle| {
                let radians = f64::from(angle.to_radians() * 0.5);
                (sim::minecraft_sin(radians) / sim::minecraft_cos(radians)) as f32
            }),
            distance_squared: distance * distance,
        })
    }

    /// Returns the far rectangle basis and half extents used by block sampling.
    #[must_use]
    pub fn far_plane(&self, distance: f32) -> (Vec3, Vec3, f32, f32) {
        (
            self.right,
            self.up,
            self.half_angle_tangent[0] * distance,
            self.half_angle_tangent[1] * distance,
        )
    }

    /// A point on the distance boundary is excluded, matching actor candidate admission.
    #[must_use]
    pub fn contains_box(&self, minimum: Vec3, maximum: Vec3) -> bool {
        if !minimum.is_finite() || !maximum.is_finite() || minimum.cmpgt(maximum).any() {
            return false;
        }
        let delta = self.origin.clamp(minimum, maximum) - self.origin;
        let depth = delta.dot(self.forward);
        depth > 0.0
            && delta.length_squared() < self.distance_squared
            && delta.dot(self.right).abs() <= depth * self.half_angle_tangent[0]
            && delta.dot(self.up).abs() <= depth * self.half_angle_tangent[1]
    }

    /// Ranks already visible candidates without building or sorting a temporary collection.
    #[must_use]
    pub fn select(
        &self,
        mode: CameraAimAssistTargetMode,
        candidates: impl IntoIterator<Item = AimAssistCandidate>,
    ) -> Option<AimAssistTarget> {
        let mut selected: Option<AimAssistTarget> = None;
        for candidate in candidates {
            if candidate.obstructed || !self.contains_box(candidate.minimum, candidate.maximum) {
                continue;
            }
            let delta = candidate.point - self.origin;
            if !delta.is_finite() || delta.length_squared() < 1.0e-10 {
                continue;
            }
            let score = target_score(mode, delta, self.forward, candidate.priority);
            if score.is_finite() && selected.is_none_or(|current| score < current.score) {
                selected = Some(AimAssistTarget {
                    kind: candidate.kind,
                    point: candidate.point,
                    direction: delta.normalize(),
                    score,
                });
            }
        }
        selected
    }
}

/// Stable identity retained from the world candidate; no string copies are needed per frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    Actor(u64),
    Block { position: [i32; 3], face: u8 },
}

/// Visibility and priority are resolved before scoring the actor center or exposed block face.
#[derive(Debug, Clone, Copy)]
pub struct AimAssistCandidate {
    pub kind: TargetKind,
    pub minimum: Vec3,
    pub maximum: Vec3,
    pub point: Vec3,
    pub priority: i32,
    pub obstructed: bool,
}

/// The assisted interaction direction is separate from the rendered camera rotation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AimAssistTarget {
    pub kind: TargetKind,
    pub point: Vec3,
    pub direction: Vec3,
    pub score: f32,
}

/// Larger priorities reduce a bounded weight; angle ranking uses vanilla's cubic approximation.
#[must_use]
pub fn target_score(
    mode: CameraAimAssistTargetMode,
    delta: Vec3,
    forward: Vec3,
    priority: i32,
) -> f32 {
    let weight = (1.1 - 0.01 * priority as f32).clamp(0.1, 1.1);
    match mode {
        CameraAimAssistTargetMode::Angle => {
            let dot = delta.normalize_or_zero().dot(forward);
            weight
                * (-0.698_131_7 * dot * dot * dot - 0.872_664_63 * dot
                    + std::f32::consts::FRAC_PI_2)
        }
        CameraAimAssistTargetMode::Distance => (delta * weight).length_squared(),
    }
}
