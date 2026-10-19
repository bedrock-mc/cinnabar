//! Compiles literal equipment channels from one animation document at a time.

use super::*;

struct Request {
    binding: usize,
    alias: String,
    clip: Box<str>,
    slots: &'static [Option<&'static str>],
}

/// Fills final binding products and releases each animation document before reading the next.
pub(super) fn compile(
    payloads: &SourcePayloads,
    symbols: &[EntityAssetSymbol],
    sources: &[EntityAssetSource],
    attachables: &mut [ParsedAttachable],
) {
    let sources = animation_source_index(symbols, sources);
    for (source, requests) in requests(attachables, &sources) {
        let Some(bytes) = payloads.get(source.as_ref()) else {
            continue;
        };
        #[cfg(test)]
        ANIMATION_PARSES.update(|count| count + 1);
        let Ok(document) = pack_json::parse_jsonc::<Value>(bytes) else {
            continue;
        };
        for request in requests {
            let Some(bones) = document
                .get("animations")
                .and_then(|clips| clips.get(request.clip.as_ref()))
                .and_then(|clip| clip.get("bones"))
                .and_then(Value::as_object)
            else {
                continue;
            };
            apply(&mut attachables[request.binding], &request, bones);
        }
    }
    for attachable in attachables {
        attachable
            .poses
            .sort_by(|left, right| left.key.cmp(&right.key));
    }
}

/// Groups only requested clips using the existing first-source identifier selection.
fn requests(
    attachables: &mut [ParsedAttachable],
    sources: &BTreeMap<&str, &str>,
) -> BTreeMap<Box<str>, Vec<Request>> {
    let mut requests = BTreeMap::<Box<str>, Vec<Request>>::new();
    for (binding, attachable) in attachables.iter_mut().enumerate() {
        let slots: &'static [Option<&'static str>] =
            match category(&attachable.item_identifier, &attachable.geometry) {
                EquipmentCategory::Elytra => &[None],
                EquipmentCategory::Shield => &[Some("main_hand"), Some("off_hand")],
                _ => &[],
            };
        for (alias, clip) in std::mem::take(&mut attachable.animations) {
            if slots.is_empty()
                && !matches!(alias.as_str(), "wield_first_person" | "wield_third_person")
            {
                continue;
            }
            let Some(source) = sources.get(clip.as_ref()) else {
                continue;
            };
            requests.entry((*source).into()).or_default().push(Request {
                binding,
                alias,
                clip,
                slots,
            });
        }
    }
    requests
}

/// Writes only requested final transforms and poses, without retaining any JSON node.
fn apply(
    attachable: &mut ParsedAttachable,
    request: &Request,
    bones: &serde_json::Map<String, Value>,
) {
    if matches!(
        request.alias.as_str(),
        "wield_first_person" | "wield_third_person"
    ) {
        let transform = match literal_bone_transform(bones) {
            Some(transform) => EquipmentTransform::Literal { transform },
            None => EquipmentTransform::NeedsMeasurement,
        };
        match request.alias.as_str() {
            "wield_first_person" => attachable.first_person = transform,
            _ => attachable.third_person = transform,
        }
    }
    for slot in request.slots {
        let Some(bones) = literal_clip_bones(bones, *slot) else {
            continue;
        };
        let key = match slot {
            Some(slot) => format!("{}@{slot}", request.alias),
            None => request.alias.clone(),
        };
        if let Ok(key) = bounded_identifier(&key) {
            attachable.poses.push(AttachablePose { key, bones });
        }
    }
}

/// A literal display transform from a single-bone clip whose position and
/// rotation (and scale, if any) are pure numeric arrays; Molang strings -> None.
fn literal_bone_transform(bones: &serde_json::Map<String, Value>) -> Option<ItemDisplayTransform> {
    if bones.len() != 1 {
        return None;
    }
    let bone = bones.values().next()?;
    let translation = numeric_vec3(bone.get("position"))?;
    let rotation = numeric_vec3(bone.get("rotation"))?;
    let scale = match bone.get("scale") {
        None => [scalar(1.0)?; 3],
        Some(scale) => numeric_vec3(Some(scale))?,
    };
    Some(ItemDisplayTransform {
        translation,
        rotation,
        scale,
    })
}

/// Extracts literal pose channels, resolving supported hand-dependent values for the slot.
fn literal_clip_bones(
    bones: &serde_json::Map<String, Value>,
    slot: Option<&str>,
) -> Option<Box<[AttachablePoseBone]>> {
    let channel = |bone: &Value, name: &str| -> Option<Option<[ItemDisplayScalar; 3]>> {
        match bone.get(name) {
            None => Some(None),
            // A bare number is a uniform value on every axis.
            Some(Value::Number(number)) => {
                let value = scalar(number.as_f64()? as f32)?;
                Some(Some([value; 3]))
            }
            Some(Value::Array(array)) if array.len() == 3 => {
                let mut out = [scalar(0.0)?; 3];
                for (target, element) in out.iter_mut().zip(array) {
                    *target = scalar(slot_number(element, slot)?)?;
                }
                Some(Some(out))
            }
            Some(_) => None,
        }
    };
    let mut out = Vec::new();
    for (name, bone) in bones {
        out.push(AttachablePoseBone {
            bone: bounded_identifier(name).ok()?,
            translation: channel(bone, "position")?,
            rotation: channel(bone, "rotation")?,
            scale: channel(bone, "scale")?,
        });
    }
    (!out.is_empty() && out.len() <= 32).then(|| out.into_boxed_slice())
}

/// A JSON number, or `c.item_slot == 'name' ? a : b` with literal branches resolved for `slot`.
fn slot_number(value: &Value, slot: Option<&str>) -> Option<f32> {
    match value {
        Value::Number(number) => number.as_f64().map(|value| value as f32),
        Value::String(text) => {
            let compact = text
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect::<String>();
            let rest = compact.strip_prefix("c.item_slot==")?;
            let (name, rest) = rest.strip_prefix('\'')?.split_once('\'')?;
            let (when_true, when_false) = rest.strip_prefix('?')?.split_once(':')?;
            let chosen = if slot? == name { when_true } else { when_false };
            chosen.parse::<f32>().ok()
        }
        _ => None,
    }
}

/// Accepts exactly three literal numeric components.
fn numeric_vec3(value: Option<&Value>) -> Option<[ItemDisplayScalar; 3]> {
    let array = value?.as_array()?;
    if array.len() != 3 {
        return None;
    }
    let mut out = [scalar(0.0)?; 3];
    for (slot, element) in out.iter_mut().zip(array) {
        *slot = scalar(element.as_f64()? as f32)?;
    }
    Some(out)
}

/// Rejects scalar values outside the carrier's finite numeric representation.
fn scalar(value: f32) -> Option<ItemDisplayScalar> {
    ItemDisplayScalar::new(value)
}
