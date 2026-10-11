use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use assets::{
    AssetError, CompiledEntityAssets, EntityAssetKind, EntityAssetSource, EntityAssetSymbol,
    EntityDependency, EntityDependencyKind, EntityDependencyResolution, EntityGeometry,
    EntityGeometryBone, EntityGeometryInheritance, EquipmentBinding, MAX_ENTITY_ASSET_SOURCES,
    MAX_ENTITY_ASSET_SYMBOLS, MAX_ENTITY_DEPENDENCIES, MAX_ENTITY_GEOMETRIES,
    MAX_ENTITY_TOTAL_SOURCE_BYTES, validate_entity_geometry_inheritance,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

mod animation;
mod attachable;
mod collect;
mod geometry;
mod geometry_layers;
pub use geometry_layers::select_entity_geometry;
mod item;
mod item_bindings;
mod json;
mod legacy_block_geometry;
mod legacy_icons;
mod molang;
mod native_bind_pose;
mod native_dragon_geometry;
mod native_held_items;
mod pack;
mod sanitize;
mod source;
mod vanilla_refs;
mod versions;
pub use molang::{BlockMolang, BlockStateValue, compile_molang_expression};
pub use vanilla_refs::compile_vanilla_entity_refs;

pub use pack::{
    EntityPackCompilation, EntityPackSkips, MAX_PACK_ENTITY_BYTES, MAX_PACK_ENTITY_SOURCES,
    compile_entity_pack,
};

use collect::{collect_family, collect_optional_family, collect_optional_file};
use geometry::parse_geometry;
pub(crate) use json::parse_fully_unique_json;
use json::{parse_semantic_json, parse_unique_json};
pub(crate) use source::{open_source_handle, read_bounded_source};

#[allow(unused_imports)] // Integration publishes this private leaf after review.
pub use animation::{CompileReferenceOutcome, FallbackReason, RejectReason};
pub use attachable::{
    compile_item_attack_timings, compile_item_use as compile_item_use_durations,
    compile_textures as compile_equipment_textures,
    compile_textures_for_assets as compile_equipment_textures_for_assets,
    compile_textures_for_assets_with as compile_equipment_textures_for_assets_with,
    compile_textures_with as compile_equipment_textures_with,
};

/// Deterministic carrier plus the attributed resolution decision for every rig.
///
/// `equipment_bindings` is compiled from `attachables/` for the separate
/// equipment carrier; it is not part of the entity carrier byte format.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntityAssetCompilation {
    pub assets: CompiledEntityAssets,
    pub reference_outcomes: Box<[CompileReferenceOutcome<u32>]>,
    pub equipment_bindings: Box<[EquipmentBinding]>,
}

const MAX_SOURCE_MANIFEST_BYTES: usize = 1024 * 1024;
const MAX_ENTITY_SOURCE_DIRECTORY_DEPTH: usize = 32;

#[derive(Clone)]
struct PendingSymbol {
    kind: EntityAssetKind,
    identifier: Box<str>,
    source_path: Box<str>,
    dependencies: Box<[EntityDependency]>,
}

#[derive(Clone)]
struct PendingGeometry {
    identifier: Box<str>,
    inherits: Option<Box<str>>,
    source_path: Box<str>,
    texture_width: Option<u16>,
    texture_height: Option<u16>,
    visible_bounds: Option<assets::EntityGeometryBounds>,
    bones: Box<[EntityGeometryBone]>,
}

type SourcePayloads = BTreeMap<Box<str>, Box<[u8]>>;

/// Compiles deterministic entity catalog and geometry payloads from the exact
/// pinned local Bedrock resource pack. Source payloads remain local-only.
pub fn compile_entity_assets(
    root: &Path,
    source_manifest: &[u8],
) -> Result<CompiledEntityAssets, AssetError> {
    Ok(compile_entity_assets_with_report(root, source_manifest)?.assets)
}

