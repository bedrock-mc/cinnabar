use super::{MAX_IMPORT_WRITES, State, cinnabar};
use crate::{
    CameraDelta, GameplayCameraRig, GameplayMob, GameplaySnapshot, GameplayVector3, ModCue,
};
use anyhow::{Result, bail, ensure};
use mod_api::{
    MAX_CAMERA_DELTA_RADIANS, MAX_COMMAND_BYTES, MAX_COMMANDS_PER_FRAME, MAX_COMMANDS_PER_SECOND,
    MAX_CUE_NAME_BYTES, MAX_CUE_VALUES, MAX_CUES_PER_FRAME, MAX_GAMEPLAY_MOBS,
    MAX_GAMEPLAY_PLAYERS, MAX_INCOMING_CUES, MAX_MOB_RANGE_BLOCKS, MAX_MOB_TYPE_BYTES,
    MAX_RIG_BACK_BLOCKS, MAX_RIG_FOV_DELTA_DEGREES, MAX_RIG_ROLL_RADIANS, MAX_RIG_SIDE_BLOCKS,
    MAX_RIG_VERTICAL_BLOCKS,
};

/// Mob input, the retained rig, and per-frame command and cue output.
#[derive(Default)]
pub(super) struct WorldState {
    pub mobs: Vec<GameplayMob>,
    pub rig: Option<GameplayCameraRig>,
    pub commands: Vec<String>,
    pub cues: Vec<ModCue>,
    /// Delivered before a callback and dropped after it.
    pub incoming: Vec<ModCue>,
    polls: u32,
    pending_rig: Option<Option<GameplayCameraRig>>,
    pending_commands: Vec<String>,
    pending_cues: Vec<ModCue>,
    window_seconds: f32,
    window_sent: usize,
}

impl WorldState {
    /// Drops unconsumed one-frame output; the committed rig is retained.
    pub fn begin_frame(&mut self) {
        self.polls = 0;
        self.mobs.clear();
        self.commands.clear();
        self.cues.clear();
        self.pending_rig = None;
        self.pending_commands.clear();
        self.pending_cues.clear();
    }

    /// Advances the one-second command rate window by host-measured frame time.
    pub fn advance_command_window(&mut self, seconds: f32) {
        self.window_seconds += seconds;
        if self.window_seconds >= 1.0 {
            self.window_seconds = 0.0;
            self.window_sent = 0;
        }
    }

    pub fn commit(&mut self) {
        if let Some(rig) = self.pending_rig.take() {
            self.rig = rig;
        }
        self.commands = std::mem::take(&mut self.pending_commands);
        self.window_sent += self.commands.len();
        self.cues = std::mem::take(&mut self.pending_cues);
    }
}

fn finite(point: &GameplayVector3) -> bool {
    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
}

/// Mobs need a current snapshot and must lie within the published range of its eye.
pub(super) fn validate_mobs(
    snapshot: Option<&GameplaySnapshot>,
    mobs: &[GameplayMob],
) -> Result<()> {
    if mobs.is_empty() {
        return Ok(());
    }
    let Some(frame) = snapshot else {
        bail!("mobs require a current gameplay frame");
    };
    ensure!(mobs.len() <= MAX_GAMEPLAY_MOBS, "too many gameplay mobs");
    let range = MAX_MOB_RANGE_BLOCKS * MAX_MOB_RANGE_BLOCKS;
    ensure!(
        mobs.iter().all(|mob| {
            let (dx, dy, dz) = (
                mob.position.x - frame.eye.x,
                mob.position.y - frame.eye.y,
                mob.position.z - frame.eye.z,
            );
            finite(&mob.position)
                && dx * dx + dy * dy + dz * dz <= range
                && !mob.type_id.is_empty()
                && mob.type_id.len() <= MAX_MOB_TYPE_BYTES
                && mob.health.is_none_or(f32::is_finite)
                && mob.max_health.is_none_or(f32::is_finite)
        }),
        "invalid gameplay mob"
    );
    Ok(())
}

fn rig_valid(rig: &GameplayCameraRig) -> bool {
    let offset = &rig.offset;
    finite(offset)
        && rig.roll.is_finite()
        && rig.fov_delta.is_finite()
        && offset.x.abs() <= MAX_RIG_SIDE_BLOCKS
        && offset.y.abs() <= MAX_RIG_VERTICAL_BLOCKS
        && (0.0..=MAX_RIG_BACK_BLOCKS).contains(&offset.z)
        && rig.roll.abs() <= MAX_RIG_ROLL_RADIANS
        && rig.fov_delta.abs() <= MAX_RIG_FOV_DELTA_DEGREES
}

/// Accepts `/name args` only for a granted bare name, in printable ASCII.
fn command_allowed(command: &str, granted: &[String]) -> bool {
    let Some(body) = command.strip_prefix('/') else {
        return false;
    };
    let name = body.split(' ').next().unwrap_or_default();
    command.len() <= MAX_COMMAND_BYTES
        && command.bytes().all(|byte| (0x20..0x7f).contains(&byte))
        && granted.iter().any(|granted| granted == name)
}

