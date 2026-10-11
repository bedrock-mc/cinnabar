use super::*;

/// Offset that includes the block above an actor's upper collision face.
const ACTOR_LIGHT_TOP_OFFSET: f32 = 0.01;

/// Default block and sky light when a dimension has no light cell at the requested height.
pub(super) const fn dimension_default_light(dimension: i32) -> [u8; 2] {
    if dimension == 0 { [0, 15] } else { [0, 0] }
}

impl WorldStream {
    /// Retained `(block, sky)` light in the current dimension. Outside its admitted
    /// height uses the dimension default; unresolved or non-finite positions read dark.
    #[must_use]
    pub fn light_level_at(&self, position: [f32; 3]) -> (u8, u8) {
        self.solved_light_at(position).unwrap_or((0, 0))
    }

    /// Render light at ordinary actor AABB endpoints, taking each channel's maximum.
    /// `feet` is the interpolated collision position, independent of camera perspective.
    #[must_use]
    pub fn actor_light_at(
        &self,
        actor: &client_world::ActorSnapshot,
        feet: [f32; 3],
    ) -> Option<(u8, u8)> {
        let Some((min, max)) = actor.bounding_box_at(feet) else {
            return self.solved_light_at(feet);
        };
        let top_y = max[1] + ACTOR_LIGHT_TOP_OFFSET;
        let top = self.solved_light_at([feet[0], top_y, feet[2]]);
        if top_y.floor() == min[1].floor() {
            return top;
        }
        let bottom = self.solved_light_at([feet[0], min[1], feet[2]]);
        match (top, bottom) {
            (Some(top), Some(bottom)) => Some((top.0.max(bottom.0), top.1.max(bottom.1))),
            (top, bottom) => top.or(bottom),
        }
    }

    /// Retained `(block, sky)` light or the dimension default beyond its height.
    /// `None` identifies invalid coordinates or unresolved light within the height range.
    #[must_use]
    pub fn solved_light_at(&self, position: [f32; 3]) -> Option<(u8, u8)> {
        if !position.iter().all(|value| value.is_finite()) {
            return None;
        }
        let block = position.map(floor_to_i32);
        let dimension = self.authority.current_dimension();
        let section_y = block[1].div_euclid(16);
        if let Some(range) = self.authority.dimension_range(dimension) {
            let offset = i64::from(section_y) - i64::from(range.base_sub_chunk_y);
            if offset < 0 || offset >= range.sub_chunk_count as i64 {
                let [block, sky] = dimension_default_light(dimension);
                return Some((block, sky));
            }
        }
        let key = SubChunkKey::new(
            dimension,
            block[0].div_euclid(16),
            section_y,
            block[2].div_euclid(16),
        );
        let light = self.lighting.store.light(key)?;
        let local = |axis: usize| block[axis].rem_euclid(16) as u8;
        let (x, y, z) = (local(0), local(1), local(2));
        Some((
            light.get(LightChannel::Block, x, y, z).unwrap_or(0),
            light.get(LightChannel::Sky, x, y, z).unwrap_or(0),
        ))
    }
}