/// Compiles the carrier while retaining every required-rejection and optional
/// fallback decision for the integration-owned provenance report.
pub fn compile_entity_assets_with_report(
    root: &Path,
    source_manifest: &[u8],
) -> Result<EntityAssetCompilation, AssetError> {
    let source_manifest_sha256 = validate_source_manifest(source_manifest)?;
    let mut selected = Vec::new();
    collect_family(root, "entity", &["json"], &mut selected)?;
    collect_family(root, "models/entity", &["json"], &mut selected)?;
    collect_optional_file(root, assets::LEGACY_ENTITY_GEOMETRY_PATH, &mut selected)?;
    collect_family(root, "animations", &["json"], &mut selected)?;
    collect_family(root, "animation_controllers", &["json"], &mut selected)?;
    collect_family(root, "render_controllers", &["json"], &mut selected)?;
    collect_optional_family(root, "materials", &["material"], &mut selected)?;
    collect_family(
        root,
        "textures/entity",
        &["json", "png", "tga"],
        &mut selected,
    )?;
    collect_optional_family(
        root,
        "textures/items",
        &["json", "png", "tga"],
        &mut selected,
    )?;
    collect_optional_family(root, "attachables", &["json"], &mut selected)?;
    collect_optional_family(
        root,
        "textures/models/armor",
        &["json", "png", "tga"],
        &mut selected,
    )?;
    for extension in ["png", "tga"] {
        let path = format!("{}.{extension}", assets::ACTOR_GLINT_TEXTURE_IDENTIFIER);
        collect_optional_file(root, &path, &mut selected)?;
    }
    collect_optional_file(root, "textures/item_texture.json", &mut selected)?;
    collect_optional_file(root, "manifest.json", &mut selected)?;
    selected.sort_by(|left, right| left.0.cmp(&right.0));
    if selected.is_empty() || selected.len() > MAX_ENTITY_ASSET_SOURCES {
        return Err(invalid("entity asset source count exceeds bound"));
    }
    for pair in selected.windows(2) {
        if pair[0].0 == pair[1].0 {
            return Err(invalid("duplicate entity asset source path"));
        }
    }

    let mut total_source_bytes = 0usize;
    let mut sources = Vec::with_capacity(selected.len());
    let mut source_payloads = SourcePayloads::new();
    let mut symbols = BTreeMap::<(EntityAssetKind, Box<str>, Box<str>), PendingSymbol>::new();
    let mut geometries = BTreeMap::<(Box<str>, Box<str>), PendingGeometry>::new();
    for (relative_path, absolute_path) in selected {
        let bytes = read_bounded_source(root, &absolute_path)?;
        total_source_bytes = total_source_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| invalid("entity source-byte total overflow"))?;
        if total_source_bytes > MAX_ENTITY_TOTAL_SOURCE_BYTES {
            return Err(invalid("entity source-byte total exceeds bound"));
        }
        let source_index = sources.len();
        sources.push(EntityAssetSource {
            path: relative_path.clone(),
            source_bytes: u32::try_from(bytes.len())
                .map_err(|_| invalid("entity source byte count overflow"))?,
            source_sha256: Sha256::digest(&bytes).into(),
        });
        parse_source(
            &relative_path,
            &absolute_path,
            &bytes,
            &mut symbols,
            &mut geometries,
        )?;
        // Pinned vanilla samples occasionally omit a legacy cube bind transform that
        // the shipped native base pack retains. This is never applied to session packs.
        native_bind_pose::restore_sample_defaults(&relative_path, &bytes, &mut geometries);
        native_dragon_geometry::expand_sample(&relative_path, &bytes, &mut geometries);
        source_payloads.insert(relative_path, bytes.into_boxed_slice());
        debug_assert_eq!(source_index + 1, sources.len());
    }
    let native_binding_bytes = native_held_items::complete_sample_bindings(
        root,
        &mut sources,
        &mut source_payloads,
        &mut symbols,
        &mut geometries,
    )?;
    total_source_bytes = total_source_bytes
        .checked_add(native_binding_bytes)
        .ok_or_else(|| invalid("entity source-byte total overflow"))?;
    let route_bytes = item::BLOCK_ITEM_ROUTES;
    total_source_bytes = total_source_bytes
        .checked_add(route_bytes.len())
        .ok_or_else(|| invalid("entity source-byte total overflow"))?;
    if total_source_bytes > MAX_ENTITY_TOTAL_SOURCE_BYTES
        || sources.len() >= MAX_ENTITY_ASSET_SOURCES
    {
        return Err(invalid("entity source-byte total or count exceeds bound"));
    }
    let route_path: Box<str> = "registry/block-item-routes-v2193.json".into();
    sources.push(EntityAssetSource {
        path: route_path.clone(),
        source_bytes: route_bytes.len() as u32,
        source_sha256: Sha256::digest(route_bytes).into(),
    });
    source_payloads.insert(route_path, route_bytes.into());
    let binding_bytes = item_bindings::SOURCE_BYTES;
    total_source_bytes = total_source_bytes
        .checked_add(binding_bytes.len())
        .ok_or_else(|| invalid("entity source-byte total overflow"))?;
    if total_source_bytes > MAX_ENTITY_TOTAL_SOURCE_BYTES
        || sources.len() >= MAX_ENTITY_ASSET_SOURCES
    {
        return Err(invalid("entity source-byte total or count exceeds bound"));
    }
    sources.push(EntityAssetSource {
        path: item_bindings::SOURCE_PATH.into(),
        source_bytes: binding_bytes.len() as u32,
        source_sha256: Sha256::digest(binding_bytes).into(),
    });
    let legacy_bytes = legacy_icons::SOURCE_BYTES;
    total_source_bytes = total_source_bytes
        .checked_add(legacy_bytes.len())
        .ok_or_else(|| invalid("entity source-byte total overflow"))?;
    if total_source_bytes > MAX_ENTITY_TOTAL_SOURCE_BYTES
        || sources.len() >= MAX_ENTITY_ASSET_SOURCES
    {
        return Err(invalid("entity source-byte total or count exceeds bound"));
    }
    sources.push(EntityAssetSource {
        path: legacy_icons::SOURCE_PATH.into(),
        source_bytes: legacy_bytes.len() as u32,
        source_sha256: Sha256::digest(legacy_bytes).into(),
    });
    versions::select_vanilla_definitions(&mut symbols, &source_payloads)?;
    assemble(
        root,
        sources,
        &source_payloads,
        symbols,
        geometries,
        source_manifest_sha256,
        true,
    )
}

