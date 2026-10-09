use super::{MAX_IMPORT_WRITES, State, cinnabar};
use anyhow::{Result, bail};

/// Ephemeral output published only after a successful gameplay callback.
#[derive(Default)]
pub(super) struct CameraPolicy {
    pub pending: bool,
    pub committed: bool,
    pub pending_view_scale: Option<[f32; 2]>,
    pub committed_view_scale: Option<[f32; 2]>,
}

impl cinnabar::extension::camera::Host for State {
    fn set_preserve_teleport_rotation(&mut self, enabled: bool) -> Result<Result<(), String>> {
        self.camera_writes += 1;
        if self.camera_writes > MAX_IMPORT_WRITES {
            bail!("camera import budget exhausted");
        }
        if !self.grants.camera {
            return Ok(Err("camera capability denied".into()));
        }
        self.camera_policy.pending = enabled;
        Ok(Ok(()))
    }

    fn set_view_scale(&mut self, fov_scale: f32, look_scale: f32) -> Result<Result<(), String>> {
        self.camera_writes += 1;
        if self.camera_writes > MAX_IMPORT_WRITES {
            bail!("camera import budget exhausted");
        }
        if !self.grants.camera {
            return Ok(Err("camera capability denied".into()));
        }
        if !fov_scale.is_finite()
            || !look_scale.is_finite()
            || !(mod_api::MIN_VIEW_FOV_SCALE..=1.0).contains(&fov_scale)
            || !(mod_api::MIN_VIEW_LOOK_SCALE..=1.0).contains(&look_scale)
        {
            return Ok(Err("view scales are outside their finite bounds".into()));
        }
        self.camera_policy.pending_view_scale =
            ([fov_scale, look_scale] != [1.0, 1.0]).then_some([fov_scale, look_scale]);
        Ok(Ok(()))
    }
}
