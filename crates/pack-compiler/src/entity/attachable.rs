//! `minecraft:attachable` ingestion: item identifier -> entity-catalog geometry,
//! texture, material, render controller, and any literal display transform.
//!
//! Armor ships a mob variant (`geometry.humanoid.armor.*`, uncollected) and a
//! `.player.json` variant selected by `query.owner_identifier == 'minecraft:player'`
//! (`geometry.player.armor.*`, collected); the player variant wins so its geometry
//! resolves in-catalog. Transforms are populated only from pure-numeric bone
//! keyframes in the exact `wield_first_person`/`wield_third_person` base clips;
//! anything Molang/query-derived is left `NeedsMeasurement`.

use std::{collections::BTreeMap, path::Path};

use assets::{
    ArmorSlot, AssetError, AttachablePose, AttachablePoseBone, EntityAssetKind, EntityAssetSource,
    EntityAssetSymbol, EntityDependencyResolution, EquipmentBinding, EquipmentCategory,
    EquipmentReference, EquipmentTexture, EquipmentTransform, ItemDisplayScalar,
    ItemDisplayTransform, ItemUseDuration, MAX_EQUIPMENT_BINDINGS, MAX_EQUIPMENT_IDENTIFIER_BYTES,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{SourcePayloads, invalid, json::parse_unique_json, read_bounded_source};
mod item_attack;
mod textures;
pub use item_attack::compile_item_attack_timings;
use textures::compile_texture_identifiers_with;
pub use textures::{compile_textures_for_assets, compile_textures_for_assets_with};

/// Literal transforms an item's attachable exposed, keyed by item identifier.
pub(super) struct ItemTransforms {
    pub(super) first_person: Option<ItemDisplayTransform>,
    pub(super) third_person: Option<ItemDisplayTransform>,
    pub(super) dropped: Option<ItemDisplayTransform>,
}

struct ParsedAttachable {
    item_identifier: Box<str>,
    prefer: bool,
    geometry: Box<str>,
    texture: Box<str>,
    material: Box<str>,
    render_controller: Box<str>,
    first_person: EquipmentTransform,
    third_person: EquipmentTransform,
    poses: Box<[AttachablePose]>,
}

/// Validates one attachable source's shape during collection.
pub(super) fn validate_source(value: &Value) -> Result<(), AttachableError> {
    parse_description(value).map(|_| ())
}

/// Builds the sorted per-item binding table from every collected attachable.
pub(super) fn compile_bindings(
    payloads: &SourcePayloads,
    symbols: &[EntityAssetSymbol],
    sources: &[EntityAssetSource],
    lenient: bool,
) -> Result<Box<[EquipmentBinding]>, AttachableError> {
    let animation_sources = animation_source_index(symbols, sources);
    let geometry_symbols = symbol_identifiers(symbols, EntityAssetKind::Geometry);
    let texture_symbols = symbol_identifiers(symbols, EntityAssetKind::Texture);
    let mut chosen = BTreeMap::<Box<str>, ParsedAttachable>::new();
    for (path, bytes) in payloads {
        if !path.starts_with("attachables/") {
            continue;
        }
        let parsed = match parse_unique_json(Path::new(path.as_ref()), bytes)
            .map_err(|_| AttachableError::Malformed)
            .and_then(|value| parse_attachable(&value, payloads, &animation_sources))
        {
            Ok(parsed) => parsed,
            // A server pack's bad attachable drops only itself.
            Err(_) if lenient => continue,
            Err(error) => return Err(error),
        };
        match chosen.get(&parsed.item_identifier) {
            // The player variant wins so its collected geometry resolves in-catalog.
            Some(existing) if existing.prefer && !parsed.prefer => {}
            _ => {
                chosen.insert(parsed.item_identifier.clone(), parsed);
            }
        }
    }
    if chosen.len() > MAX_EQUIPMENT_BINDINGS {
        if !lenient {
            return Err(AttachableError::TooMany);
        }
        let keep = chosen
            .keys()
            .take(MAX_EQUIPMENT_BINDINGS)
            .cloned()
            .collect::<Vec<_>>();
        chosen.retain(|identifier, _| keep.contains(identifier));
    }
    let bindings = chosen
        .into_values()
        .map(|parsed| binding(parsed, &geometry_symbols, &texture_symbols))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(bindings.into_boxed_slice())
}

/// Literal transforms keyed by item identifier, for `item::compile`.
pub(super) fn transform_lookup(
    bindings: &[EquipmentBinding],
) -> BTreeMap<Box<str>, ItemTransforms> {
    bindings
        .iter()
        .map(|binding| {
            (
                binding.identifier.clone(),
                ItemTransforms {
                    first_person: binding.first_person.literal(),
                    third_person: binding.third_person.literal(),
                    dropped: binding.dropped.literal(),
                },
            )
        })
        .collect()
}

/// Item use durations the behavior pack states, sorted by identifier: `use_modifiers.use_duration`
/// in seconds or the older `minecraft:use_duration` in ticks. Unreadable items are skipped.
pub fn compile_item_use(behavior_pack: &Path) -> Result<Vec<ItemUseDuration>, AssetError> {
    let mut durations = BTreeMap::<Box<str>, u32>::new();
    for (identifier, components) in item_attack::components(behavior_pack)? {
        let seconds = components
            .pointer("/minecraft:use_modifiers/use_duration")
            .and_then(Value::as_f64)
            .map(|seconds| seconds * 20.0);
        let ticks = seconds.or_else(|| {
            components
                .get("minecraft:use_duration")
                .and_then(Value::as_f64)
        });
        let Some(ticks) = ticks.filter(|ticks| ticks.is_finite() && *ticks >= 1.0) else {
            continue;
        };
        let Ok(ticks) = u32::try_from(ticks.round() as u64) else {
            continue;
        };
        durations.insert(identifier, ticks);
    }
    Ok(durations
        .into_iter()
        .map(|(identifier, ticks)| ItemUseDuration { identifier, ticks })
        .collect())
}

/// Decodes every binding's default texture found among the collected sources, sorted by
/// identifier. Bindings whose raster is not collected are skipped, never fatal.
pub fn compile_textures(
    root: &Path,
    sources: &[EntityAssetSource],
    bindings: &[EquipmentBinding],
) -> Result<Vec<EquipmentTexture>, AssetError> {
    compile_textures_with(sources, bindings, &mut |source| {
        let bytes = read_bounded_source(root, &root.join(source.path.as_ref()))?;
        if bytes.len() != source.source_bytes as usize
            || <[u8; 32]>::from(Sha256::digest(&bytes)) != source.source_sha256
        {
            return Err(invalid("equipment raster changed after entity compilation"));
        }
        Ok(bytes)
    })
}

/// Like [`compile_textures`], reading each raster through `read` (for in-memory packs).
pub fn compile_textures_with(
    sources: &[EntityAssetSource],
    bindings: &[EquipmentBinding],
    read: &mut dyn FnMut(&EntityAssetSource) -> Result<Vec<u8>, AssetError>,
) -> Result<Vec<EquipmentTexture>, AssetError> {
    let identifiers = bindings
        .iter()
        .map(|binding| binding.texture.identifier.as_ref())
        .collect::<Vec<_>>();
    compile_texture_identifiers_with(sources, identifiers, read)
}

fn binding(
    parsed: ParsedAttachable,
    geometry_symbols: &BTreeMap<&str, ()>,
    texture_symbols: &BTreeMap<&str, ()>,
) -> Result<EquipmentBinding, AttachableError> {
    let category = category(&parsed.item_identifier, &parsed.geometry);
    Ok(EquipmentBinding {
        geometry: reference(parsed.geometry, geometry_symbols),
        texture: reference(parsed.texture, texture_symbols),
        material: parsed.material,
        render_controller: parsed.render_controller,
        first_person: parsed.first_person,
        third_person: parsed.third_person,
        poses: parsed.poses,
        dropped: EquipmentTransform::NeedsMeasurement,
        category,
        identifier: parsed.item_identifier,
    })
}

fn reference(identifier: Box<str>, catalog: &BTreeMap<&str, ()>) -> EquipmentReference {
    let resolution = if catalog.contains_key(identifier.as_ref()) {
        EntityDependencyResolution::Catalog
    } else {
        EntityDependencyResolution::External
    };
    EquipmentReference {
        identifier,
        resolution,
    }
}

fn category(item_identifier: &str, geometry: &str) -> EquipmentCategory {
    if item_identifier == "minecraft:shield" || geometry == "geometry.shield" {
        return EquipmentCategory::Shield;
    }
    if item_identifier == "minecraft:elytra" || geometry == assets::ELYTRA_GEOMETRY_IDENTIFIER {
        return EquipmentCategory::Elytra;
    }
    let armor = |needle: &str| item_identifier.contains(needle) || geometry.contains(needle);
    if armor("helmet") {
        EquipmentCategory::Armor {
            slot: ArmorSlot::Helmet,
        }
    } else if armor("chestplate") {
        EquipmentCategory::Armor {
            slot: ArmorSlot::Chestplate,
        }
    } else if armor("leggings") {
        EquipmentCategory::Armor {
            slot: ArmorSlot::Leggings,
        }
    } else if armor("boots") {
        EquipmentCategory::Armor {
            slot: ArmorSlot::Boots,
        }
    } else {
        EquipmentCategory::Held
    }
}

fn parse_attachable(
    value: &Value,
    payloads: &SourcePayloads,
    animation_sources: &BTreeMap<&str, &str>,
) -> Result<ParsedAttachable, AttachableError> {
    let description = parse_description(value)?;
    let (item_identifier, prefer) = item_binding(description)?;
    let animations = description.get("animations").and_then(Value::as_object);
    let geometry = named_default(description, "geometry")?;
    // Elytra and shields pose their own bones from literal clips; a shield's clip picks values by
    // the hand it is held in, so it stores one pose per hand.
    let poses = match category(&item_identifier, &geometry) {
        EquipmentCategory::Elytra => {
            literal_poses(animations, payloads, animation_sources, &[None])
        }
        EquipmentCategory::Shield => literal_poses(
            animations,
            payloads,
            animation_sources,
            &[Some("main_hand"), Some("off_hand")],
        ),
        _ => Box::new([]),
    };
    Ok(ParsedAttachable {
        poses,
        geometry,
        texture: named_default(description, "textures")?,
        material: named_default(description, "materials")?,
        render_controller: first_render_controller(description)?,
        first_person: wield_transform(
            animations,
            "wield_first_person",
            payloads,
            animation_sources,
        ),
        third_person: wield_transform(
            animations,
            "wield_third_person",
            payloads,
            animation_sources,
        ),
        item_identifier,
        prefer,
    })
}

fn parse_description(value: &Value) -> Result<&Value, AttachableError> {
    let root = value.as_object().ok_or(AttachableError::Malformed)?;
    if root.len() != 2 || !root.contains_key("format_version") {
        return Err(AttachableError::Malformed);
    }
    value
        .get("minecraft:attachable")
        .and_then(|attachable| attachable.get("description"))
        .filter(|description| description.is_object())
        .ok_or(AttachableError::Malformed)
}

/// The item id the attachable binds to, and whether it is the player variant.
fn item_binding(description: &Value) -> Result<(Box<str>, bool), AttachableError> {
    if let Some(item) = description.get("item").and_then(Value::as_object) {
        let mut keys = item.keys();
        let (Some(identifier), None) = (keys.next(), keys.next()) else {
            return Err(AttachableError::Malformed);
        };
        return Ok((bounded_identifier(identifier)?, true));
    }
    let identifier = description
        .get("identifier")
        .and_then(Value::as_str)
        .ok_or(AttachableError::Malformed)?;
    Ok((bounded_identifier(identifier)?, false))
}

fn named_default(description: &Value, field: &str) -> Result<Box<str>, AttachableError> {
    description
        .get(field)
        .and_then(|map| map.get("default"))
        .and_then(Value::as_str)
        .map(bounded_identifier)
        .ok_or(AttachableError::Malformed)?
}

fn first_render_controller(description: &Value) -> Result<Box<str>, AttachableError> {
    let entry = description
        .get("render_controllers")
        .and_then(Value::as_array)
        .and_then(|controllers| controllers.first())
        .ok_or(AttachableError::Malformed)?;
    let identifier = match entry {
        Value::String(identifier) => identifier.as_str(),
        Value::Object(conditional) => conditional
            .keys()
            .next()
            .map(String::as_str)
            .ok_or(AttachableError::Malformed)?,
        _ => return Err(AttachableError::Malformed),
    };
    bounded_identifier(identifier)
}

fn wield_transform(
    animations: Option<&serde_json::Map<String, Value>>,
    local_key: &str,
    payloads: &SourcePayloads,
    animation_sources: &BTreeMap<&str, &str>,
) -> EquipmentTransform {
    let literal = animations
        .and_then(|animations| animations.get(local_key))
        .and_then(Value::as_str)
        .and_then(|identifier| literal_bone_transform(identifier, payloads, animation_sources));
    match literal {
        Some(transform) => EquipmentTransform::Literal { transform },
        None => EquipmentTransform::NeedsMeasurement,
    }
}

/// A literal display transform from a single-bone clip whose position and
/// rotation (and scale, if any) are pure numeric arrays; Molang strings -> None.
fn literal_bone_transform(
    identifier: &str,
    payloads: &SourcePayloads,
    animation_sources: &BTreeMap<&str, &str>,
) -> Option<ItemDisplayTransform> {
    let source = animation_sources.get(identifier)?;
    let bytes = payloads.get(*source)?;
    let document: Value = serde_json::from_slice(bytes).ok()?;
    let bones = document
        .get("animations")?
        .get(identifier)?
        .get("bones")?
        .as_object()?;
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

/// Every animation of the attachable whose bone channels are all literal, sorted by key. A slot
/// resolves `c.item_slot == 'name' ? a : b` channels and suffixes the key with `@slot`. Clips
/// with any other Molang are left out (`NeedsMeasurement` at the consumer).
fn literal_poses(
    animations: Option<&serde_json::Map<String, Value>>,
    payloads: &SourcePayloads,
    animation_sources: &BTreeMap<&str, &str>,
    slots: &[Option<&str>],
) -> Box<[AttachablePose]> {
    let mut poses = Vec::new();
    for (key, identifier) in animations.into_iter().flatten() {
        let Some(identifier) = identifier.as_str() else {
            continue;
        };
        for slot in slots {
            let Some(bones) = literal_clip_bones(identifier, payloads, animation_sources, *slot)
            else {
                continue;
            };
            let key = match slot {
                Some(slot) => format!("{key}@{slot}"),
                None => key.clone(),
            };
            if let Ok(key) = bounded_identifier(&key) {
                poses.push(AttachablePose { key, bones });
            }
        }
    }
    poses.sort_by(|left, right| left.key.cmp(&right.key));
    poses.into_boxed_slice()
}

fn literal_clip_bones(
    identifier: &str,
    payloads: &SourcePayloads,
    animation_sources: &BTreeMap<&str, &str>,
    slot: Option<&str>,
) -> Option<Box<[AttachablePoseBone]>> {
    let source = animation_sources.get(identifier)?;
    let document: Value = serde_json::from_slice(payloads.get(*source)?).ok()?;
    let bones = document
        .get("animations")?
        .get(identifier)?
        .get("bones")?
        .as_object()?;
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

fn scalar(value: f32) -> Option<ItemDisplayScalar> {
    ItemDisplayScalar::new(value)
}

fn animation_source_index<'a>(
    symbols: &'a [EntityAssetSymbol],
    sources: &'a [EntityAssetSource],
) -> BTreeMap<&'a str, &'a str> {
    let mut index = BTreeMap::new();
    for symbol in symbols {
        if symbol.kind != EntityAssetKind::Animation {
            continue;
        }
        if let Some(source) = sources.get(symbol.source_index as usize) {
            index
                .entry(symbol.identifier.as_ref())
                .or_insert(source.path.as_ref());
        }
    }
    index
}

fn symbol_identifiers(symbols: &[EntityAssetSymbol], kind: EntityAssetKind) -> BTreeMap<&str, ()> {
    symbols
        .iter()
        .filter(|symbol| symbol.kind == kind)
        .map(|symbol| (symbol.identifier.as_ref(), ()))
        .collect()
}

fn bounded_identifier(identifier: &str) -> Result<Box<str>, AttachableError> {
    if identifier.is_empty()
        || identifier.len() > MAX_EQUIPMENT_IDENTIFIER_BYTES
        || identifier.chars().any(char::is_control)
    {
        return Err(AttachableError::Malformed);
    }
    Ok(identifier.into())
}

#[derive(Debug)]
pub(super) enum AttachableError {
    Malformed,
    TooMany,
}

impl From<AttachableError> for assets::AssetError {
    fn from(error: AttachableError) -> Self {
        let detail = match error {
            AttachableError::Malformed => "malformed attachable source",
            AttachableError::TooMany => "attachable binding count exceeds bound",
        };
        invalid(detail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assets::EquipmentCategory;

    fn payloads(entries: &[(&str, &str)]) -> SourcePayloads {
        entries
            .iter()
            .map(|(path, bytes)| ((*path).into(), bytes.as_bytes().to_vec().into_boxed_slice()))
            .collect()
    }

    fn symbols(entries: &[(EntityAssetKind, &str, u32)]) -> Vec<EntityAssetSymbol> {
        entries
            .iter()
            .map(|(kind, identifier, source_index)| EntityAssetSymbol {
                kind: *kind,
                identifier: (*identifier).into(),
                source_index: *source_index,
                dependencies: Box::new([]),
            })
            .collect()
    }

    fn sources(paths: &[&str]) -> Vec<EntityAssetSource> {
        paths
            .iter()
            .map(|path| EntityAssetSource {
                path: (*path).into(),
                source_bytes: 1,
                source_sha256: [0; 32],
            })
            .collect()
    }

    const TRIDENT: &str = r#"{"format_version":"1.10","minecraft:attachable":{"description":{
        "identifier":"minecraft:trident",
        "materials":{"default":"entity_alphatest"},
        "textures":{"default":"textures/entity/trident"},
        "geometry":{"default":"geometry.trident"},
        "animations":{"wield_first_person":"animation.trident.wield_first_person","wield_third_person":"animation.trident.wield_third_person"},
        "render_controllers":["controller.render.item_default"]}}}"#;

    const TRIDENT_ANIM: &str = r#"{"format_version":"1.10.0","animations":{
        "animation.trident.wield_first_person":{"loop":true,"bones":{"pole":{"position":[-7.0,-3.0,-2.0],"rotation":[152.0,-9.0,25.0]}}},
        "animation.trident.wield_third_person":{"loop":true,"bones":{"pole":{"position":[1.5,-2.5,-10.5],"rotation":[97.0,-1.5,-49.0]}}}}}"#;

    const HELMET_MOB: &str = r#"{"format_version":"1.8.0","minecraft:attachable":{"description":{
        "identifier":"minecraft:diamond_helmet",
        "materials":{"default":"armor"},
        "textures":{"default":"textures/models/armor/diamond_1"},
        "geometry":{"default":"geometry.humanoid.armor.helmet"},
        "render_controllers":["controller.render.armor"]}}}"#;

    const HELMET_PLAYER: &str = r#"{"format_version":"1.10.0","minecraft:attachable":{"description":{
        "identifier":"minecraft:diamond_helmet.player",
        "item":{"minecraft:diamond_helmet":"query.owner_identifier == 'minecraft:player'"},
        "materials":{"default":"armor"},
        "textures":{"default":"textures/models/armor/diamond_1"},
        "geometry":{"default":"geometry.player.armor.helmet"},
        "animations":{"offset":"animation.armor.helmet.offset"},
        "render_controllers":["controller.render.armor"]}}}"#;

    #[test]
    fn trident_carries_literal_first_and_third_person_transforms() {
        let payloads = payloads(&[
            ("attachables/trident.entity.json", TRIDENT),
            ("animations/trident.animation.json", TRIDENT_ANIM),
        ]);
        let symbols = symbols(&[
            (EntityAssetKind::Geometry, "geometry.trident", 1),
            (EntityAssetKind::Texture, "textures/entity/trident", 2),
            (
                EntityAssetKind::Animation,
                "animation.trident.wield_first_person",
                0,
            ),
            (
                EntityAssetKind::Animation,
                "animation.trident.wield_third_person",
                0,
            ),
        ]);
        let sources = sources(&["animations/trident.animation.json"]);
        let bindings = compile_bindings(&payloads, &symbols, &sources, false).unwrap();
        assert_eq!(bindings.len(), 1);
        let trident = &bindings[0];
        assert_eq!(trident.category, EquipmentCategory::Held);
        assert_eq!(
            trident.geometry.resolution,
            EntityDependencyResolution::Catalog
        );
        assert_eq!(
            trident.texture.resolution,
            EntityDependencyResolution::Catalog
        );
        let first = trident.first_person.literal().unwrap();
        assert_eq!(first.translation[0].get(), -7.0);
        assert_eq!(first.rotation[2].get(), 25.0);
        assert_eq!(first.scale[1].get(), 1.0);
        let third = trident.third_person.literal().unwrap();
        assert_eq!(third.translation[2].get(), -10.5);
        assert!(trident.dropped.literal().is_none());
    }

    #[test]
    fn player_armor_variant_wins_and_resolves_in_catalog() {
        let payloads = payloads(&[
            ("attachables/diamond_helmet.json", HELMET_MOB),
            ("attachables/diamond_helmet.player.json", HELMET_PLAYER),
        ]);
        // Only the player geometry is collected; the mob geometry is external.
        let symbols = symbols(&[(EntityAssetKind::Geometry, "geometry.player.armor.helmet", 0)]);
        let bindings = compile_bindings(&payloads, &symbols, &[], false).unwrap();
        assert_eq!(bindings.len(), 1);
        let helmet = &bindings[0];
        assert_eq!(helmet.identifier.as_ref(), "minecraft:diamond_helmet");
        assert_eq!(
            helmet.geometry.identifier.as_ref(),
            "geometry.player.armor.helmet"
        );
        assert_eq!(
            helmet.geometry.resolution,
            EntityDependencyResolution::Catalog
        );
        assert_eq!(
            helmet.category,
            EquipmentCategory::Armor {
                slot: ArmorSlot::Helmet
            }
        );
        // Armor offsets are Molang locator queries, never literal.
        assert!(helmet.first_person.literal().is_none());
        assert!(helmet.third_person.literal().is_none());
    }

    #[test]
    fn molang_bone_keyframes_stay_needs_measurement() {
        const SHIELD: &str = r#"{"format_version":"1.10.0","minecraft:attachable":{"description":{
            "identifier":"minecraft:shield",
            "materials":{"default":"entity_alphatest"},
            "textures":{"default":"textures/entity/shield"},
            "geometry":{"default":"geometry.shield"},
            "animations":{"wield_third_person":"animation.shield.wield_third_person"},
            "render_controllers":["controller.render.item_default"]}}}"#;
        const SHIELD_ANIM: &str = r#"{"format_version":"1.10.0","animations":{
            "animation.shield.wield_third_person":{"bones":{"shield":{"position":["c.item_slot == 'main_hand' ? -0.4 : -1.6",9.0,9.3],"rotation":[-90.0,0.0,90.0]}}}}}"#;
        let payloads = payloads(&[
            ("attachables/shield.entity.json", SHIELD),
            ("animations/shield.animation.json", SHIELD_ANIM),
        ]);
        let symbols = symbols(&[
            (EntityAssetKind::Geometry, "geometry.shield", 0),
            (
                EntityAssetKind::Animation,
                "animation.shield.wield_third_person",
                0,
            ),
        ]);
        let sources = sources(&["animations/shield.animation.json"]);
        let bindings = compile_bindings(&payloads, &symbols, &sources, false).unwrap();
        assert_eq!(bindings[0].category, EquipmentCategory::Shield);
        assert!(bindings[0].third_person.literal().is_none());
    }

    #[test]
    fn elytra_keeps_only_literal_poses_sorted_by_key() {
        const ELYTRA: &str = r#"{"format_version":"1.10.0","minecraft:attachable":{"description":{
            "identifier":"minecraft:elytra",
            "materials":{"default":"elytra"},
            "textures":{"default":"textures/models/armor/elytra"},
            "geometry":{"default":"geometry.elytra"},
            "animations":{"sneaking":"animation.elytra.sneaking","default":"animation.elytra.default","gliding":"animation.elytra.gliding","default_controller":"controller.animation.elytra.default"},
            "render_controllers":["controller.render.armor"]}}}"#;
        const ELYTRA_ANIM: &str = r#"{"format_version":"1.8.0","animations":{
            "animation.elytra.default":{"bones":{"body":{"scale":1.067},"left_wing":{"position":[4.5,4,-2],"rotation":[15,0,-13],"scale":[1,1,2]}}},
            "animation.elytra.sneaking":{"bones":{"left_wing":{"position":[2,3,-2],"rotation":[20,-15,-45]}}},
            "animation.elytra.gliding":{"bones":{"left_wing":{"rotation":["math.lerp(15, 20, 1)",0,0]}}}}}"#;
        let payloads = payloads(&[
            ("attachables/elytra.json", ELYTRA),
            ("animations/elytra.animation.json", ELYTRA_ANIM),
        ]);
        let symbols = symbols(&[
            (EntityAssetKind::Geometry, "geometry.elytra", 1),
            (EntityAssetKind::Texture, "textures/models/armor/elytra", 2),
            (EntityAssetKind::Animation, "animation.elytra.default", 0),
            (EntityAssetKind::Animation, "animation.elytra.sneaking", 0),
            (EntityAssetKind::Animation, "animation.elytra.gliding", 0),
        ]);
        let sources = sources(&["animations/elytra.animation.json"]);
        let bindings = compile_bindings(&payloads, &symbols, &sources, false).unwrap();
        assert_eq!(bindings[0].category, EquipmentCategory::Elytra);
        let keys = bindings[0]
            .poses
            .iter()
            .map(|pose| pose.key.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(keys, ["default", "sneaking"]);
        let default = bindings[0].pose("default").unwrap();
        let body = default
            .bones
            .iter()
            .find(|bone| bone.bone.as_ref() == "body")
            .unwrap();
        assert_eq!(body.scale.unwrap().map(|value| value.get()), [1.067; 3]);
        assert!(body.translation.is_none());
    }

    #[test]
    fn shield_third_person_resolves_the_hand_conditional_into_one_pose_per_hand() {
        const SHIELD: &str = r#"{"format_version":"1.10.0","minecraft:attachable":{"description":{
            "identifier":"minecraft:shield",
            "materials":{"default":"entity_alphatest"},
            "textures":{"default":"textures/entity/shield"},
            "geometry":{"default":"geometry.shield"},
            "animations":{"wield_third_person":"animation.shield.wield_third_person","wield_main_hand_first_person":"animation.shield.wield_main_hand_first_person"},
            "render_controllers":["controller.render.item_default"]}}}"#;
        const SHIELD_ANIM: &str = r#"{"format_version":"1.10.0","animations":{
            "animation.shield.wield_third_person":{"bones":{"shield":{"position":["c.item_slot == 'main_hand' ? -0.4 : -1.6",9.0,9.3],"rotation":[-90.0,0.0,90.0],"scale":["c.item_slot == 'main_hand' ? 1.0 : -1.0",-1.0,"c.item_slot == 'main_hand' ? -1.0 : 1.0"]}}},
            "animation.shield.wield_main_hand_first_person":{"bones":{"shield":{"position":["variable.x",0,0]}}}}}"#;
        let payloads = payloads(&[
            ("attachables/shield.entity.json", SHIELD),
            ("animations/shield.animation.json", SHIELD_ANIM),
        ]);
        let symbols = symbols(&[
            (EntityAssetKind::Geometry, "geometry.shield", 1),
            (EntityAssetKind::Texture, "textures/entity/shield", 2),
            (
                EntityAssetKind::Animation,
                "animation.shield.wield_third_person",
                0,
            ),
            (
                EntityAssetKind::Animation,
                "animation.shield.wield_main_hand_first_person",
                0,
            ),
        ]);
        let sources = sources(&["animations/shield.animation.json"]);
        let bindings = compile_bindings(&payloads, &symbols, &sources, false).unwrap();
        let shield = &bindings[0];
        assert_eq!(shield.category, EquipmentCategory::Shield);
        assert_eq!(shield.poses.len(), 2);
        let main = shield.pose("wield_third_person@main_hand").unwrap();
        let off = shield.pose("wield_third_person@off_hand").unwrap();
        let translation =
            |pose: &AttachablePose| pose.bones[0].translation.unwrap().map(|v| v.get());
        assert_eq!(translation(main), [-0.4, 9.0, 9.3]);
        assert_eq!(translation(off), [-1.6, 9.0, 9.3]);
        assert_eq!(
            off.bones[0].scale.unwrap().map(|v| v.get()),
            [-1.0, -1.0, 1.0]
        );
    }

    #[test]
    fn item_use_durations_read_seconds_and_ticks_and_skip_the_rest() {
        let root = tempfile::tempdir().unwrap();
        let items = root.path().join("items");
        std::fs::create_dir(&items).unwrap();
        let write = |name: &str, body: &str| std::fs::write(items.join(name), body).unwrap();
        write(
            "honey.json",
            r#"{"minecraft:item":{"description":{"identifier":"minecraft:honey_bottle"},
                "components":{"minecraft:use_modifiers":{"use_duration":1.6}}}}"#,
        );
        write(
            "spear.json",
            r#"{"minecraft:item":{"description":{"identifier":"minecraft:iron_spear"},
                "components":{"minecraft:use_duration":72000}}}"#,
        );
        write(
            "seed.json",
            r#"{"minecraft:item":{"description":{"identifier":"minecraft:seeds"},"components":{}}}"#,
        );
        write("broken.json", "{");
        let durations = compile_item_use(root.path()).unwrap();
        let pairs = durations
            .iter()
            .map(|entry| (entry.identifier.as_ref(), entry.ticks))
            .collect::<Vec<_>>();
        assert_eq!(
            pairs,
            [
                ("minecraft:honey_bottle", 32),
                ("minecraft:iron_spear", 72000)
            ]
        );
    }

    /// Comments in a food's unrelated effects must not hide its use duration.
    #[test]
    fn item_use_durations_accept_commented_food_definitions() {
        let root = tempfile::tempdir().unwrap();
        let items = root.path().join("items");
        std::fs::create_dir(&items).unwrap();
        std::fs::write(
            items.join("golden_apple.json"),
            r#"{"minecraft:item":{
                "description":{"identifier":"minecraft:golden_apple"},
                "components":{
                    "minecraft:use_duration":32,
                    "minecraft:food":{
                        "can_always_eat":true, /* Usable at full hunger. */
                        "effects":[{"duration":120 // Seconds.
                        }]
                    }
                }
            }}"#,
        )
        .unwrap();
        let durations = compile_item_use(root.path()).unwrap();
        assert_eq!(durations.len(), 1);
        assert_eq!(durations[0].identifier.as_ref(), "minecraft:golden_apple");
        assert_eq!(durations[0].ticks, 32);
    }
}