/// Resolves symbols and geometry inheritance, compiles animation and Molang
/// payloads, and (for the vanilla carrier) item visuals and equipment bindings.
fn assemble(
    root: &Path,
    mut sources: Vec<EntityAssetSource>,
    source_payloads: &SourcePayloads,
    symbols: BTreeMap<(EntityAssetKind, Box<str>, Box<str>), PendingSymbol>,
    geometries: BTreeMap<(Box<str>, Box<str>), PendingGeometry>,
    source_manifest_sha256: [u8; 32],
    include_items: bool,
) -> Result<EntityAssetCompilation, AssetError> {
    sources.sort_by(|left, right| left.path.cmp(&right.path));
    if symbols.is_empty() || symbols.len() > MAX_ENTITY_ASSET_SYMBOLS {
        return Err(invalid("entity asset symbol count exceeds bound"));
    }
    let source_indices = sources
        .iter()
        .enumerate()
        .map(|(index, source)| (source.path.as_ref(), index as u32))
        .collect::<BTreeMap<_, _>>();
    let mut symbols = symbols
        .into_values()
        .map(|symbol| {
            let source_index = source_indices
                .get(symbol.source_path.as_ref())
                .copied()
                .ok_or_else(|| invalid("entity symbol references an absent source"))?;
            Ok(EntityAssetSymbol {
                kind: symbol.kind,
                identifier: symbol.identifier,
                source_index,
                dependencies: symbol.dependencies,
            })
        })
        .collect::<Result<Vec<_>, AssetError>>()?;
    let available_symbols = symbols
        .iter()
        .map(|symbol| (symbol.kind, symbol.identifier.clone()))
        .collect::<BTreeSet<_>>();
    for symbol in &mut symbols {
        for dependency in &mut symbol.dependencies {
            dependency.resolution = if available_symbols.contains(&(
                dependency_asset_kind(dependency.kind),
                dependency.identifier.clone(),
            )) {
                EntityDependencyResolution::Catalog
            } else {
                EntityDependencyResolution::External
            };
        }
    }
    if geometries.len() > MAX_ENTITY_GEOMETRIES {
        return Err(invalid("entity geometry count exceeds bound"));
    }
    let pending_geometries = geometries.into_values().collect::<Vec<_>>();
    let local_dimensions = pending_geometries
        .iter()
        .map(|geometry| (geometry.texture_width, geometry.texture_height))
        .collect::<Vec<_>>();
    let mut geometries = pending_geometries
        .into_iter()
        .map(|geometry| {
            let source_index = source_indices
                .get(geometry.source_path.as_ref())
                .copied()
                .ok_or_else(|| invalid("entity geometry references an absent source"))?;
            Ok(EntityGeometry {
                identifier: geometry.identifier,
                inherits: geometry
                    .inherits
                    .map(|identifier| EntityGeometryInheritance {
                        resolution: if available_symbols
                            .contains(&(EntityAssetKind::Geometry, identifier.clone()))
                        {
                            EntityDependencyResolution::Catalog
                        } else {
                            EntityDependencyResolution::External
                        },
                        identifier,
                    }),
                source_index,
                texture_width: geometry.texture_width.unwrap_or(64),
                texture_height: geometry.texture_height.unwrap_or(64),
                bones: geometry.bones,
                visible_bounds: geometry.visible_bounds,
            })
        })
        .collect::<Result<Vec<_>, AssetError>>()?;
    let selected_parents = validate_entity_geometry_inheritance(&geometries)?;
    for (index, geometry) in geometries.iter_mut().enumerate() {
        geometry.texture_width = resolve_geometry_dimension(
            index,
            &local_dimensions,
            &selected_parents,
            |dimensions| dimensions.0,
        )?;
        geometry.texture_height = resolve_geometry_dimension(
            index,
            &local_dimensions,
            &selected_parents,
            |dimensions| dimensions.1,
        )?;
    }
    let mut molang_compiler = molang::MolangCompiler::default();
    let animation = animation::compile(
        root,
        source_payloads,
        &sources,
        &symbols,
        &geometries,
        &mut molang_compiler,
        !include_items,
    )?;
    validate_reference_coverage(&symbols, &animation)?;
    let mut molang = molang_compiler.finish()?;
    let equipment_bindings =
        attachable::compile_bindings(source_payloads, &symbols, &sources, !include_items)?;
    let items = if include_items {
        let item_transforms = attachable::transform_lookup(&equipment_bindings);
        item::compile(root, source_payloads, &sources, &item_transforms)?
    } else {
        item::ItemPayload {
            block_visual_count: 0,
            visuals: Box::default(),
            aliases: Box::default(),
        }
    };
    let reference_outcomes = animation.outcomes;
    let assets = CompiledEntityAssets {
        source_manifest_sha256,
        block_visual_count: items.block_visual_count,
        sources: sources.into_boxed_slice(),
        symbols: symbols.into_boxed_slice(),
        geometries: geometries.into_boxed_slice(),
        animation_clips: animation.clips,
        animation_channels: animation.channels,
        animation_keyframes: animation.keyframes,
        molang_expressions: std::mem::take(&mut molang.expressions),
        molang_ops: std::mem::take(&mut molang.ops),
        molang_collections: std::mem::take(&mut molang.collections),
        molang_collection_items: std::mem::take(&mut molang.collection_items),
        molang_symbols: molang.into_symbols(),
        controllers: animation.controllers,
        controller_states: animation.controller_states,
        controller_animations: animation.controller_animations,
        controller_transitions: animation.controller_transitions,
        rig_bindings: animation.rig_bindings,
        rig_geometries: animation.rig_geometries,
        rig_animations: animation.rig_animations,
        rig_controllers: animation.rig_controllers,
        item_visuals: items.visuals,
        item_visual_aliases: items.aliases,
        render: animation.render,
    };
    if include_items {
        assets.validate()?;
    }
    Ok(EntityAssetCompilation {
        assets,
        reference_outcomes,
        equipment_bindings,
    })
}

