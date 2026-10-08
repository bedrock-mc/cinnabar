//! In-memory entity compile for a server pack's client entity families. Unlike the
//! vanilla carrier build it is lenient per file: an unparsable or oversized
//! source is skipped and counted, and items and equipment are not compiled.

use super::*;

/// Sources one pack may contribute before the rest are ignored.
pub const MAX_PACK_ENTITY_SOURCES: usize = 4_096;
/// Total source bytes one pack may contribute before the rest are ignored.
pub const MAX_PACK_ENTITY_BYTES: usize = 128 * 1024 * 1024;

/// Counted reasons pack sources were left out of the catalog.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EntityPackSkips {
    pub oversized: u32,
    pub unparsable: u32,
    /// Sources past the count or byte bound.
    pub over_budget: u32,
    /// Files dropped because the pack failed to compile with them and succeeded without.
    pub isolated: u32,
}

/// A pack's entity catalog. Symbols a pack shares with vanilla resolve as
/// external within it, so rigs that depend on vanilla clips are attributed in
/// `reference_outcomes` rather than compiled.
#[derive(Debug)]
pub struct EntityPackCompilation {
    pub assets: CompiledEntityAssets,
    pub reference_outcomes: Box<[CompileReferenceOutcome<u32>]>,
    /// Attachable bindings for the pack's items, geometry and texture resolved within it.
    pub equipment_bindings: Box<[EquipmentBinding]>,
    pub skipped: EntityPackSkips,
    /// Retained source bytes by pack-relative path, for consumers that read rasters.
    pub payloads: BTreeMap<Box<str>, Box<[u8]>>,
}

/// Whether a pack file feeds the entity catalog: definition JSON, geometry anywhere under
/// `models/`, and rasters anywhere under `textures/`.
fn in_families(path: &str) -> bool {
    let extension = path.rsplit_once('.').map(|(_, extension)| extension);
    let json = extension == Some("json");
    let raster = matches!(extension, Some("png" | "tga"));
    let material = path.starts_with("materials/") && extension == Some("material");
    (json
        && [
            "entity/",
            "models/",
            "animations/",
            "animation_controllers/",
            "render_controllers/",
            "attachables/",
        ]
        .iter()
        .any(|prefix| path.starts_with(prefix)))
        || (path.starts_with("textures/") && (raster || (json && path.contains("/entity/"))))
        || material
}

/// Geometry is keyed by identifier, so a file outside `models/entity/` is filed under it.
fn canonical_path(path: Box<str>) -> Box<str> {
    match path.strip_prefix("models/") {
        Some(rest) if !path.starts_with("models/entity/") => {
            format!("models/entity/_pack/{rest}").into()
        }
        _ => path,
    }
}

fn identifier_is_valid(identifier: &str) -> bool {
    !identifier.is_empty()
        && identifier.len() <= assets::MAX_ENTITY_IDENTIFIER_BYTES
        && !identifier.chars().any(char::is_control)
}

/// Whether every identifier a parsed file introduces would pass catalog validation.
fn file_identifiers_are_valid(
    symbols: &BTreeMap<(EntityAssetKind, Box<str>, Box<str>), PendingSymbol>,
    geometries: &BTreeMap<(Box<str>, Box<str>), PendingGeometry>,
) -> bool {
    symbols.values().all(|symbol| {
        identifier_is_valid(&symbol.identifier)
            && symbol
                .dependencies
                .iter()
                .all(|dependency| identifier_is_valid(&dependency.identifier))
    }) && geometries.values().all(|geometry| {
        identifier_is_valid(&geometry.identifier)
            && geometry.inherits.as_deref().is_none_or(identifier_is_valid)
    })
}

/// Most recompiles spent isolating one structurally invalid file.
const MAX_ISOLATION_ATTEMPTS: usize = 64;

/// Compiles `(pack-relative path, bytes)` files; `Ok(None)` when no usable
/// entity source remains. When the set is structurally invalid, single files are
/// dropped one at a time (entities first) until it compiles, and counted in
/// `isolated`; `Err` only when no single file explains the failure.
pub fn compile_entity_pack(
    files: Vec<(Box<str>, Vec<u8>)>,
) -> Result<Option<EntityPackCompilation>, AssetError> {
    let mut selected = files
        .into_iter()
        .filter(|(path, _)| in_families(path))
        .map(|(path, bytes)| (canonical_path(path), bytes))
        .collect::<Vec<_>>();
    selected.sort_by(|left, right| left.0.cmp(&right.0));
    selected.dedup_by(|later, earlier| later.0 == earlier.0);
    let first_error = match compile_selected(&selected, None) {
        Ok(compiled) => return Ok(compiled),
        Err(error) => error,
    };
    // Entity definitions are the likeliest culprits, then the files they pull in.
    let is_entity = |path: &str| path.starts_with("entity/");
    let candidates = selected
        .iter()
        .enumerate()
        .filter(|(_, (path, _))| is_entity(path))
        .chain(
            selected
                .iter()
                .enumerate()
                .filter(|(_, (path, _))| !is_entity(path)),
        )
        .map(|(index, _)| index)
        .take(MAX_ISOLATION_ATTEMPTS);
    for index in candidates {
        if let Ok(Some(mut compiled)) = compile_selected(&selected, Some(index)) {
            compiled.skipped.isolated += 1;
            return Ok(Some(compiled));
        }
    }
    Err(first_error)
}

