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
    EntityGeometryBone, EntityGeometryCube, EntityGeometryFaceUv, EntityGeometryFaceUvs,
    EntityGeometryInheritance, EntityGeometryScalar, EntityGeometryUv, MAX_ENTITY_ASSET_SOURCES,
    MAX_ENTITY_ASSET_SYMBOLS, MAX_ENTITY_DEPENDENCIES, MAX_ENTITY_GEOMETRIES,
    MAX_ENTITY_GEOMETRY_BONES, MAX_ENTITY_GEOMETRY_CUBES, MAX_ENTITY_GEOMETRY_NAME_BYTES,
    MAX_ENTITY_SOURCE_BYTES, MAX_ENTITY_TEXTURE_DIMENSION, MAX_ENTITY_TOTAL_SOURCE_BYTES,
    validate_entity_geometry_inheritance,
};
use serde::{Deserialize, Deserializer, de};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

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

/// Compiles deterministic entity animation authority metadata from the exact
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
        sources: sources.into_boxed_slice(),
        symbols: symbols.into_boxed_slice(),
        geometries: geometries.into_boxed_slice(),
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

fn parse_geometry(
    relative_path: &str,
    path: &Path,
    value: &Value,
    symbols: &mut BTreeMap<(EntityAssetKind, Box<str>, Box<str>), PendingSymbol>,
    geometry_payloads: &mut BTreeMap<(Box<str>, Box<str>), PendingGeometry>,
) -> Result<(), AssetError> {
    let root = value
        .as_object()
        .ok_or_else(|| invalid(format!("invalid geometry root in {}", path.display())))?;
    let is_modern = root.contains_key("minecraft:geometry");
    let format_version = required_string(value, "format_version", path)?;
    let supported_version = if is_modern {
        matches!(
            format_version,
            "1.12.0" | "1.16.0" | "1.21.0" | "1.21.120" | "1.26.10"
        )
    } else {
        matches!(format_version, "1.8.0" | "1.10.0")
    };
    if !supported_version {
        return Err(invalid(format!(
            "unsupported entity geometry format version or schema branch in {}",
            path.display()
        )));
    }
    if is_modern {
        if root.len() != 2 {
            return Err(invalid(format!(
                "unknown modern geometry root field in {}",
                path.display()
            )));
        }
        let geometries = root
            .get("minecraft:geometry")
            .ok_or_else(|| invalid("missing modern entity geometry payload"))?
            .as_array()
            .ok_or_else(|| invalid(format!("invalid geometry array in {}", path.display())))?;
        for geometry in geometries {
            validate_object_fields(geometry, path, &["description", "bones"], &["description"])?;
            let description = geometry.get("description").ok_or_else(|| {
                invalid(format!(
                    "missing geometry description in {}",
                    path.display()
                ))
            })?;
            validate_object_fields(
                description,
                path,
                &[
                    "identifier",
                    "texture_width",
                    "texture_height",
                    "visible_bounds_width",
                    "visible_bounds_height",
                    "visible_bounds_offset",
                ],
                &["identifier"],
            )?;
            let identifier = required_string(description, "identifier", path)?;
            let texture_width = optional_texture_dimension(description, "texture_width", path)?;
            let texture_height = optional_texture_dimension(description, "texture_height", path)?;
            let bones = parse_geometry_bones(geometry.get("bones"), path)?;
            insert_symbol(
                symbols,
                EntityAssetKind::Geometry,
                identifier,
                relative_path,
                Box::new([]),
            )?;
            insert_geometry(
                geometry_payloads,
                identifier,
                None,
                relative_path,
                texture_width,
                texture_height,
                bones,
            )?;
        }
    } else {
        let identifiers = root
            .keys()
            .filter(|field| field.as_str() != "format_version")
            .collect::<Vec<_>>();
        if identifiers.is_empty()
            || identifiers
                .iter()
                .any(|identifier| !identifier.starts_with("geometry."))
        {
            return Err(invalid(format!(
                "unknown legacy geometry root field in {}",
                path.display()
            )));
        }
        for raw_identifier in identifiers {
            let raw_identifier = raw_identifier.as_str();
            let (identifier, inherits) = match raw_identifier.split_once(':') {
                Some((identifier, inherits))
                    if !identifier.is_empty()
                        && !inherits.is_empty()
                        && !inherits.contains(':')
                        && identifier.starts_with("geometry.")
                        && inherits.starts_with("geometry.") =>
                {
                    (identifier, Some(inherits))
                }
                Some(_) => return Err(invalid("invalid legacy entity geometry inheritance key")),
                None => (raw_identifier, None),
            };
            let geometry = root
                .get(raw_identifier)
                .ok_or_else(|| invalid("missing legacy entity geometry"))?;
            validate_object_fields(
                geometry,
                path,
                &[
                    "texturewidth",
                    "textureheight",
                    "visible_bounds_width",
                    "visible_bounds_height",
                    "visible_bounds_offset",
                    "bones",
                ],
                &[],
            )?;
            let texture_width = optional_texture_dimension(geometry, "texturewidth", path)?;
            let texture_height = optional_texture_dimension(geometry, "textureheight", path)?;
            let bones = parse_geometry_bones_with_inheritance(
                geometry.get("bones"),
                path,
                inherits.is_some(),
            )?;
            insert_symbol(
                symbols,
                EntityAssetKind::Geometry,
                identifier,
                relative_path,
                Box::new([]),
            )?;
            insert_geometry(
                geometry_payloads,
                identifier,
                inherits,
                relative_path,
                texture_width,
                texture_height,
                bones,
            )?;
        }
    }
    Ok(())
}