fn validate_reference_coverage(
    symbols: &[EntityAssetSymbol],
    animation: &animation::AnimationPayload,
) -> Result<(), AssetError> {
    let attributed = animation
        .outcomes
        .iter()
        .filter_map(|outcome| match outcome {
            CompileReferenceOutcome::Resolved(_) => None,
            CompileReferenceOutcome::OptionalStaticFallback { symbol, .. }
            | CompileReferenceOutcome::RequiredRigRejected { symbol, .. } => Some(*symbol),
        })
        .collect::<BTreeSet<_>>();
    let compiled_clips = animation
        .clips
        .iter()
        .map(|clip| clip.symbol)
        .collect::<BTreeSet<_>>();
    let compiled_controllers = animation
        .controllers
        .iter()
        .map(|controller| controller.symbol)
        .collect::<BTreeSet<_>>();
    let compiled_entities = animation
        .rig_bindings
        .iter()
        .map(|rig| rig.entity_symbol)
        .collect::<BTreeSet<_>>();
    for (index, symbol) in symbols.iter().enumerate() {
        let index = index as u32;
        let covered = match symbol.kind {
            EntityAssetKind::Animation => {
                compiled_clips.contains(&index) || attributed.contains(&index)
            }
            EntityAssetKind::AnimationController => {
                compiled_controllers.contains(&index) || attributed.contains(&index)
            }
            EntityAssetKind::Entity | EntityAssetKind::Attachable => {
                compiled_entities.contains(&index) || attributed.contains(&index)
            }
            _ => true,
        };
        if !covered {
            return Err(invalid(format!(
                "unexplained entity asset loss for {:?} `{}`",
                symbol.kind, symbol.identifier
            )));
        }
    }
    Ok(())
}

