//! Bounded declarative world drawing for trusted packet extensions.
use serde::{Deserialize, Serialize};

/// Native initialization bit for the independent `render-frame` callback.
pub const CAPABILITY: u32 = 8;
/// Optional consent bits carried by the same independent callback.
pub const ENTITY_HIGHLIGHTS: u32 = 16;
pub const BLOCK_HIGHLIGHTS: u32 = 32;
/// Optional per-block outlines; requires the same block-highlight consent.
pub const BLOCK_OUTLINES: u32 = 128;
/// Optional actor-relative lines and repositioned mesh coverage under entity consent.
pub const ACTOR_ANCHORS: u32 = 256;
/// Complete scenes have separate bounds from the small input snapshot.
pub const MAX_BYTES: usize = 256 << 10;
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
    #[serde(default, skip_serializing_if = "is_false")]
    pub camera_start: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_end: Option<EntityLineEnd>,
    pub start: [f32; 3],
    pub end: [f32; 3],
    pub colour: [f32; 4],
}
/// Omits inactive optional anchors for earlier SDK consumers.
fn is_false(value: &bool) -> bool {
    !value
}
/// A target offset in world axes, resolved from the actor's interpolated body root.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityLineEnd {
    pub runtime_id: u64,
    pub offset: [f32; 3],
}
/// One bounded union of actual actor meshes, composed by the existing mask renderer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityHighlight {
    /// Places a single selected mesh at these feet, preserving its current animated pose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<[f32; 3]>,
    pub runtime_ids: Vec<u64>,
    pub style: crate::drawing::MaskStyle,
}
/// One full-block highlight using the host's loaded-block presentation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockHighlight {
    pub position: [i32; 3],
    pub colour: [f32; 4],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<BlockOutline>,
}
/// Independent edge colour and constant physical-pixel width for a unit block.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockOutline {
    pub colour: [f32; 4],
    pub width: f32,
}
impl BlockOutline {
    /// Applies the existing highlight colour bounds and a bounded pixel width.
    pub fn valid(&self) -> bool {
        self.colour
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            && self.width.is_finite()
            && self.width > 0.0
            && self.width <= 8.0
    }
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
    #[serde(default)]
    pub entities: Vec<EntityHighlight>,
    #[serde(default)]
    pub blocks: Vec<BlockHighlight>,
}
impl Scene {
    /// Validates the wire budget and finite input; retained drawing owns resource admission.
    pub fn is_valid(&self) -> bool {
        self.version == super::native_frame::VERSION
            && self.boxes.len() <= MAX_BOXES
            && self.lines.len() <= MAX_LINES
            && self.entities.len() <= crate::drawing::MAX_MASKS
            && self.entities.iter().all(|group| {
                group.style.valid()
                    && group
                        .position
                        .is_none_or(|position| group.runtime_ids.len() == 1 && point(position))
                    && group.runtime_ids.len() <= crate::drawing::MAX_MASK_ENTITIES
                    && group.runtime_ids.iter().all(|id| *id != 0)
            })
            && self.blocks.len() <= crate::MAX_BLOCK_HIGHLIGHTS
            && self.blocks.iter().all(|b| {
                b.outline.as_ref().is_none_or(BlockOutline::valid)
                    && point(b.position.map(|v| v as f32))
                    && b.colour
                        .iter()
                        .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            })
            && self.boxes.iter().all(|b| {
                point(b.centre)
                    && b.size.iter().all(|v| {
                        v.is_finite() && *v > 0.0 && *v <= crate::MAX_PRIMITIVE_EXTENT_BLOCKS
                    })
                    && b.yaw.is_finite()
                    && colour(b.colour)
            })
            && self.lines.iter().all(|l| {
                point(l.start)
                    && point(l.end)
                    && colour(l.colour)
                    && l.entity_end.as_ref().is_none_or(|anchor| {
                        anchor.runtime_id != 0
                            && anchor.offset.iter().all(|v| {
                                v.is_finite() && v.abs() <= crate::MAX_PRIMITIVE_EXTENT_BLOCKS
                            })
                    })
            })
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
    fn block_outlines_are_optional_bounded_and_independent_of_the_retained_box_budget() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../native-render-v1.json")).unwrap();
        let mut scene: Scene = serde_json::from_value(fixture["scene"].clone()).unwrap();
        assert!(scene.blocks.iter().all(|b| b.outline.is_none()));
        let block = BlockHighlight {
            position: [1, 2, 3],
            colour: [0.; 4],
            outline: Some(BlockOutline {
                colour: [1., 0., 0., 0.85],
                width: 1.5,
            }),
        };
        scene.blocks = vec![block; crate::MAX_BLOCK_HIGHLIGHTS];
        assert!(scene.is_valid());
        assert!(serde_json::to_vec(&scene).unwrap().len() <= MAX_BYTES);
        for width in [0., -1., 8.1, f32::NAN, f32::INFINITY] {
            scene.blocks[0].outline.as_mut().unwrap().width = width;
            assert!(!scene.is_valid());
        }
        scene.blocks[0].outline.as_mut().unwrap().width = 1.5;
        scene.blocks[0].outline.as_mut().unwrap().colour[3] = 1.1;
        assert!(!scene.is_valid());
    }
    #[test]
    fn actor_anchors_and_filled_meshes_require_bounded_unambiguous_targets() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../native-render-v1.json")).unwrap();
        assert_eq!(fixture["actor_anchor_capability"], ACTOR_ANCHORS);
        let mut scene: Scene = serde_json::from_value(fixture["anchored_scene"].clone()).unwrap();
        assert!(scene.is_valid());
        scene.entities[0].runtime_ids.push(43);
        assert!(!scene.is_valid());
        scene.entities[0].runtime_ids.pop();
        scene.entities[0].style.fill.as_mut().unwrap()[3] = 1.1;
        assert!(!scene.is_valid());
        scene.entities[0].style.fill.as_mut().unwrap()[3] = 0.3;
        scene.lines[0].entity_end.as_mut().unwrap().runtime_id = 0;
        assert!(!scene.is_valid());
        scene.lines[0].entity_end.as_mut().unwrap().runtime_id = 42;
        scene.lines[0].entity_end.as_mut().unwrap().offset[0] = f32::NAN;
        assert!(!scene.is_valid());
    }
    #[test]
    fn fixture_and_retained_budget_match() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../native-render-v1.json")).unwrap();
        assert_eq!(fixture["capability"], CAPABILITY);
        assert_eq!(fixture["max_boxes"], MAX_BOXES);
        assert_eq!(fixture["max_lines"], MAX_LINES);
        assert_eq!(fixture["entity_capability"], ENTITY_HIGHLIGHTS);
        assert_eq!(fixture["block_capability"], BLOCK_HIGHLIGHTS);
        assert_eq!(fixture["block_outline_capability"], BLOCK_OUTLINES);
        assert_eq!(fixture["max_bytes"], MAX_BYTES);
        assert_eq!(fixture["max_blocks"], crate::MAX_BLOCK_HIGHLIGHTS);
        assert_eq!(fixture["max_groups"], crate::drawing::MAX_MASKS);
        assert_eq!(
            fixture["max_group_entities"],
            crate::drawing::MAX_MASK_ENTITIES
        );
        const { assert!(MAX_BOXES * 12 + MAX_LINES <= crate::MAX_DRAW_INSTANCES) };
        let scene: Scene = serde_json::from_value(fixture["scene"].clone()).unwrap();
        assert!(scene.is_valid());
        let encoded = serde_json::to_vec(&scene).unwrap();
        assert_eq!(serde_json::from_slice::<Scene>(&encoded).unwrap(), scene);
        let frame: Frame = serde_json::from_value(fixture["input"].clone()).unwrap();
        assert_eq!(serde_json::to_value(frame).unwrap(), fixture["input"]);
    }
}