fn insert_geometry(
    geometries: &mut BTreeMap<(Box<str>, Box<str>), PendingGeometry>,
    identifier: &str,
    inherits: Option<&str>,
    source_path: &str,
    texture_width: Option<u16>,
    texture_height: Option<u16>,
    bones: Box<[EntityGeometryBone]>,
) -> Result<(), AssetError> {
    let identifier: Box<str> = identifier.into();
    let source_path: Box<str> = source_path.into();
    let geometry = PendingGeometry {
        identifier: identifier.clone(),
        inherits: inherits.map(Into::into),
        source_path: source_path.clone(),
        texture_width,
        texture_height,
        bones,
    };
    if geometries
        .insert((identifier.clone(), source_path), geometry)
        .is_some()
    {
        return Err(invalid(format!(
            "duplicate entity geometry payload `{identifier}` within one source"
        )));
    }
    if geometries.len() > MAX_ENTITY_GEOMETRIES {
        return Err(invalid("entity geometry count exceeds bound"));
    }
    Ok(())
}

fn parse_geometry_bones(
    value: Option<&Value>,
    path: &Path,
) -> Result<Box<[EntityGeometryBone]>, AssetError> {
    parse_geometry_bones_with_inheritance(value, path, false)
}

fn parse_geometry_bones_with_inheritance(
    value: Option<&Value>,
    path: &Path,
    allow_inherited_parent: bool,
) -> Result<Box<[EntityGeometryBone]>, AssetError> {
    let Some(value) = value else {
        return Ok(Box::new([]));
    };
    let bones = value.as_array().ok_or_else(|| {
        invalid(format!(
            "geometry bones must be an array in {}",
            path.display()
        ))
    })?;
    if bones.len() > MAX_ENTITY_GEOMETRY_BONES {
        return Err(invalid("entity geometry bone count exceeds bound"));
    }
    let mut parsed = Vec::with_capacity(bones.len());
    let mut total_cubes = 0usize;
    for bone in bones {
        validate_object_fields(
            bone,
            path,
            &[
                "name",
                "parent",
                "pivot",
                "rotation",
                "cubes",
                "mirror",
                "inflate",
                "locators",
                "binding",
                "texture_meshes",
                "neverRender",
                "reset",
                "bind_pose_rotation",
            ],
            &["name"],
        )?;
        validate_known_deferred_bone_fields(bone, path)?;
        let name = required_string(bone, "name", path)?;
        let parent = optional_string(bone, "parent", path)?
            .filter(|parent| !parent.eq_ignore_ascii_case(name))
            .map(Into::into);
        let pivot = optional_vec(bone, "pivot", path)?;
        let rotation = optional_vec(bone, "rotation", path)?;
        let mirror = optional_bool(bone, "mirror", path)?;
        let inflate = optional_scalar(bone, "inflate", path)?;
        let never_render = optional_bool(bone, "neverRender", path)?;
        let reset = optional_bool(bone, "reset", path)?;
        let cubes = bone
            .get("cubes")
            .map(|cubes| {
                parse_geometry_cubes(
                    cubes,
                    path,
                    mirror.unwrap_or(false),
                    inflate.unwrap_or_else(zero_scalar),
                )
            })
            .transpose()?
            .unwrap_or_default();
        total_cubes = total_cubes
            .checked_add(cubes.len())
            .ok_or_else(|| invalid("entity geometry cube count overflow"))?;
        if total_cubes > MAX_ENTITY_GEOMETRY_CUBES {
            return Err(invalid("entity geometry cube count exceeds bound"));
        }
        parsed.push(EntityGeometryBone {
            name: name.into(),
            parent,
            pivot,
            rotation,
            mirror,
            inflate,
            never_render,
            reset,
            cubes,
        });
    }
    validate_bone_hierarchy(&parsed, path, allow_inherited_parent)?;
    Ok(parsed.into_boxed_slice())
}

