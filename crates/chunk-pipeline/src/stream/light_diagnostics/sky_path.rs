use std::fmt::Write;

use super::*;

const MAX_SKY_PATH_BLOCKS: i32 = 1024;

impl WorldStream {
    /// Finds the first filtering block and unknown gap between the sky seed and a sample.
    pub(super) fn describe_sky_path(&self, out: &mut String, position: [i32; 3], ids: &DecodeIds) {
        let dimension = self.current_dimension();
        if dimension != 0 {
            return;
        }
        let (key, _) = split_light_position(
            dimension,
            BlockPos::new(position[0], position[1], position[2]),
        );
        let Some(top) = self
            .light_column_top_sub_chunk_y(key)
            .and_then(|y| y.checked_mul(16)?.checked_add(15))
        else {
            return;
        };
        let bottom = position[1].max(top.saturating_sub(MAX_SKY_PATH_BLOCKS - 1));
        let mut gap = None;
        let mut blocker = None;
        for y in (bottom..=top).rev() {
            let (key, local) =
                split_light_position(dimension, BlockPos::new(position[0], y, position[2]));
            if !self.light_source_is_known(key) {
                gap.get_or_insert(y);
                continue;
            }
            if let Some(chunk) = self.authority.terrain().sub_chunk(key) {
                let filters = !self.known_air.contains(&key)
                    && (0..chunk.storages().len()).any(|layer| {
                        chunk
                            .runtime_id(layer, local[0], local[1], local[2])
                            .is_some_and(|id| {
                                id != ids.air
                                    && ids.assets.resolve(ids.mode, id).light_properties().filter()
                                        > 0
                            })
                    });
                if filters {
                    blocker = Some((y, key, local));
                    break;
                }
            }
        }
        let _ = write!(
            out,
            "sky_path_xz=({},{}) seed_top_block_sky={:?} first_unknown_y={gap:?} first_filter_y={:?} path_truncated={} ",
            position[0],
            position[2],
            self.solved_light_at([position[0] as f32, top as f32, position[2] as f32]),
            blocker.map(|(y, _, _)| y),
            position[1] < bottom
        );
        if let Some((_, key, local)) = blocker {
            self.describe_block(out, key, local, ids);
        }
        out.push_str("; ");
    }
}