fn resolve_geometry_dimension(
    start: usize,
    local_dimensions: &[(Option<u16>, Option<u16>)],
    selected_parents: &[Option<usize>],
    select: impl Fn((Option<u16>, Option<u16>)) -> Option<u16>,
) -> Result<u16, AssetError> {
    let mut current = start;
    for _ in 0..=selected_parents.len() {
        if let Some(dimension) = select(local_dimensions[current]) {
            return Ok(dimension);
        }
        let Some(parent) = selected_parents[current] else {
            return Ok(64);
        };
        current = parent;
    }
    Err(invalid("entity geometry dimension inheritance is cyclic"))
}

fn parse_source(
    relative_path: &str,
    absolute_path: &Path,
    bytes: &[u8],
    symbols: &mut BTreeMap<(EntityAssetKind, Box<str>, Box<str>), PendingSymbol>,
    geometry_payloads: &mut BTreeMap<(Box<str>, Box<str>), PendingGeometry>,
) -> Result<(), AssetError> {
    if matches!(
        relative_path,
        "textures/item_texture.json" | "manifest.json"
    ) {
        parse_unique_json(absolute_path, bytes)?;
        return Ok(());
    }
    if relative_path.starts_with("textures/items/") {
        if relative_path.ends_with(".png") || relative_path.ends_with(".tga") {
            return Ok(());
        }
        let value = parse_semantic_json(absolute_path, bytes)?;
        validate_root_fields(
            &value,
            absolute_path,
            &["format_version", "minecraft:texture_set"],
            &["format_version", "minecraft:texture_set"],
        )?;
        return Ok(());
    }
    if relative_path.starts_with("attachables/") {
        let value = parse_unique_json(absolute_path, bytes)?;
        attachable::validate_source(&value)?;
        return parse_entity(
            relative_path,
            absolute_path,
            &value,
            symbols,
            EntityAssetKind::Attachable,
        );
    }
    if relative_path.starts_with("materials/") && relative_path.ends_with(".material") {
        let value = parse_fully_unique_json(absolute_path, bytes)?;
        return value
            .get("materials")
            .and_then(Value::as_object)
            .map(|_| ())
            .ok_or_else(|| invalid("entity material definitions must be an object"));
    }
    if relative_path.starts_with("textures/") {
        if relative_path.ends_with(".png") || relative_path.ends_with(".tga") {
            let identifier = relative_path
                .strip_suffix(".png")
                .or_else(|| relative_path.strip_suffix(".tga"))
                .ok_or_else(|| invalid("texture source lacks a canonical raster extension"))?;
            return insert_symbol(
                symbols,
                EntityAssetKind::Texture,
                identifier,
                relative_path,
                Box::new([]),
            );
        }
        let value = parse_unique_json(absolute_path, bytes)?;
        validate_root_fields(
            &value,
            absolute_path,
            &["format_version", "minecraft:texture_set"],
            &["format_version", "minecraft:texture_set"],
        )?;
        return Ok(());
    }

    let value = if relative_path.starts_with("models/entity/")
        || relative_path == assets::LEGACY_ENTITY_GEOMETRY_PATH
    {
        parse_fully_unique_json(absolute_path, bytes)?
    } else {
        parse_unique_json(absolute_path, bytes)?
    };
    if relative_path.starts_with("entity/") {
        validate_root_fields(
            &value,
            absolute_path,
            &["format_version", "minecraft:client_entity"],
            &["format_version", "minecraft:client_entity"],
        )?;
        parse_entity(
            relative_path,
            absolute_path,
            &value,
            symbols,
            EntityAssetKind::Entity,
        )
    } else if relative_path == assets::LEGACY_ENTITY_GEOMETRY_PATH {
        legacy_block_geometry::parse(
            relative_path,
            absolute_path,
            &value,
            symbols,
            geometry_payloads,
        )
    } else if relative_path.starts_with("models/entity/") {
        parse_geometry(
            relative_path,
            absolute_path,
            &value,
            symbols,
            geometry_payloads,
        )
    } else if relative_path.starts_with("animations/") {
        parse_named_map(
            relative_path,
            absolute_path,
            &value,
            "animations",
            EntityAssetKind::Animation,
            symbols,
        )
    } else if relative_path.starts_with("animation_controllers/") {
        parse_named_map(
            relative_path,
            absolute_path,
            &value,
            "animation_controllers",
            EntityAssetKind::AnimationController,
            symbols,
        )
    } else if relative_path.starts_with("render_controllers/") {
        parse_named_map(
            relative_path,
            absolute_path,
            &value,
            "render_controllers",
            EntityAssetKind::RenderController,
            symbols,
        )
    } else {
        Err(invalid(
            "entity asset source is outside the recognized families",
        ))
    }
}

