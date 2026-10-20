//! Bounded declarative world drawing for trusted packet extensions.
use serde::{Deserialize, Serialize};

/// Native initialization bit for the independent `render-frame` callback.
pub const CAPABILITY: u32 = 8;
/// A wire box may expand to twelve admitted line instances.
pub const MAX_BOXES: usize = crate::MAX_DRAW_INSTANCES / 16;
/// Remaining bounded tracer capacity within the retained instance allowance.
pub const MAX_LINES: usize = crate::MAX_DRAW_INSTANCES / 8;

/// Drawing observes only the current connected-world scope.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub version: u32,
    pub session: u64,
    pub dimension: i32,
}

/// One oriented volume in the connected world.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoxVolume {
    pub centre: [f32; 3],
    pub size: [f32; 3],
    pub yaw: f32,
    pub colour: [f32; 4],
    pub filled: bool,
}
/// One world-space line, with no captured-input observations.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Line {
    pub start: [f32; 3],
    pub end: [f32; 3],
    pub colour: [f32; 4],
}
/// One complete desired scene, fenced to the host's native frame scope.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    pub version: u32,
    pub session: u64,
    pub dimension: i32,
    pub boxes: Vec<BoxVolume>,
    pub lines: Vec<Line>,
}
impl Scene {
    /// Validates the wire budget and finite input; retained drawing owns resource admission.
    pub fn is_valid(&self) -> bool {
        self.version == super::native_frame::VERSION
            && self.boxes.len() <= MAX_BOXES
            && self.lines.len() <= MAX_LINES
            && self.boxes.iter().all(|b| {
                point(b.centre)
                    && b.size.iter().all(|v| {
                        v.is_finite() && *v > 0.0 && *v <= crate::MAX_PRIMITIVE_EXTENT_BLOCKS
                    })
                    && b.yaw.is_finite()
                    && colour(b.colour)
            })
            && self
                .lines
                .iter()
                .all(|l| point(l.start) && point(l.end) && colour(l.colour))
    }
}
/// Drawing coordinates use the existing native world-primitives extent.
fn point(v: [f32; 3]) -> bool {
    v.iter()
        .all(|v| v.is_finite() && v.abs() <= crate::MAX_PRIMITIVE_COORDINATE)
}
/// Colours use the retained drawing owner's component bounds.
fn colour(v: [f32; 4]) -> bool {
    v.iter()
        .all(|v| v.is_finite() && (0.0..=crate::MAX_DRAW_COLOR_COMPONENT).contains(v))
        && v[3] <= 1.0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixture_and_retained_budget_match() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../native-render-v1.json")).unwrap();
        assert_eq!(fixture["capability"], CAPABILITY);
        assert_eq!(fixture["max_boxes"], MAX_BOXES);
        assert_eq!(fixture["max_lines"], MAX_LINES);
        assert!(MAX_BOXES * 12 + MAX_LINES <= crate::MAX_DRAW_INSTANCES);
        let scene: Scene = serde_json::from_value(fixture["scene"].clone()).unwrap();
        assert!(scene.is_valid());
        let encoded = serde_json::to_vec(&scene).unwrap();
        assert_eq!(serde_json::from_slice::<Scene>(&encoded).unwrap(), scene);
        let frame: Frame = serde_json::from_value(fixture["input"].clone()).unwrap();
        assert_eq!(serde_json::to_value(frame).unwrap(), fixture["input"]);
    }
}