fn validate_bone_hierarchy(
    bones: &[EntityGeometryBone],
    path: &Path,
    allow_inherited_parent: bool,
) -> Result<(), AssetError> {
    for bone in bones {
        if bone.name.is_empty()
            || bone.name.len() > MAX_ENTITY_GEOMETRY_NAME_BYTES
            || bone.name.chars().any(char::is_control)
        {
            return Err(invalid(format!(
                "invalid entity geometry bone name in {}",
                path.display()
            )));
        }
        if let Some(parent) = &bone.parent {
            let invalid_parent = parent.is_empty()
                || parent.len() > MAX_ENTITY_GEOMETRY_NAME_BYTES
                || parent.chars().any(char::is_control)
                || parent.eq_ignore_ascii_case(&bone.name)
                || (!allow_inherited_parent
                    && !bones
                        .iter()
                        .any(|candidate| candidate.name.eq_ignore_ascii_case(parent)));
            if invalid_parent {
                return Err(invalid(format!(
                    "invalid entity geometry bone parent in {}",
                    path.display()
                )));
            }
        }
    }
    for start in 0..bones.len() {
        let mut current = Some(start);
        for step in 0..=bones.len() {
            let Some(index) = current else {
                break;
            };
            if step == bones.len() {
                return Err(invalid(format!(
                    "entity geometry bone hierarchy contains a cycle in {}",
                    path.display()
                )));
            }
            current = bones[index].parent.as_ref().and_then(|parent| {
                bones
                    .iter()
                    .position(|candidate| candidate.name.eq_ignore_ascii_case(parent))
            });
        }
    }
    Ok(())
}

fn parse_geometry_cubes(
    value: &Value,
    path: &Path,
    bone_mirror: bool,
    bone_inflate: EntityGeometryScalar,
) -> Result<Box<[EntityGeometryCube]>, AssetError> {
    let cubes = value.as_array().ok_or_else(|| {
        invalid(format!(
            "geometry cubes must be an array in {}",
            path.display()
        ))
    })?;
    if cubes.len() > MAX_ENTITY_GEOMETRY_CUBES {
        return Err(invalid("entity geometry cube count exceeds bound"));
    }
    cubes
        .iter()
        .map(|cube| {
            validate_object_fields(
                cube,
                path,
                &[
                    "origin", "size", "pivot", "rotation", "uv", "inflate", "mirror",
                ],
                &["origin", "size"],
            )?;
            let size = required_vec(cube, "size", path)?;
            if size.iter().any(|value| value.get() < 0.0) {
                return Err(invalid("entity geometry cube size is negative"));
            }
            Ok(EntityGeometryCube {
                origin: required_vec(cube, "origin", path)?,
                size,
                pivot: optional_vec(cube, "pivot", path)?.unwrap_or_else(zero_vec3),
                rotation: optional_vec(cube, "rotation", path)?.unwrap_or_else(zero_vec3),
                uv: cube
                    .get("uv")
                    .map(|uv| parse_geometry_uv(uv, path))
                    .transpose()?
                    .unwrap_or_else(|| EntityGeometryUv::Box(zero_vec2())),
                inflate: optional_scalar(cube, "inflate", path)?.unwrap_or(bone_inflate),
                mirror: optional_bool(cube, "mirror", path)?.unwrap_or(bone_mirror),
            })
        })
        .collect::<Result<Vec<_>, AssetError>>()
        .map(Vec::into_boxed_slice)
}

