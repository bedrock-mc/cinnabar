use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use super::geometry::cuboid_quads;
use super::state::{exact_tagged_byte, exact_tagged_int, exact_tagged_string};
use {
    super::super::*,
    assets::{BlockFace, door::DoorPose},
};

/// A class keeps distinct topology even when a pack shares its artwork with another door.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::compiler) struct TemplateKey {
    name: Box<str>,
    materials: [u32; 2],
}

/// Admits the complete typed door state and its matching compiled selectors.
fn state(record: &RegistryRecord) -> Option<DoorPose> {
    let state: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&record.canonical_state).ok()?;
    if state.len() != 4 {
        return None;
    }
    let direction = if let Some(value) = state.get("minecraft:cardinal_direction") {
        match exact_tagged_string(value)? {
            "south" => 0,
            "west" => 1,
            "north" => 2,
            "east" => 3,
            _ => return None,
        }
    } else {
        exact_tagged_int(state.get("direction")?, 3)?
    };
    let open = exact_tagged_byte(state.get("open_bit")?, 1)?;
    let hinge = exact_tagged_byte(state.get("door_hinge_bit")?, 1)?;
    let upper = exact_tagged_byte(state.get("upper_block_bit")?, 1)?;
    for (field, value) in [
        (ModelStateField::Orientation, direction),
        (ModelStateField::Open, u32::from(open)),
        (ModelStateField::Hinge, u32::from(hinge)),
        (ModelStateField::Flags, u32::from(upper) << 7),
    ] {
        if record.model_state.get(field)? != value {
            return None;
        }
    }
    Some(DoorPose {
        direction,
        open: open != 0,
        hinge_right: hinge != 0,
        upper: upper != 0,
    })
}

/// Compiles all poses so world meshing can combine selectors across door halves.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<TemplateKey, u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_door(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    if let Some(pose) = state(record)
        && let (Some(lower), Some(upper)) = (
            inputs.material(record, BlockFace::Down),
            inputs.material(record, BlockFace::South),
        )
    {
        let key = TemplateKey {
            name: record.name.clone(),
            materials: [lower, upper],
        };
        let base = if let Some(&base) = templates.get(&key) {
            base
        } else {
            let base = u32::try_from(storage.templates.len()).map_err(|_| {
                AssetError::BlobSizeOverflow {
                    section: "door template",
                }
            })?;
            for offset in 0..assets::door::TEMPLATE_COUNT {
                let pose = DoorPose::from_offset(offset).expect("bounded door pose");
                let material = if pose.upper { upper } else { lower };
                let (min, max) = door_bounds(
                    pose.direction,
                    u32::from(pose.open),
                    u32::from(pose.hinge_right),
                );
                push_model_template(
                    cuboid_quads([material; 6], min, max).to_vec(),
                    assets::MODEL_TEMPLATE_FLAG_DOOR,
                    storage.templates,
                    storage.quads,
                )?;
            }
            templates.insert(key, base);
            base
        };
        set_model_visual(
            &mut visual,
            [if pose.upper { upper } else { lower }; 6],
            base + pose.offset(),
        );
        visual.variant = pose.offset() << 2;
    }
    Ok(CompileRuleResult::Compiled(visual))
}

/// Builds the native door plane in fixed-point model coordinates.
pub(in crate::compiler) fn door_bounds(
    direction: u32,
    open: u32,
    hinge: u32,
) -> ([i16; 3], [i16; 3]) {
    const THICKNESS: i16 = (assets::door::THICKNESS * 256.0 + 0.5) as i16;
    const HIGH: i16 = 256 - THICKNESS;
    let plane = DoorPose {
        direction,
        open: open != 0,
        hinge_right: hinge != 0,
        upper: false,
    }
    .plane();
    match plane {
        0 => ([0, 0, 0], [256, 256, THICKNESS]),
        1 => ([HIGH, 0, 0], [256, 256, 256]),
        2 => ([0, 0, HIGH], [256, 256, 256]),
        _ => ([0, 0, 0], [THICKNESS, 256, 256]),
    }
}