fn parse_entity(
    relative_path: &str,
    path: &Path,
    value: &Value,
    symbols: &mut BTreeMap<(EntityAssetKind, Box<str>, Box<str>), PendingSymbol>,
    kind: EntityAssetKind,
) -> Result<(), AssetError> {
    let description = animation::roots::description(value).ok_or_else(|| {
        invalid(format!(
            "missing client entity description in {}",
            path.display()
        ))
    })?;
    let identifier = description
        .get("identifier")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            invalid(format!(
                "missing client entity identifier in {}",
                path.display()
            ))
        })?;
    let mut dependencies = BTreeSet::new();
    collect_named_dependencies(
        description.get("geometry"),
        EntityDependencyKind::Geometry,
        &mut dependencies,
    )?;
    collect_named_dependencies(
        description.get("textures"),
        EntityDependencyKind::Texture,
        &mut dependencies,
    )?;
    if let Some(animations) = description.get("animations") {
        let mut values = Vec::new();
        collect_string_leaves(animations, &mut values, 0)?;
        for target in values {
            let kind = if target.starts_with("controller.animation.") {
                EntityDependencyKind::AnimationController
            } else {
                EntityDependencyKind::Animation
            };
            dependencies.insert(EntityDependency {
                kind,
                identifier: target.into(),
                resolution: EntityDependencyResolution::External,
            });
        }
    }
    collect_named_dependencies(
        description.get("animation_controllers"),
        EntityDependencyKind::AnimationController,
        &mut dependencies,
    )?;
    collect_render_controller_dependencies(
        description.get("render_controllers"),
        &mut dependencies,
    )?;
    if dependencies.len() > MAX_ENTITY_DEPENDENCIES {
        return Err(invalid("client entity dependency count exceeds bound"));
    }
    insert_symbol(
        symbols,
        kind,
        identifier,
        relative_path,
        dependencies
            .into_iter()
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    )
}

