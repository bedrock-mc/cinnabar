//! Connection-state collision boxes for fences and thin panes/bars.

use assets::RegistryRecord;
use assets::block_shapes::ShapeBox;

/// Uses the registry's connection traits instead of its reduced legacy shape.
pub(super) fn shapes(record: &RegistryRecord) -> Option<Vec<ShapeBox>> {
    let name = record.name.strip_prefix("minecraft:")?;
    let pane = name == "iron_bars" || name == "glass_pane" || name.ends_with("_stained_glass_pane");
    if !pane && !name.ends_with("_fence") {
        return None;
    }
    let state: serde_json::Value = serde_json::from_str(&record.canonical_state).ok()?;
    let connected = |direction| {
        state
            .get(format!("minecraft:connection_{direction}"))?
            .get("value")?
            .as_i64()
            .map(|value| value != 0)
    };
    let links = [
        connected("west")?,
        connected("east")?,
        connected("north")?,
        connected("south")?,
    ];
    let inset = if pane { 0.4375 } else { 0.375 };
    let height = if pane { 1.0 } else { 1.5 };
    if !links.into_iter().any(|link| link) {
        return Some(vec![ShapeBox::new(
            [inset, 0.0, inset],
            [1.0 - inset, height, 1.0 - inset],
        )]);
    }
    let mut boxes = Vec::with_capacity(2);
    // Native pane emission is X then Z; fences emit Z then X.
    for axis in if pane { [0, 2] } else { [2, 0] } {
        let [negative, positive] = [links[axis], links[axis + 1]];
        if !negative && !positive {
            continue;
        }
        let mut min = [inset, 0.0, inset];
        let mut max = [1.0 - inset, height, 1.0 - inset];
        min[axis] = if negative {
            0.0
        } else if pane {
            0.5
        } else {
            inset
        };
        max[axis] = if positive {
            1.0
        } else if pane {
            0.5
        } else {
            1.0 - inset
        };
        boxes.push(ShapeBox::new(min, max));
    }
    Some(boxes)
}
