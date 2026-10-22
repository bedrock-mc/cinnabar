//! `minecraft:attachable` ingestion: item identifier -> entity-catalog geometry,
//! texture, material, render controller, and any literal display transform.
//!
//! Owner-specific definitions remain separate until the wearer selects one.
//! Transforms are populated only from pure-numeric bone
//! keyframes in the exact `wield_first_person`/`wield_third_person` base clips;
//! anything Molang/query-derived is left `NeedsMeasurement`.

use pack_compile_input::source::read_bounded_source;

mod literals;
mod use_profiles;
pub use use_profiles::compile_item_use_profiles;

use std::{collections::BTreeMap, path::Path};

use assets::{
    ArmorSlot, AssetError, AttachablePose, AttachablePoseBone, EntityAssetKind, EntityAssetSource,
    EntityAssetSymbol, EntityDependencyResolution, EquipmentBinding, EquipmentCategory,
    EquipmentReference, EquipmentTexture, EquipmentTransform, ItemDisplayScalar,
    ItemDisplayTransform, ItemUseDuration, MAX_EQUIPMENT_BINDINGS, MAX_EQUIPMENT_IDENTIFIER_BYTES,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[cfg(test)]
use super::SourcePayloads;
use super::{SourceDocuments, invalid};

#[cfg(test)]
thread_local! {
    static ANIMATION_PARSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
mod item_attack;
mod item_food;
pub use item_food::compile_item_foods;
mod dependencies;
mod resources;
mod textures;
pub use dependencies::EquipmentBindingDependencies;
pub use item_attack::compile_item_attack_timings;
pub(super) use resources::controller_resources;
use resources::{ControllerResources, named_resource};
use textures::compile_texture_identifiers_with;
pub use textures::decode_raster as decode_equipment_raster;
pub use textures::{compile_textures_for_assets, compile_textures_for_assets_with};

/// Literal transforms an item's attachable exposed, keyed by item identifier.
pub(super) struct ItemTransforms {
    pub(super) first_person: Option<ItemDisplayTransform>,
    pub(super) third_person: Option<ItemDisplayTransform>,
    pub(super) dropped: Option<ItemDisplayTransform>,
}

struct ParsedAttachable {
    resources: assets::equipment::EquipmentResources,
    item_identifier: Box<str>,
    attachable_identifier: Box<str>,
    geometry: Box<str>,
    texture: Box<str>,
    material: Box<str>,
    render_controller: Box<str>,
    first_person: EquipmentTransform,
    third_person: EquipmentTransform,
    poses: Vec<AttachablePose>,
    animations: BTreeMap<String, Box<str>>,
}

/// Validates one attachable source's shape during collection.
pub(super) fn validate_source(value: &Value) -> Result<(), AttachableError> {
    parse_description(value).map(|_| ())
}

/// Builds the sorted per-item binding table from every collected attachable.
pub(super) fn compile_bindings(
    payloads: &SourceDocuments<'_>,
    symbols: &[EntityAssetSymbol],
    sources: &[EntityAssetSource],
    controller_resources: &BTreeMap<Box<str>, ControllerResources>,
    lenient: bool,
) -> Result<Box<[EquipmentBinding]>, AttachableError> {
    let materials = super::materials::MaterialStates::load(Path::new(""), payloads, sources)
        .map_err(|_| AttachableError::Malformed)?;
    let geometry_symbols = symbol_identifiers(symbols, EntityAssetKind::Geometry);
    let texture_symbols = symbol_identifiers(symbols, EntityAssetKind::Texture);
    let mut chosen = BTreeMap::<(Box<str>, Box<str>), ParsedAttachable>::new();
    for path in payloads.paths() {
        if !path.starts_with("attachables/") {
            continue;
        }
        let parsed = match payloads
            .read(Path::new(""), path)
            .map_err(|_| AttachableError::Malformed)
            .and_then(|value| parse_attachable(value, controller_resources.get(path)))
        {
            Ok(parsed) => parsed,
            // A server pack's bad attachable drops only itself.
            Err(_) if lenient => continue,
            Err(error) => return Err(error),
        };
        chosen.insert(
            (
                parsed.item_identifier.clone(),
                parsed.attachable_identifier.clone(),
            ),
            parsed,
        );
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
    let mut chosen = chosen.into_values().collect::<Vec<_>>();
    literals::compile(payloads, symbols, sources, &mut chosen);
    let bindings = chosen
        .into_iter()
        .map(|parsed| binding(parsed, &geometry_symbols, &texture_symbols, &materials))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(bindings.into_boxed_slice())
}

/// Ordinary definitions supply item defaults; owner mappings stay in the runtime catalog.
pub(super) fn transform_lookup(
    bindings: &[EquipmentBinding],
) -> BTreeMap<Box<str>, ItemTransforms> {
    bindings
        .iter()
        .filter(|binding| binding.attachable.is_none())
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
            .and_then(Value::as_f64);
        let ticks = match seconds {
            Some(seconds) => {
                simulation_clock::whole_ticks_from_seconds(seconds as f32).map(f64::from)
            }
            None => components
                .get("minecraft:use_duration")
                .and_then(Value::as_f64),
        };
        let Some(ticks) = ticks
            .filter(|ticks| ticks.is_finite() && *ticks >= 1.0 && *ticks <= f64::from(u32::MAX))
        else {
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
    materials: &super::materials::MaterialStates,
) -> Result<EquipmentBinding, AttachableError> {
    let category = category(&parsed.item_identifier, &parsed.geometry);
    Ok(EquipmentBinding {
        geometry: reference(parsed.geometry, geometry_symbols),
        texture: reference(parsed.texture, texture_symbols),
        resources: parsed.resources,
        color_mask: materials.color_mask(&parsed.material),
        material: parsed.material,
        render_controller: parsed.render_controller,
        first_person: parsed.first_person,
        third_person: parsed.third_person,
        poses: parsed.poses.into_boxed_slice(),
        dropped: EquipmentTransform::NeedsMeasurement,
        category,
        attachable: (parsed.attachable_identifier != parsed.item_identifier)
            .then_some(parsed.attachable_identifier),
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
    let facts = assets::facts::ItemFacts::builtin().ok();
    let fact = facts.and_then(|facts| facts.by_identifier(item_identifier));
    if let Some(tag) = facts.and_then(|facts| facts.tag(assets::facts::HARNESS_TAG))
        && fact.is_some_and(|fact| fact.tags.contains(tag))
    {
        return EquipmentCategory::Body;
    }
    if fact.is_some_and(|fact| fact.traits.contains(assets::facts::ItemTraits::SHIELD))
        || geometry == "geometry.shield"
    {
        return EquipmentCategory::Shield;
    }
    if fact.is_some_and(|fact| fact.traits.contains(assets::facts::ItemTraits::GLIDER))
        || geometry == assets::ELYTRA_GEOMETRY_IDENTIFIER
    {
        return EquipmentCategory::Elytra;
    }
    use assets::item::armour::ItemArmourSlot as S;
    match fact.and_then(|fact| fact.armour_slot) {
        Some(slot) => EquipmentCategory::Armor {
            slot: match slot {
                S::Head => ArmorSlot::Helmet,
                S::Chest => ArmorSlot::Chestplate,
                S::Legs => ArmorSlot::Leggings,
                S::Feet => ArmorSlot::Boots,
            },
        },
        None => EquipmentCategory::Held,
    }
}

fn parse_attachable(
    value: &Value,
    controller_resources: Option<&ControllerResources>,
) -> Result<ParsedAttachable, AttachableError> {
    let description = parse_description(value)?;
    let item_identifier = item_binding(description)?;
    let attachable_identifier = bounded_identifier(
        description
            .get("identifier")
            .and_then(Value::as_str)
            .ok_or(AttachableError::Malformed)?,
    )?;
    let animations = description
        .get("animations")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(alias, value)| Some((alias.clone(), Box::from(value.as_str()?))))
        .collect();
    let geometry = named_resource(
        description,
        "geometry",
        controller_resources.and_then(|resources| resources.geometry.as_deref()),
    )?;
    Ok(ParsedAttachable {
        poses: Vec::new(),
        animations,
        resources: resources::aliases(description)?,
        geometry,
        texture: named_resource(
            description,
            "textures",
            controller_resources.and_then(|resources| resources.texture.as_deref()),
        )?,
        material: named_resource(description, "materials", None)?,
        render_controller: first_render_controller(description)?,
        first_person: EquipmentTransform::NeedsMeasurement,
        third_person: EquipmentTransform::NeedsMeasurement,
        item_identifier,
        attachable_identifier,
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

/// Reads the item identity independently of the authored definition name.
fn item_binding(description: &Value) -> Result<Box<str>, AttachableError> {
    if let Some(item) = description.get("item").and_then(Value::as_object) {
        let mut keys = item.keys();
        let (Some(identifier), None) = (keys.next(), keys.next()) else {
            return Err(AttachableError::Malformed);
        };
        return bounded_identifier(identifier);
    }
    let identifier = description
        .get("identifier")
        .and_then(Value::as_str)
        .ok_or(AttachableError::Malformed)?;
    bounded_identifier(identifier)
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
mod tests;