/// One compile of `selected`, leaving out the file at `omit`.
fn compile_selected(
    selected: &[(Box<str>, Vec<u8>)],
    omit: Option<usize>,
) -> Result<Option<EntityPackCompilation>, AssetError> {
    let mut skipped = EntityPackSkips::default();
    let mut sources = Vec::new();
    let mut payloads = SourcePayloads::new();
    let mut symbols = BTreeMap::new();
    let mut geometries = BTreeMap::new();
    let mut total = 0usize;
    for (index, (path, bytes)) in selected.iter().enumerate() {
        if omit == Some(index) {
            continue;
        }
        let path = path.clone();
        // Geometry is normalised to the accepted schema; a file that leaves nothing is skipped.
        let normalised;
        let bytes = if path.starts_with("models/entity/") {
            let Some(text) = serde_json::from_slice::<serde_json::Value>(bytes)
                .ok()
                .filter(|_| true)
                .and_then(|mut value| {
                    sanitize::sanitize_geometry(&mut value).then(|| serde_json::to_vec(&value).ok())
                })
                .flatten()
            else {
                skipped.unparsable += 1;
                continue;
            };
            normalised = text;
            normalised.as_slice()
        } else {
            bytes.as_slice()
        };
        if bytes.len() > assets::MAX_ENTITY_SOURCE_BYTES {
            skipped.oversized += 1;
            continue;
        }
        let next_total = total.saturating_add(bytes.len());
        if sources.len() >= MAX_PACK_ENTITY_SOURCES || next_total > MAX_PACK_ENTITY_BYTES {
            skipped.over_budget += 1;
            continue;
        }
        // Parse into scratch maps so a failing file leaves no partial symbols.
        let mut file_symbols = BTreeMap::new();
        let mut file_geometries = BTreeMap::new();
        if parse_source(
            &path,
            Path::new(path.as_ref()),
            bytes,
            &mut file_symbols,
            &mut file_geometries,
        )
        .is_err()
        {
            skipped.unparsable += 1;
            continue;
        }
        if !file_identifiers_are_valid(&file_symbols, &file_geometries) {
            skipped.unparsable += 1;
            continue;
        }
        total = next_total;
        symbols.extend(file_symbols);
        geometries.extend(file_geometries);
        sources.push(EntityAssetSource {
            path: path.clone(),
            source_bytes: bytes.len() as u32,
            source_sha256: Sha256::digest(bytes).into(),
        });
        payloads.insert(path, bytes.into());
    }
    // Attachables alone (custom armor and held models) still make a pack worth compiling.
    let has_attachables = sources
        .iter()
        .any(|source| source.path.starts_with("attachables/"));
    if !has_attachables
        && !symbols
            .keys()
            .any(|(kind, ..)| *kind == EntityAssetKind::Entity)
    {
        return Ok(None);
    }
    let mut identity = Sha256::new();
    for source in &sources {
        identity.update(source.path.as_bytes());
        identity.update(source.source_sha256);
    }
    let compilation = assemble(
        Path::new(""),
        sources,
        &payloads,
        symbols,
        geometries,
        identity.finalize().into(),
        false,
    )?;
    Ok(Some(EntityPackCompilation {
        assets: compilation.assets,
        reference_outcomes: compilation.reference_outcomes,
        equipment_bindings: compilation.equipment_bindings,
        skipped,
        payloads,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn file(path: &str, text: &str) -> (Box<str>, Vec<u8>) {
        (path.into(), text.as_bytes().to_vec())
    }

    #[test]
    fn server_animation_without_entity_alias_keeps_its_named_bones() {
        let geometry = json!({"format_version":"1.12.0","minecraft:geometry":[{
            "description":{"identifier":"geometry.fixture"},
            "bones":[{"name":"head"},{"name":"body"}]
        }]});
        let entity = json!({"format_version":"1.10.0","minecraft:client_entity":{
            "description":{"identifier":"fixture:model",
                "geometry":{"default":"geometry.fixture"}}
        }});
        let animation = json!({"format_version":"1.8.0","animations":{
            "animation.fixture.server":{"animation_length":2.0,"bones":{
                "body":{"position":[0,16,0]},"absent":{"rotation":[90,0,0]}
            }}
        }});
        let compiled = compile_entity_pack(vec![
            file("entity/fixture.json", &entity.to_string()),
            file("models/entity/fixture.geo.json", &geometry.to_string()),
            file("animations/fixture.json", &animation.to_string()),
        ])
        .unwrap()
        .unwrap();
        let runtime = assets::RuntimeEntityAssets::from_compiled(compiled.assets).unwrap();
        let symbol = runtime
            .symbols()
            .iter()
            .position(|symbol| symbol.identifier.as_ref() == "animation.fixture.server")
            .unwrap() as u32;
        let clip = runtime
            .clip_for_geometry(symbol, 0)
            .expect("the server may select an animation absent from the entity's aliases");
        assert_eq!(
            runtime.animation_clips()[clip as usize]
                .length_seconds
                .get(),
            2.0
        );
        assert_eq!(runtime.animation_clips()[clip as usize].channel_count, 2);
    }

    // Bad and out-of-family files are dropped and counted; nothing usable yields None.
    #[test]
    fn unusable_sources_are_skipped_and_counted() {
        let result = compile_entity_pack(vec![
            file("entity/bad.json", "{not json"),
            file("ui/other.json", "{}"),
        ])
        .unwrap();
        assert!(result.is_none());
    }

    // An unparsable file never reaches assembly, and an empty pack stays None after isolation.
    #[test]
    fn isolation_leaves_an_unusable_pack_empty() {
        let result = compile_entity_pack(vec![file("animations/a.json", "{oops")]).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn server_entity_empty_texture_path_keeps_other_dependencies() {
        let entity = json!({"format_version":"1.18.0","minecraft:client_entity":{
            "description":{"identifier":"fixture:particles",
                "geometry":{"default":"geometry.marker"},
                "textures":{"default":"","alternate":"textures/entity/fixture"},
                "render_controllers":["controller.render.marker"]}
        }});
        let compiled =
            compile_entity_pack(vec![file("entity/particles.json", &entity.to_string())])
                .unwrap()
                .expect("an absent texture cannot discard a controller-only entity");
        assert_eq!(compiled.skipped.unparsable, 0);
        let symbol = compiled
            .assets
            .symbols
            .iter()
            .find(|symbol| {
                symbol.kind == EntityAssetKind::Entity
                    && symbol.identifier.as_ref() == "fixture:particles"
            })
            .unwrap();
        assert!(
            symbol
                .dependencies
                .iter()
                .all(|dependency| !dependency.identifier.is_empty())
        );
        assert!(symbol.dependencies.iter().any(|dependency| {
            dependency.kind == EntityDependencyKind::RenderController
                && dependency.identifier.as_ref() == "controller.render.marker"
        }));
        assert!(symbol.dependencies.iter().any(|dependency| {
            dependency.kind == EntityDependencyKind::Texture
                && dependency.identifier.as_ref() == "textures/entity/fixture"
        }));
    }

    fn compile_synthetic_geometry(bones: Value) -> EntityPackCompilation {
        let geometry = json!({"format_version":"1.12.0","minecraft:geometry":[{
            "description":{"identifier":"geometry.fixture"}, "bones":bones
        }]});
        let entity = json!({"format_version":"1.10.0","minecraft:client_entity":{
            "description":{"identifier":"fixture:model",
                "geometry":{"default":"geometry.fixture"}}
        }});
        compile_entity_pack(vec![
            file("entity/fixture.json", &entity.to_string()),
            file("models/entity/fixture.geo.json", &geometry.to_string()),
        ])
        .unwrap()
        .unwrap()
    }

    #[test]
    fn server_entity_crowd_geometry_preserves_all_authored_bones() {
        let bones: Vec<_> = (0..1_024)
            .map(|index| {
                json!({
                    "name":format!("part_{index}"),"pivot":[0,0,0]
                })
            })
            .collect();
        let compiled = compile_synthetic_geometry(bones.into());
        assert_eq!(compiled.skipped.unparsable, 0);
        assert_eq!(compiled.assets.geometries[0].bones.len(), 1_024);
        let bytes = assets::encode_entity_blob(&compiled.assets).unwrap();
        let runtime = assets::RuntimeEntityAssets::decode(&bytes).unwrap();
        assert_eq!(runtime.geometries()[0].bones.len(), 1_024);
    }

    #[test]
    fn server_entity_large_static_geometry_preserves_all_authored_cubes() {
        let cubes = vec![json!({"origin":[0,0,0],"size":[1,1,1],"uv":[0,0]}); 16_384];
        let compiled = compile_synthetic_geometry(json!([{"name":"root","cubes":cubes}]));
        assert_eq!(compiled.skipped.unparsable, 0);
        assert_eq!(compiled.assets.geometries[0].bones[0].cubes.len(), 16_384);
        let bytes = assets::encode_entity_blob(&compiled.assets).unwrap();
        let runtime = assets::RuntimeEntityAssets::decode(&bytes).unwrap();
        assert_eq!(runtime.geometries()[0].bones[0].cubes.len(), 16_384);
    }

    #[test]
    fn family_filter_matches_directory_and_extension() {
        assert!(in_families("entity/a.json"));
        assert!(in_families("textures/entity/a/b.png"));
        assert!(in_families("textures/cc/buddy/a.buddy.png"));
        assert!(in_families("models/mobs/a.geo.json"));
        assert!(!in_families("entity/a.png"));
        assert!(!in_families("ui/a.json"));
        assert_eq!(
            canonical_path("models/mobs/a.geo.json".into()).as_ref(),
            "models/entity/_pack/mobs/a.geo.json"
        );
    }
}