fn parse_geometry_uv(value: &Value, path: &Path) -> Result<EntityGeometryUv, AssetError> {
    if value.is_array() {
        return Ok(EntityGeometryUv::Box(parse_vec(value, "uv", path)?));
    }
    let object = value.as_object().ok_or_else(|| {
        invalid(format!(
            "entity geometry UV must be an array or face object in {}",
            path.display()
        ))
    })?;
    const FACE_NAMES: [&str; 6] = ["north", "south", "east", "west", "up", "down"];
    if object.is_empty()
        || object
            .keys()
            .any(|field| !FACE_NAMES.contains(&field.as_str()))
    {
        return Err(invalid("unknown or empty entity geometry face UV map"));
    }
    let parse_face = |name: &str| -> Result<Option<EntityGeometryFaceUv>, AssetError> {
        object
            .get(name)
            .map(|face| {
                validate_object_fields(face, path, &["uv", "uv_size"], &["uv"])?;
                Ok(EntityGeometryFaceUv {
                    uv: required_vec(face, "uv", path)?,
                    uv_size: optional_vec(face, "uv_size", path)?,
                })
            })
            .transpose()
    };
    Ok(EntityGeometryUv::Faces(EntityGeometryFaceUvs {
        north: parse_face("north")?,
        south: parse_face("south")?,
        east: parse_face("east")?,
        west: parse_face("west")?,
        up: parse_face("up")?,
        down: parse_face("down")?,
    }))
}

fn validate_object_fields(
    value: &Value,
    path: &Path,
    allowed: &[&str],
    required: &[&str],
) -> Result<(), AssetError> {
    let object = value.as_object().ok_or_else(|| {
        invalid(format!(
            "entity geometry value must be an object in {}",
            path.display()
        ))
    })?;
    if object
        .keys()
        .any(|field| !allowed.contains(&field.as_str()))
        || required.iter().any(|field| !object.contains_key(*field))
    {
        return Err(invalid(format!(
            "unknown or missing entity geometry field in {}",
            path.display()
        )));
    }
    Ok(())
}

fn required_string<'a>(value: &'a Value, field: &str, path: &Path) -> Result<&'a str, AssetError> {
    value.get(field).and_then(Value::as_str).ok_or_else(|| {
        invalid(format!(
            "entity geometry field `{field}` must be a string in {}",
            path.display()
        ))
    })
}

fn optional_string<'a>(
    value: &'a Value,
    field: &str,
    path: &Path,
) -> Result<Option<&'a str>, AssetError> {
    value
        .get(field)
        .map(|value| {
            value.as_str().ok_or_else(|| {
                invalid(format!(
                    "entity geometry field `{field}` must be a string in {}",
                    path.display()
                ))
            })
        })
        .transpose()
}

fn optional_texture_dimension(
    value: &Value,
    field: &str,
    path: &Path,
) -> Result<Option<u16>, AssetError> {
    let Some(raw) = value.get(field) else {
        return Ok(None);
    };
    let number = raw.as_f64().ok_or_else(|| {
        invalid(format!(
            "entity geometry texture dimension `{field}` must be numeric in {}",
            path.display()
        ))
    })?;
    if !number.is_finite()
        || number.fract() != 0.0
        || number < 1.0
        || number > f64::from(MAX_ENTITY_TEXTURE_DIMENSION)
    {
        return Err(invalid("entity geometry texture dimension exceeds bound"));
    }
    Ok(Some(number as u16))
}