fn cue_name_valid(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_CUE_NAME_BYTES
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
        })
}

impl cinnabar::extension::events::Host for State {
    fn emit(&mut self, name: String, values: Vec<f32>) -> Result<Result<(), String>> {
        if self.world.pending_cues.len() >= MAX_CUES_PER_FRAME {
            bail!("cue budget exhausted");
        }
        if !cue_valid(&name, &values) {
            return Ok(Err(
                "cue must be a short name with a few finite values".into()
            ));
        }
        self.world.pending_cues.push(ModCue { name, values });
        Ok(Ok(()))
    }

    fn poll(&mut self) -> Result<Vec<ModCue>> {
        self.world.polls += 1;
        if self.world.polls > MAX_IMPORT_WRITES {
            bail!("cue poll budget exhausted");
        }
        Ok(self.world.incoming.clone())
    }
}

fn cue_valid(name: &str, values: &[f32]) -> bool {
    cue_name_valid(name) && values.len() <= MAX_CUE_VALUES && values.iter().all(|v| v.is_finite())
}

/// Keeps only cues a guest could have emitted, up to the per-callback bound.
pub(super) fn incoming(cues: Vec<ModCue>) -> Vec<ModCue> {
    cues.into_iter()
        .filter(|cue| cue_valid(&cue.name, &cue.values))
        .take(MAX_INCOMING_CUES)
        .collect()
}

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
    fn set_show_real_position(&mut self, enabled: bool) -> Result<Result<(), String>> {
        self.packet_delay_writes += 1;
        if self.packet_delay_writes > MAX_IMPORT_WRITES {
            bail!("packet delay import budget exhausted");
        }
        if !self.grants.packet_delay {
            return Ok(Err("packet delay capability denied".into()));
        }
        self.pending_show_real_position = Some(enabled);
        Ok(Ok(()))
    }

    fn set_packet_delay(&mut self, delay_ms: u32) -> Result<Result<(), String>> {
        self.packet_delay_writes += 1;
        if self.packet_delay_writes > MAX_IMPORT_WRITES {
            bail!("packet delay import budget exhausted");
        }
        if !self.grants.packet_delay {
            return Ok(Err("packet delay capability denied".into()));
        }
        if delay_ms > mod_api::MAX_PACKET_DELAY_MS {
            return Ok(Err("packet delay exceeds the capability limit".into()));
        }
        self.pending_packet_delay = Some(delay_ms);
        Ok(Ok(()))
    }

    fn set_attack_reach(&mut self, blocks: Option<f32>) -> Result<Result<(), String>> {
        self.set_reach(blocks)
    }

    fn pulse_attack(&mut self) -> Result<Result<(), String>> {
        State::pulse_attack(self)
    }

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

    fn read_mobs(&mut self) -> Result<Result<Vec<GameplayMob>, String>> {
        self.gameplay_reads += 1;
        if self.gameplay_reads > MAX_IMPORT_WRITES {
            bail!("gameplay read budget exhausted");
        }
        if !self.grants.entities {
            return Ok(Err("entities capability denied".into()));
        }
        Ok(Ok(self.world.mobs.clone()))
    }

    fn set_camera_rig(&mut self, rig: Option<GameplayCameraRig>) -> Result<Result<(), String>> {
        self.camera_writes += 1;
        if self.camera_writes > MAX_IMPORT_WRITES {
            bail!("camera import budget exhausted");
        }
        if !self.grants.camera {
            return Ok(Err("camera capability denied".into()));
        }
        if let Some(rig) = &rig {
            if self.snapshot.is_none() {
                return Ok(Err("camera rig requires a current gameplay frame".into()));
            }
            if !rig_valid(rig) {
                return Ok(Err("camera rig must be finite and within its bounds".into()));
            }
        }
        self.world.pending_rig = Some(rig);
        Ok(Ok(()))
    }

    fn request_command(&mut self, command: String) -> Result<Result<(), String>> {
        if self.world.pending_commands.len() >= MAX_COMMANDS_PER_FRAME {
            bail!("command budget exhausted");
        }
        if self.grants.commands.is_empty() {
            return Ok(Err("commands capability denied".into()));
        }
        if self.snapshot.is_none() {
            return Ok(Err("commands require a current gameplay frame".into()));
        }
        if !command_allowed(&command, &self.grants.commands) {
            return Ok(Err(
                "command is not granted or not short printable text".into()
            ));
        }
        if self.world.window_sent + self.world.pending_commands.len() >= MAX_COMMANDS_PER_SECOND {
            return Ok(Err("command rate limit reached".into()));
        }
        self.world.pending_commands.push(command);
        Ok(Ok(()))
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

#[cfg(test)]
#[path = "gameplay_tests.rs"]
mod tests;