fn collect_named_dependencies(
    value: Option<&Value>,
    kind: EntityDependencyKind,
    dependencies: &mut BTreeSet<EntityDependency>,
) -> Result<(), AssetError> {
    let Some(value) = value else {
        return Ok(());
    };
    let mut values = Vec::new();
    collect_string_leaves(value, &mut values, 0)?;
    for identifier in values {
        if kind == EntityDependencyKind::Texture && identifier.is_empty() {
            continue;
        }
        dependencies.insert(EntityDependency {
            kind,
            identifier: identifier.into(),
            resolution: EntityDependencyResolution::External,
        });
        if dependencies.len() > MAX_ENTITY_DEPENDENCIES {
            return Err(invalid("client entity dependency count exceeds bound"));
        }
    }
    Ok(())
}

fn collect_render_controller_dependencies(
    value: Option<&Value>,
    dependencies: &mut BTreeSet<EntityDependency>,
) -> Result<(), AssetError> {
    let Some(value) = value else {
        return Ok(());
    };
    let entries = value
        .as_array()
        .ok_or_else(|| invalid("client entity render_controllers must be an array"))?;
    for entry in entries {
        match entry {
            Value::String(identifier) => {
                dependencies.insert(EntityDependency {
                    kind: EntityDependencyKind::RenderController,
                    identifier: identifier.as_str().into(),
                    resolution: EntityDependencyResolution::External,
                });
            }
            Value::Object(conditional) => {
                for identifier in conditional.keys() {
                    dependencies.insert(EntityDependency {
                        kind: EntityDependencyKind::RenderController,
                        identifier: identifier.as_str().into(),
                        resolution: EntityDependencyResolution::External,
                    });
                }
            }
            _ => {
                return Err(invalid(
                    "client entity render controller entry must be a string or conditional object",
                ));
            }
        }
        if dependencies.len() > MAX_ENTITY_DEPENDENCIES {
            return Err(invalid("client entity dependency count exceeds bound"));
        }
    }
    Ok(())
}

const fn dependency_asset_kind(kind: EntityDependencyKind) -> EntityAssetKind {
    match kind {
        EntityDependencyKind::Geometry => EntityAssetKind::Geometry,
        EntityDependencyKind::Animation => EntityAssetKind::Animation,
        EntityDependencyKind::AnimationController => EntityAssetKind::AnimationController,
        EntityDependencyKind::RenderController => EntityAssetKind::RenderController,
        EntityDependencyKind::Texture => EntityAssetKind::Texture,
    }
}

