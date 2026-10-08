use super::{MAX_IMPORT_WRITES, State, cinnabar};
use anyhow::{Result, bail};

/// Ephemeral output published only after a successful gameplay callback.
#[derive(Default)]
pub(super) struct CameraPolicy {
    pub pending: bool,
    pub committed: bool,
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
}
