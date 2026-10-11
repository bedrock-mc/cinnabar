use super::{BlockHit, PaletteWorld, Vec3, WorldQueryError, validate_ray};

/// Maximum length of the downward support ray after a clear pick miss.
pub const BLOCK_USE_SUPPORT_DEPTH: f64 = 2.0;
/// Strict upper bound on normalized look Y for indirect support picking.
pub const BLOCK_USE_SUPPORT_MAX_Y: f32 = -0.7;

impl PaletteWorld<'_> {
    /// After a clear block and actor miss, finds nearby support with a horizontal look face.
    /// Keeps the downward intercept and identity; callers must exclude this hit from mining.
    pub fn block_use_miss_support_current(
        &self,
        origin: Vec3,
        direction: Vec3,
    ) -> Result<Option<BlockHit>, WorldQueryError> {
        let direction = validate_ray(origin, direction, BLOCK_USE_SUPPORT_DEPTH)?;
        if direction.y as f32 >= BLOCK_USE_SUPPORT_MAX_Y {
            return Ok(None);
        }
        let Some(mut hit) = self.block_interaction_ray_current(
            origin,
            Vec3::new(0.0, -1.0, 0.0),
            BLOCK_USE_SUPPORT_DEPTH,
        )?
        else {
            return Ok(None);
        };
        hit.face = if direction.x.abs() > direction.z.abs() {
            if direction.x < 0.0 { 4 } else { 5 }
        } else if direction.z < 0.0 {
            2
        } else {
            3
        };
        Ok(Some(hit))
    }
}
