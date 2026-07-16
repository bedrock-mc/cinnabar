use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

use assets::{
    AssetError, CompiledEntityAssets, EntityAssetKind, EntityAssetSource, EntityAssetSymbol,
    EntityDependency, EntityDependencyKind, MAX_ENTITY_ASSET_SOURCES, MAX_ENTITY_ASSET_SYMBOLS,
    MAX_ENTITY_DEPENDENCIES, MAX_ENTITY_SOURCE_BYTES, MAX_ENTITY_TOTAL_SOURCE_BYTES,
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
        parse_source(&relative_path, &absolute_path, &bytes, &mut symbols)?;
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
    let symbols = symbols
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
    Ok(CompiledEntityAssets {
        source_manifest_sha256,
        sources: sources.into_boxed_slice(),
        symbols: symbols.into_boxed_slice(),
    })
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
) -> Result<(), AssetError> {
    if relative_path.starts_with("textures/entity/") {
        if relative_path.ends_with(".png") || relative_path.ends_with(".tga") {
            return insert_symbol(
                symbols,
                EntityAssetKind::Texture,
                relative_path,
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

    let value = parse_unique_json(absolute_path, bytes)?;
    if relative_path.starts_with("entity/") {
        validate_root_fields(
            &value,
            absolute_path,
            &["format_version", "minecraft:client_entity"],
            &["format_version", "minecraft:client_entity"],
        )?;
        parse_entity(relative_path, absolute_path, &value, symbols)
    } else if relative_path.starts_with("models/entity/") {
        parse_geometry(relative_path, absolute_path, &value, symbols)
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
            });
        }
    }
    collect_named_dependencies(
        description.get("animation_controllers"),
        EntityDependencyKind::AnimationController,
        &mut dependencies,
    )?;
    collect_named_dependencies(
        description.get("render_controllers"),
        EntityDependencyKind::RenderController,
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
        });
        if dependencies.len() > MAX_ENTITY_DEPENDENCIES {
            return Err(invalid("client entity dependency count exceeds bound"));
        }
    }
    Ok(())
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
) -> Result<(), AssetError> {
    let root = value
        .as_object()
        .ok_or_else(|| invalid(format!("invalid geometry root in {}", path.display())))?;
    if !root.contains_key("format_version") {
        return Err(invalid(format!(
            "missing geometry format version in {}",
            path.display()
        )));
    }
    if let Some(geometries) = root.get("minecraft:geometry") {
        if root.len() != 2 {
            return Err(invalid(format!(
                "unknown modern geometry root field in {}",
                path.display()
            )));
        }
        let geometries = geometries
            .as_array()
            .ok_or_else(|| invalid(format!("invalid geometry array in {}", path.display())))?;
        for geometry in geometries {
            let identifier = geometry
                .get("description")
                .and_then(|description| description.get("identifier"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    invalid(format!("missing geometry identifier in {}", path.display()))
                })?;
            insert_symbol(
                symbols,
                EntityAssetKind::Geometry,
                identifier,
                relative_path,
                Box::new([]),
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
        for identifier in identifiers {
            insert_symbol(
                symbols,
                EntityAssetKind::Geometry,
                identifier,
                relative_path,
                Box::new([]),
            )?;
        }
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
