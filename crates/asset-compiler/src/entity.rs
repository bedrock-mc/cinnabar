use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

use assets::{
    AssetError, CompiledEntityAssets, EntityAssetKind, EntityAssetSource, EntityAssetSymbol,
    EntityDependency, EntityDependencyKind, EntityDependencyResolution, EntityGeometry,
    EntityGeometryBone, EntityGeometryInheritance, MAX_ENTITY_ASSET_SOURCES,
    MAX_ENTITY_ASSET_SYMBOLS, MAX_ENTITY_DEPENDENCIES, MAX_ENTITY_GEOMETRIES,
    MAX_ENTITY_SOURCE_BYTES, MAX_ENTITY_TOTAL_SOURCE_BYTES, validate_entity_geometry_inheritance,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

mod geometry;
mod json;

use geometry::parse_geometry;
use json::{parse_fully_unique_json, parse_unique_json};

const MAX_SOURCE_MANIFEST_BYTES: usize = 1024 * 1024;
const MAX_ENTITY_SOURCE_DIRECTORY_DEPTH: usize = 32;
const PINNED_MANIFEST_SHA256: [u8; 32] =
    decode_sha256(b"c6d5f56b942d703a7acd1f83b2cddb7633069e13412ad5a1c3beae666e2ec6f6");

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
    bones: Box<[EntityGeometryBone]>,
}

