//! The wire: one JSON request per line carrying the token, an id and a `cmd`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{camera::CameraPath, recorder::Codec};

/// Bedrock view angles in degrees: yaw 0 faces +Z (south), pitch 90 looks straight down.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Look {
    pub yaw: f32,
    pub pitch: f32,
    /// Added to the current view instead of replacing it.
    #[serde(default)]
    pub relative: bool,
    /// Frames to turn over; zero snaps.
    #[serde(default)]
    pub frames: u32,
}

/// Held movement: each axis in -1..=1 holds the bound key for that direction.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Move {
    #[serde(default)]
    pub forward: f32,
    #[serde(default)]
    pub strafe: f32,
}

/// A wheel delta uses lines unless `pixels` selects window-logical pixels.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scroll {
    #[serde(default)]
    pub x: f32,
    pub y: f32,
    #[serde(default)]
    pub pixels: bool,
}

/// Absolute pointer position in logical window pixels, measured from its top-left corner.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Pointer {
    pub x: f32,
    pub y: f32,
}

impl<'de> Deserialize<'de> for Pointer {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Coordinates {
            x: f32,
            y: f32,
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Object(Coordinates),
            Pair([f32; 2]),
        }
        Ok(match Wire::deserialize(deserializer)? {
            Wire::Object(Coordinates { x, y }) | Wire::Pair([x, y]) => Self { x, y },
        })
    }
}

/// Scroll distance follows Bevy's wheel sign: positive Y scrolls up.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wheel {
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    #[serde(default)]
    pub unit: WheelUnit,
}

/// Wheel distances are lines by default, or logical pixels for precise scrolling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WheelUnit {
    #[default]
    Line,
    Pixel,
}

/// Synthetic input. Controls are vanilla binding names (`key.jump`, `key.hotbar.1`), Bevy
/// key names as mods bind them (`Digit1`, `KeyF`, `F8`), or `MouseLeft`/`MouseRight`/...
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputCommand {
    #[serde(default)]
    pub hold: Vec<String>,
    #[serde(default)]
    pub release: Vec<String>,
    /// Pressed now and released after `press_frames` frames.
    #[serde(default)]
    pub press: Vec<String>,
    pub press_frames: Option<u32>,
    #[serde(rename = "move")]
    pub movement: Option<Move>,
    pub jump: Option<bool>,
    pub sneak: Option<bool>,
    pub sprint: Option<bool>,
    /// Selects hotbar slot 1..=9 through its binding.
    pub hotbar: Option<u8>,
    pub look: Option<Look>,
    /// Logical window coordinates; `pointer` takes precedence when both are supplied.
    pub cursor: Option<[f32; 2]>,
    /// Text delivered to the focused editor through keyboard messages.
    pub text: Option<String>,
    pub pointer: Option<Pointer>,
    pub wheel: Option<Wheel>,
    /// Compatibility wheel delta; `wheel` takes precedence.
    pub scroll: Option<Scroll>,
    #[serde(default)]
    pub release_all: bool,
    /// Releases everything and hands the window back to the real keyboard and mouse.
    #[serde(default)]
    pub release_control: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Condition {
    /// Every chunk column within `radius` columns of the player is resident.
    ChunksLoaded { radius: u32 },
    /// A screen whose name contains `name` is on top (e.g. `Gameplay`, `Chat`, `Menu`).
    Screen { name: String },
    /// An actor whose type id or name contains `matches` is within `radius` blocks.
    Entity {
        matches: String,
        #[serde(default)]
        radius: Option<f32>,
    },
    /// The local player has spawned into a world.
    InWorld,
    /// A non-looping camera path has reached its last keyframe.
    CameraFinished,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordSettings {
    pub path: PathBuf,
    #[serde(default = "default_fps")]
    pub fps: u32,
    #[serde(default)]
    pub codec: Codec,
    /// Advance simulation exactly 1/fps per rendered frame instead of following the wall clock.
    #[serde(default = "default_true")]
    pub fixed_clock: bool,
    /// Also capture the game's mixed audio and mux it into the video.
    #[serde(default = "default_true")]
    pub audio: bool,
}

pub const DEFAULT_FPS: u32 = 60;

fn default_fps() -> u32 {
    DEFAULT_FPS
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Connect {
        address: String,
    },
    Disconnect,
    Input(InputCommand),
    /// Imports a classic PNG through the same path as the Dressing Room file picker.
    ImportSkin {
        path: PathBuf,
    },
    /// Imports a PNG cape through the same path as the Dressing Room file picker.
    ImportCape {
        path: PathBuf,
    },
    /// A chat line, or a command when it starts with `/`.
    Chat {
        text: String,
    },
    CameraPath(CameraPath),
    CameraRelease,
    /// Installs or removes an original local cape for animation captures.
    TestCape {
        enabled: bool,
    },
    /// Presents a signed-in launcher with placeholder accounts; nothing signs in or is saved.
    TestAccounts {
        enabled: bool,
    },
    State,
    WaitFor {
        condition: Condition,
        timeout_ms: Option<u64>,
    },
    Screenshot {
        path: PathBuf,
    },
    RecordStart(RecordSettings),
    RecordStop,
    Quit,
}

/// A request line split into its envelope and still-unparsed command.
#[derive(Debug)]
pub struct Request {
    pub id: Value,
    pub token: String,
    body: Value,
}

#[derive(Debug)]
pub struct RequestError {
    pub id: Value,
    pub message: String,
    /// Garbage that never presented a token closes the connection.
    pub unauthenticated: bool,
}

impl Request {
    pub fn parse(line: &str) -> Result<Self, RequestError> {
        let mut body: Value = serde_json::from_str(line).map_err(|error| RequestError {
            id: Value::Null,
            message: format!("malformed request: {error}"),
            unauthenticated: true,
        })?;
        let object = body.as_object_mut().ok_or_else(|| RequestError {
            id: Value::Null,
            message: "request must be a JSON object".into(),
            unauthenticated: true,
        })?;
        let id = object.remove("id").unwrap_or(Value::Null);
        let token = match object.remove("token") {
            Some(Value::String(token)) => token,
            _ => {
                return Err(RequestError {
                    id,
                    message: "unauthorized".into(),
                    unauthenticated: true,
                });
            }
        };
        Ok(Self { id, token, body })
    }

    /// Parsed only after the token checks out.
    pub fn command(self) -> Result<Command, String> {
        serde_json::from_value(self.body).map_err(|error| format!("bad command: {error}"))
    }
}

/// The line a controller sends for `command`.
pub fn request_line(id: u64, token: &str, command: &Command) -> String {
    let mut value = serde_json::to_value(command).unwrap_or(Value::Null);
    if let Some(object) = value.as_object_mut() {
        object.insert("id".into(), id.into());
        object.insert("token".into(), token.into());
    }
    value.to_string()
}

/// A reply line: `{"id", "ok", "result" | "error"}`.
pub fn parse_reply(line: &str) -> Result<(Value, Result<Value, String>), String> {
    let mut reply: Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
    let id = reply.get("id").cloned().unwrap_or(Value::Null);
    let outcome = if reply.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(reply
            .get_mut("result")
            .map(Value::take)
            .unwrap_or(Value::Null))
    } else {
        Err(reply
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("client returned no error message")
            .to_owned())
    };
    Ok((id, outcome))
}

#[cfg(test)]
mod tests;
