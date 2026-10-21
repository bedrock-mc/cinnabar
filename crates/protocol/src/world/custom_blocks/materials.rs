//! Material instances of a custom block's visual components.

use super::{CustomMaterialInstance, Nbt};

/// Material instances one component set may define before extras are ignored.
const MAX_MATERIAL_INSTANCES: usize = 64;

/// Reads the `materials` table and applies its `mappings` aliases; `None` without a table.
pub(super) fn instances(component: Option<&Nbt>) -> Option<Box<[CustomMaterialInstance]>> {
    component
        .and_then(|instances| match instances.field("materials") {
            Some(Nbt::Compound(fields)) => Some(fields),
            _ => None,
        })
        .map(|fields| {
            let mut instances: Vec<_> = fields
                .iter()
                .take(MAX_MATERIAL_INSTANCES)
                .filter_map(|(name, material)| {
                    let Some(Nbt::String(texture)) = material.field("texture") else {
                        return None;
                    };
                    let render_method = match material.field("render_method") {
                        Some(Nbt::String(method)) => Some(method.as_str().into()),
                        _ => None,
                    };
                    let tint_method = match material.field("tint_method") {
                        Some(Nbt::String(method)) => Some(method.as_str().into()),
                        _ => None,
                    };
                    Some(CustomMaterialInstance {
                        name: name.as_str().into(),
                        texture: texture.as_str().into(),
                        render_method,
                        tint_method,
                        ambient_occlusion: material
                            .field("ambient_occlusion")
                            .and_then(Nbt::number)
                            .map(|value| value as f32)
                            .filter(|value| value.is_finite() && *value >= 0.0),
                        face_dimming: material
                            .field("packed_bools")
                            .and_then(Nbt::number)
                            .filter(|value| value.is_finite())
                            .map(|value| value as i64 & 1 != 0)
                            .or_else(|| {
                                material
                                    .field("face_dimming")
                                    .and_then(Nbt::number)
                                    .filter(|value| matches!(value, 0.0 | 1.0))
                                    .map(|value| value != 0.0)
                            }),
                    })
                })
                .collect();
            alias_materials(&mut instances, component);
            instances.into_boxed_slice()
        })
}

/// Copies each `mappings` target instance under its alias name, replacing an instance of that
/// name, as the client builds its material table; an alias to a missing instance is ignored.
fn alias_materials(instances: &mut Vec<CustomMaterialInstance>, component: Option<&Nbt>) {
    let Some(Nbt::Compound(mappings)) = component.and_then(|component| component.field("mappings"))
    else {
        return;
    };
    for (alias, mapping) in mappings.iter().take(MAX_MATERIAL_INSTANCES) {
        let Some(target) = mapping.field("value").and_then(Nbt::as_str) else {
            continue;
        };
        let Some(instance) = instances
            .iter()
            .find(|instance| instance.name.as_ref() == target)
            .cloned()
        else {
            continue;
        };
        let aliased = CustomMaterialInstance {
            name: alias.as_str().into(),
            ..instance
        };
        match instances
            .iter_mut()
            .find(|instance| instance.name.as_ref() == alias.as_str())
        {
            Some(existing) => *existing = aliased,
            None => instances.push(aliased),
        }
    }
}
