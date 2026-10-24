use super::*;

/// Rotation queries read the same partial tick before relative yaw is clamped.
pub(super) fn is_query(assets: &RuntimeEntityAssets, symbol: u32) -> bool {
    assets
        .molang_query_binding(symbol)
        .is_some_and(|query| query.descriptor().sampling == assets::MolangQuerySampling::Rotation)
}

/// Walk queries read the stride and speed interpolated to the frame.
pub(super) fn is_walk_query(assets: &RuntimeEntityAssets, symbol: u32) -> bool {
    assets
        .molang_query_binding(symbol)
        .is_some_and(|query| query.descriptor().sampling == assets::MolangQuerySampling::Walk)
}

impl ActorTickInput {
    /// Interpolates the walk stride and speed linearly between ticks, as the walk cycle advances.
    pub(super) fn sample_walk(self, previous: Self, current: Self, alpha: f32) -> Self {
        let sample = |from: f32, to: f32| from + (to - from) * alpha;
        Self {
            distance_moved: sample(previous.distance_moved, current.distance_moved),
            move_speed: sample(previous.move_speed, current.move_speed),
            ..self
        }
    }

    /// Whether the walk stride or speed changed between the two endpoints.
    pub(super) fn walk_changed(self, other: Self) -> bool {
        self.distance_moved != other.distance_moved || self.move_speed != other.move_speed
    }

    /// Samples each angle along its shortest arc while retaining this endpoint's other inputs.
    pub(super) fn sample_rotation(self, previous: Self, current: Self, alpha: f32) -> Self {
        let sample = |from, to| from + query::wrap_degrees(to - from) * alpha;
        Self {
            body_yaw: sample(previous.body_yaw, current.body_yaw),
            yaw: sample(previous.yaw, current.yaw),
            head_yaw: sample(previous.head_yaw, current.head_yaw),
            pitch: sample(previous.pitch, current.pitch),
            ..self
        }
    }

    /// Whether the two endpoints need a fresh rotation sample between ticks.
    pub(super) fn rotation_changed(self, other: Self) -> bool {
        self.body_yaw != other.body_yaw
            || self.yaw != other.yaw
            || self.head_yaw != other.head_yaw
            || self.pitch != other.pitch
    }
}