fn scalar(value: &Value, field: &str, path: &Path) -> Result<EntityGeometryScalar, AssetError> {
    let number = value.as_f64().ok_or_else(|| {
        invalid(format!(
            "entity geometry scalar `{field}` must be numeric in {}",
            path.display()
        ))
    })?;
    EntityGeometryScalar::new(number as f32)
        .ok_or_else(|| invalid(format!("entity geometry scalar `{field}` exceeds bound")))
}

fn required_vec<const N: usize>(
    value: &Value,
    field: &str,
    path: &Path,
) -> Result<[EntityGeometryScalar; N], AssetError> {
    let value = value.get(field).ok_or_else(|| {
        invalid(format!(
            "missing entity geometry vector `{field}` in {}",
            path.display()
        ))
    })?;
    parse_vec(value, field, path)
}

fn optional_vec<const N: usize>(
    value: &Value,
    field: &str,
    path: &Path,
) -> Result<Option<[EntityGeometryScalar; N]>, AssetError> {
    value
        .get(field)
        .map(|value| parse_vec(value, field, path))
        .transpose()
}

fn parse_vec<const N: usize>(
    value: &Value,
    field: &str,
    path: &Path,
) -> Result<[EntityGeometryScalar; N], AssetError> {
    let values = value.as_array().ok_or_else(|| {
        invalid(format!(
            "entity geometry vector `{field}` must be an array in {}",
            path.display()
        ))
    })?;
    if values.len() != N {
        return Err(invalid(format!(
            "entity geometry vector `{field}` has the wrong length"
        )));
    }
    let parsed = values
        .iter()
        .map(|value| scalar(value, field, path))
        .collect::<Result<Vec<_>, _>>()?;
    parsed
        .try_into()
        .map_err(|_| invalid("entity geometry vector has the wrong length"))
}

fn optional_scalar(
    value: &Value,
    field: &str,
    path: &Path,
) -> Result<Option<EntityGeometryScalar>, AssetError> {
    value
        .get(field)
        .map(|value| scalar(value, field, path))
        .transpose()
}

fn optional_bool(value: &Value, field: &str, path: &Path) -> Result<Option<bool>, AssetError> {
    value
        .get(field)
        .map(|value| {
            value.as_bool().ok_or_else(|| {
                invalid(format!(
                    "entity geometry field `{field}` must be boolean in {}",
                    path.display()
                ))
            })
        })
        .transpose()
}

fn zero_scalar() -> EntityGeometryScalar {
    EntityGeometryScalar::new(0.0).expect("zero is a canonical geometry scalar")
}

fn zero_vec3() -> [EntityGeometryScalar; 3] {
    [zero_scalar(); 3]
}

fn zero_vec2() -> [EntityGeometryScalar; 2] {
    [zero_scalar(); 2]
}

