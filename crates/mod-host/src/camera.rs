use super::{MAX_IMPORT_WRITES, State, cinnabar};
use anyhow::{Result, bail};

/// Ephemeral output published only after a successful frame callback.
#[derive(Default)]
pub(super) struct CameraPolicy {
    pub pending: bool,
    pub committed: bool,
    pub pending_view_scale: Option<[f32; 2]>,
    pub committed_view_scale: Option<[f32; 2]>,
    pub pending_hurt_style: Option<render_api::HurtCameraStyle>,
    pub committed_hurt_style: Option<render_api::HurtCameraStyle>,
}

impl cinnabar::extension::camera::Host for State {
    fn set_hurt_camera_style(
        &mut self,
        style: Option<cinnabar::extension::camera::HurtCameraStyle>,
    ) -> Result<Result<(), String>> {
        self.camera_writes += 1;
        if self.camera_writes > MAX_IMPORT_WRITES {
            bail!("camera import budget exhausted");
        }
        if self.exclusive.camera {
            return Ok(Err(self.exclusive.refuse()));
        }
        if !self.grants.camera {
            return Ok(Err("camera capability denied".into()));
        }
        self.camera_policy.pending_hurt_style = match style {
            None => None,
            Some(style) => {
                match render_api::HurtCameraStyle::new(style.intensity, style.old_tilt) {
                    Some(style) => Some(style),
                    None => {
                        return Ok(Err(
                            "hurt camera intensity is outside its finite bounds".into()
                        ));
                    }
                }
            }
        };
        Ok(Ok(()))
    }

    fn set_preserve_teleport_rotation(&mut self, enabled: bool) -> Result<Result<(), String>> {
        self.camera_writes += 1;
        if self.camera_writes > MAX_IMPORT_WRITES {
            bail!("camera import budget exhausted");
        }
        if self.exclusive.camera {
            return Ok(Err(self.exclusive.refuse()));
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
        if self.exclusive.camera {
            return Ok(Err(self.exclusive.refuse()));
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