/// Compiles deterministic entity catalog and geometry payloads from the exact
/// pinned local Bedrock resource pack. Source payloads remain local-only.
pub fn compile_entity_assets(
    root: &Path,
    source_manifest: &[u8],
) -> Result<CompiledEntityAssets, AssetError> {
    let source_manifest_sha256 = validate_source_manifest(source_manifest)?;
    let mut selected = Vec::new();
    collect_family(root, "entity", &["json"], &mut selected)?;
    collect_family(root, "models/entity", &["json"], &mut selected)?;
    collect_family(root, "animations", &["json"], &mut selected)?;
    collect_family(root, "animation_controllers", &["json"], &mut selected)?;
    collect_family(root, "render_controllers", &["json"], &mut selected)?;
    collect_family(
        root,
        "textures/entity",
        &["json", "png", "tga"],
        &mut selected,
    )?;
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
    let mut symbols = BTreeMap::<(EntityAssetKind, Box<str>, Box<str>), PendingSymbol>::new();
    let mut geometries = BTreeMap::<(Box<str>, Box<str>), PendingGeometry>::new();
    for (relative_path, absolute_path) in selected {
        let bytes = read_bounded_source(&absolute_path)?;
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
        debug_assert_eq!(source_index + 1, sources.len());
    }
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
    Ok(CompiledEntityAssets {
        source_manifest_sha256,
        block_visual_count: 0,
        sources: sources.into_boxed_slice(),
        symbols: symbols.into_boxed_slice(),
        geometries: geometries.into_boxed_slice(),
        animation_clips: Box::new([]),
        animation_channels: Box::new([]),
        animation_keyframes: Box::new([]),
        molang_symbols: Box::new([]),
        molang_expressions: Box::new([]),
        molang_ops: Box::new([]),
        molang_collections: Box::new([]),
        molang_collection_items: Box::new([]),
        controllers: Box::new([]),
        controller_states: Box::new([]),
        controller_animations: Box::new([]),
        controller_transitions: Box::new([]),
        rig_bindings: Box::new([]),
        rig_animations: Box::new([]),
        rig_controllers: Box::new([]),
        item_visuals: Box::new([]),
        item_visual_aliases: Box::new([]),
    })
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

fn collect_family(
    root: &Path,
    relative_root: &str,
    allowed_extensions: &[&str],
    output: &mut Vec<(Box<str>, PathBuf)>,
) -> Result<(), AssetError> {
    let absolute_root = root.join(relative_root);
    let metadata = fs::symlink_metadata(&absolute_root).map_err(|source| AssetError::Io {
        path: absolute_root.clone(),
        source,
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(invalid("entity asset family root must be a real directory"));
    }
    collect_directory(root, &absolute_root, allowed_extensions, output, 0)
}

fn collect_directory(
    root: &Path,
    directory: &Path,
    allowed_extensions: &[&str],
    output: &mut Vec<(Box<str>, PathBuf)>,
    depth: usize,
) -> Result<(), AssetError> {
    if depth > MAX_ENTITY_SOURCE_DIRECTORY_DEPTH {
        return Err(invalid("entity asset source directory depth exceeds bound"));
    }
    let mut entries = fs::read_dir(directory)
        .map_err(|source| AssetError::Io {
            path: directory.to_path_buf(),
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| AssetError::Io {
            path: directory.to_path_buf(),
            source,
        })?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|source| AssetError::Io {
            path: path.clone(),
            source,
        })?;
        if metadata.file_type().is_symlink() {
            return Err(invalid(
                "entity asset source trees may not contain symlinks",
            ));
        }
        if metadata.is_dir() {
            collect_directory(root, &path, allowed_extensions, output, depth + 1)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(invalid(
                "entity asset source tree contains a non-file entry",
            ));
        }
        let extension = path.extension().and_then(|extension| extension.to_str());
        if !extension.is_some_and(|extension| allowed_extensions.contains(&extension)) {
            return Err(invalid(format!(
                "unsupported entity asset source extension at {}",
                path.display()
            )));
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| invalid("entity asset source escaped the pack root"))?
            .to_string_lossy()
            .replace('\\', "/");
        output.push((relative.into_boxed_str(), path));
        if output.len() > MAX_ENTITY_ASSET_SOURCES {
            return Err(invalid("entity asset source count exceeds bound"));
        }
    }
    Ok(())
}

fn read_bounded_source(path: &Path) -> Result<Vec<u8>, AssetError> {
    let file = File::open(path).map_err(|source| AssetError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let length = file
        .metadata()
        .map_err(|source| AssetError::Io {
            path: path.to_path_buf(),
            source,
        })?
        .len();
    if length == 0 || length > MAX_ENTITY_SOURCE_BYTES as u64 {
        return Err(invalid("entity asset source size exceeds bound"));
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(MAX_ENTITY_SOURCE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if bytes.len() > MAX_ENTITY_SOURCE_BYTES {
        return Err(invalid("entity asset source size exceeds bound"));
    }
    Ok(bytes)
}

fn parse_source(
    relative_path: &str,
    absolute_path: &Path,
    bytes: &[u8],
    symbols: &mut BTreeMap<(EntityAssetKind, Box<str>, Box<str>), PendingSymbol>,
    geometry_payloads: &mut BTreeMap<(Box<str>, Box<str>), PendingGeometry>,
) -> Result<(), AssetError> {
    if relative_path.starts_with("textures/entity/") {
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

    let value = if relative_path.starts_with("models/entity/") {
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
        parse_entity(relative_path, absolute_path, &value, symbols)
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
) -> Result<(), AssetError> {
    let description = value
        .get("minecraft:client_entity")
        .and_then(|entity| entity.get("description"))
        .and_then(Value::as_object)
        .ok_or_else(|| {
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
        EntityAssetKind::Entity,
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
    if digest != PINNED_MANIFEST_SHA256 {
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

const fn decode_sha256(hex: &[u8; 64]) -> [u8; 32] {
    let mut output = [0; 32];
    let mut index = 0;
    while index < output.len() {
        output[index] = (nibble(hex[index * 2]) << 4) | nibble(hex[index * 2 + 1]);
        index += 1;
    }
    output
}

const fn nibble(value: u8) -> u8 {
    match value {
        b'0'..=b'9' => value - b'0',
        b'a'..=b'f' => value - b'a' + 10,
        _ => panic!("invalid SHA-256"),
    }
}

fn invalid(detail: impl Into<Box<str>>) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}