fn validate_known_deferred_bone_fields(value: &Value, path: &Path) -> Result<(), AssetError> {
    if value
        .get("locators")
        .is_some_and(|value| !value.is_object())
        || value
            .get("texture_meshes")
            .is_some_and(|value| !value.is_array())
    {
        return Err(invalid(format!(
            "invalid deferred entity geometry object in {}",
            path.display()
        )));
    }
    optional_string(value, "binding", path)?;
    optional_bool(value, "neverRender", path)?;
    optional_bool(value, "reset", path)?;
    let _: Option<[EntityGeometryScalar; 3]> = optional_vec(value, "bind_pose_rotation", path)?;
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

fn parse_unique_json(path: &Path, bytes: &[u8]) -> Result<Value, AssetError> {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let uncommented = strip_json_comments(bytes)?;
    let mut deserializer = serde_json::Deserializer::from_slice(&uncommented);
    let UniqueRootValue(value) =
        UniqueRootValue::deserialize(&mut deserializer).map_err(|source| AssetError::Json {
            path: path.to_path_buf(),
            source,
        })?;
    deserializer.end().map_err(|source| AssetError::Json {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(value)
}

fn parse_fully_unique_json(path: &Path, bytes: &[u8]) -> Result<Value, AssetError> {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let uncommented = strip_json_comments(bytes)?;
    let mut deserializer = serde_json::Deserializer::from_slice(&uncommented);
    let UniqueNestedValue(value) =
        UniqueNestedValue::deserialize(&mut deserializer).map_err(|source| AssetError::Json {
            path: path.to_path_buf(),
            source,
        })?;
    deserializer.end().map_err(|source| AssetError::Json {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(value)
}

fn strip_json_comments(bytes: &[u8]) -> Result<Vec<u8>, AssetError> {
    #[derive(Clone, Copy)]
    enum State {
        Normal,
        String,
        LineComment,
        BlockComment,
    }

    let mut output = bytes.to_vec();
    let mut state = State::Normal;
    let mut index = 0;
    while index < bytes.len() {
        match state {
            State::Normal => match bytes[index] {
                b'"' => {
                    state = State::String;
                    index += 1;
                }
                b'/' if bytes.get(index + 1) == Some(&b'/') => {
                    output[index] = b' ';
                    output[index + 1] = b' ';
                    state = State::LineComment;
                    index += 2;
                }
                b'/' if bytes.get(index + 1) == Some(&b'*') => {
                    output[index] = b' ';
                    output[index + 1] = b' ';
                    state = State::BlockComment;
                    index += 2;
                }
                _ => index += 1,
            },
            State::String => match bytes[index] {
                b'\\' => index = (index + 2).min(bytes.len()),
                b'"' => {
                    state = State::Normal;
                    index += 1;
                }
                _ => index += 1,
            },
            State::LineComment => match bytes[index] {
                b'\n' | b'\r' => {
                    state = State::Normal;
                    index += 1;
                }
                _ => {
                    output[index] = b' ';
                    index += 1;
                }
            },
            State::BlockComment => {
                if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    output[index] = b' ';
                    output[index + 1] = b' ';
                    state = State::Normal;
                    index += 2;
                } else {
                    if bytes[index] != b'\n' && bytes[index] != b'\r' {
                        output[index] = b' ';
                    }
                    index += 1;
                }
            }
        }
    }
    if matches!(state, State::BlockComment) {
        return Err(invalid("unterminated block comment in entity JSON source"));
    }
    Ok(output)
}

struct UniqueRootValue(Value);

impl<'de> Deserialize<'de> for UniqueRootValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(UniqueRootValueVisitor)
    }
}

struct UniqueRootValueVisitor;

impl<'de> de::Visitor<'de> for UniqueRootValueVisitor {
    type Value = UniqueRootValue;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON object without duplicate root keys")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        let mut values = Map::new();
        while let Some((key, value)) = map.next_entry::<String, Value>()? {
            if values.insert(key.clone(), value).is_some() {
                return Err(de::Error::custom(format!("duplicate JSON key `{key}`")));
            }
        }
        Ok(UniqueRootValue(Value::Object(values)))
    }
}

struct UniqueNestedValue(Value);

impl<'de> Deserialize<'de> for UniqueNestedValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(UniqueNestedValueVisitor)
    }
}

struct UniqueNestedValueVisitor;

impl<'de> de::Visitor<'de> for UniqueNestedValueVisitor {
    type Value = UniqueNestedValue;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("JSON without duplicate object keys")
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(UniqueNestedValue(Value::Null))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(UniqueNestedValue(Value::Null))
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(UniqueNestedValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(UniqueNestedValue(Value::Number(value.into())))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(UniqueNestedValue(Value::Number(value.into())))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .map(UniqueNestedValue)
            .ok_or_else(|| de::Error::custom("invalid non-finite JSON number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(UniqueNestedValue(Value::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(UniqueNestedValue(Value::String(value)))
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: de::SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(UniqueNestedValue(value)) = sequence.next_element()? {
            values.push(value);
        }
        Ok(UniqueNestedValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        let mut values = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            let UniqueNestedValue(value) = map.next_value()?;
            if values.insert(key.clone(), value).is_some() {
                return Err(de::Error::custom(format!("duplicate JSON key `{key}`")));
            }
        }
        Ok(UniqueNestedValue(Value::Object(values)))
    }
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
