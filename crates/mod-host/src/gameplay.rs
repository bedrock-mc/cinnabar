use super::{MAX_IMPORT_WRITES, State, cinnabar};
use crate::{CameraDelta, GameplaySnapshot, GameplayVector3};
use anyhow::{Result, bail, ensure};
use mod_api::{MAX_CAMERA_DELTA_RADIANS, MAX_GAMEPLAY_PLAYERS};

/// Rejects malformed host frames before granting a guest any access to them.
pub(super) fn validate_snapshot(snapshot: Option<&GameplaySnapshot>) -> Result<()> {
    let Some(frame) = snapshot else { return Ok(()) };
    let finite =
        |point: &GameplayVector3| point.x.is_finite() && point.y.is_finite() && point.z.is_finite();
    ensure!(
        frame.players.len() <= MAX_GAMEPLAY_PLAYERS,
        "too many gameplay players"
    );
    ensure!(
        finite(&frame.eye)
            && frame.yaw.is_finite()
            && frame.pitch.is_finite()
            && frame.frame_seconds.is_finite()
            && (0.0..=1.0).contains(&frame.frame_seconds)
            && frame.players.iter().all(|player| finite(&player.position)),
        "invalid gameplay pose or frame duration"
    );
    Ok(())
}

impl cinnabar::extension::gameplay::Host for State {
    fn read_frame(&mut self) -> Result<Result<Option<GameplaySnapshot>, String>> {
        self.gameplay_reads += 1;
        if self.gameplay_reads > MAX_IMPORT_WRITES {
            bail!("gameplay read budget exhausted");
        }
        if !self.grants.players {
            return Ok(Err("players capability denied".into()));
        }
        Ok(Ok(self.snapshot.clone()))
    }

    fn rotate(&mut self, yaw_delta: f32, pitch_delta: f32) -> Result<Result<(), String>> {
        self.camera_writes += 1;
        if self.camera_writes > MAX_IMPORT_WRITES {
            bail!("camera import budget exhausted");
        }
        if !self.grants.camera {
            return Ok(Err("camera capability denied".into()));
        }
        if self.snapshot.is_none() {
            return Ok(Err("camera requires a current gameplay frame".into()));
        }
        let current = self.pending_camera.unwrap_or_default();
        let next = CameraDelta {
            yaw: current.yaw + yaw_delta,
            pitch: current.pitch + pitch_delta,
        };
        if !yaw_delta.is_finite()
            || !pitch_delta.is_finite()
            || !next.yaw.is_finite()
            || !next.pitch.is_finite()
            || next.yaw.abs() > MAX_CAMERA_DELTA_RADIANS
            || next.pitch.abs() > MAX_CAMERA_DELTA_RADIANS
        {
            return Ok(Err(
                "camera delta must be finite and within the per-frame limit".into(),
            ));
        }
        self.pending_camera = Some(next);
        Ok(Ok(()))
    }
}