fn collect_string_leaves<'a>(
    value: &'a Value,
    output: &mut Vec<&'a str>,
    depth: usize,
) -> Result<(), AssetError> {
    if depth > 16 || output.len() > MAX_ENTITY_DEPENDENCIES {
        return Err(invalid("entity dependency structure exceeds bound"));
    }
    match value {
        Value::String(value) => output.push(value),
        Value::Array(values) => {
            for value in values {
                collect_string_leaves(value, output, depth + 1)?;
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                collect_string_leaves(value, output, depth + 1)?;
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    if output.len() > MAX_ENTITY_DEPENDENCIES {
        return Err(invalid("entity dependency count exceeds bound"));
    }
    Ok(())
}
fn parse_named_map(
    relative_path: &str,
    path: &Path,
    value: &Value,
    field: &'static str,
    kind: EntityAssetKind,
    symbols: &mut BTreeMap<(EntityAssetKind, Box<str>, Box<str>), PendingSymbol>,
) -> Result<(), AssetError> {
    validate_root_fields(
        value,
        path,
        &["format_version", field],
        &["format_version", field],
    )?;
    let entries = value
        .get(field)
        .and_then(Value::as_object)
        .ok_or_else(|| invalid(format!("invalid {field} map in {}", path.display())))?;
    for identifier in entries.keys() {
        insert_symbol(symbols, kind, identifier, relative_path, Box::new([]))?;
    }
    Ok(())
}

fn insert_symbol(
    symbols: &mut BTreeMap<(EntityAssetKind, Box<str>, Box<str>), PendingSymbol>,
    kind: EntityAssetKind,
    identifier: &str,
    source_path: &str,
    dependencies: Box<[EntityDependency]>,
) -> Result<(), AssetError> {
    let identifier: Box<str> = identifier.into();
    let source_path: Box<str> = source_path.into();
    let symbol = PendingSymbol {
        kind,
        identifier: identifier.clone(),
        source_path: source_path.clone(),
        dependencies,
    };
    if symbols
        .insert((kind, identifier.clone(), source_path), symbol)
        .is_some()
    {
        return Err(invalid(format!(
            "duplicate {kind:?} entity asset symbol `{identifier}` within one source"
        )));
    }
    if symbols.len() > MAX_ENTITY_ASSET_SYMBOLS {
        return Err(invalid("entity asset symbol count exceeds bound"));
    }
    Ok(())
}

fn validate_root_fields(
    value: &Value,
    path: &Path,
    allowed: &[&str],
    required: &[&str],
) -> Result<(), AssetError> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid(format!("JSON root must be an object in {}", path.display())))?;
    if object
        .keys()
        .any(|field| !allowed.contains(&field.as_str()))
        || required.iter().any(|field| !object.contains_key(*field))
    {
        return Err(invalid(format!(
            "unknown or missing entity family root field in {}",
            path.display()
        )));
    }
    Ok(())
}
pub(crate) fn validate_vanilla_source_manifest(source: &[u8]) -> Result<[u8; 32], AssetError> {
    validate_source_manifest(source)
}

fn validate_source_manifest(source: &[u8]) -> Result<[u8; 32], AssetError> {
    if source.len() > MAX_SOURCE_MANIFEST_BYTES {
        return Err(invalid("entity source manifest exceeds bound"));
    }
    let canonical = canonical_manifest_line_endings(source)?;
    let value = parse_unique_json(Path::new("assets/vanilla-source.json"), &canonical)?;
    let object = value
        .as_object()
        .ok_or_else(|| invalid("entity source manifest must be an object"))?;
    let expected_fields = [
        "schema",
        "tag",
        "commit",
        "archive",
        "url",
        "sha256",
        "artifact_policy",
        "cache_dir",
    ];
    if object.len() != expected_fields.len()
        || expected_fields
            .iter()
            .any(|field| !object.contains_key(*field))
    {
        return Err(invalid(
            "entity source manifest fields do not match the pin",
        ));
    }
    let digest: [u8; 32] = Sha256::digest(&canonical).into();
    if digest != assets::vanilla_source_manifest_sha256() {
        return Err(invalid(
            "manifest bytes and fields must exactly match the reviewed Mojang Bedrock Samples pin",
        ));
    }
    Ok(digest)
}

fn canonical_manifest_line_endings(source: &[u8]) -> Result<Cow<'_, [u8]>, AssetError> {
    if !source.contains(&b'\r') {
        return Ok(Cow::Borrowed(source));
    }
    let mut canonical = Vec::with_capacity(source.len());
    let mut index = 0;
    while index < source.len() {
        match source[index] {
            b'\r' if source.get(index + 1) == Some(&b'\n') => {
                canonical.push(b'\n');
                index += 2;
            }
            b'\r' | b'\n' => return Err(invalid("manifest line endings are not canonical")),
            byte => {
                canonical.push(byte);
                index += 1;
            }
        }
    }
    Ok(Cow::Owned(canonical))
}

fn invalid(detail: impl Into<Box<str>>) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}
