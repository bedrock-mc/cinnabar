//! Compiles built-in hanging pictures using the accepted pack's painting rasters.
use super::*;
use assets::painting::{PAINTING_ATLAS, PAINTING_ATLAS_SIDE, PAINTING_MOTIVES, PaintingMotive};
use serde_json::json;

const CONTROLLER: &str = "controller.render.cinnabar.painting";

/// Adds complete static parts with retained rasters; bounded packs keep authored sources first.
pub(super) fn complete_bindings(
    root: &Path,
    sources: &mut Vec<EntityAssetSource>,
    payloads: &mut SourcePayloads,
    symbols: &mut BTreeMap<(EntityAssetKind, Box<str>, Box<str>), PendingSymbol>,
    geometries: &mut BTreeMap<(Box<str>, Box<str>), PendingGeometry>,
    remaining_budget: Option<(usize, usize)>,
) -> Result<usize, AssetError> {
    let has_texture = |texture: &str| {
        ["png", "tga"]
            .iter()
            .any(|extension| payloads.contains_key(format!("{texture}.{extension}").as_str()))
    };
    let parts: Vec<_> = PAINTING_MOTIVES
        .iter()
        .copied()
        .flat_map(|motive| {
            [false, true].into_iter().filter_map(move |border| {
                let texture = if border {
                    PAINTING_ATLAS.into()
                } else {
                    motive.texture()
                };
                has_texture(&texture).then_some((motive, border, texture))
            })
        })
        .collect();
    if parts.is_empty()
        || remaining_budget.is_some_and(|(sources, bytes)| sources < 3 || bytes == 0)
    {
        return Ok(0);
    }
    let mut bytes_added = 0;
    let mut sources_added = 0;
    let mut controller = Some(encode_document(
        unused_source_path(
            "render_controllers/_cinnabar/paintings.json".into(),
            payloads,
        ),
        json!({
            "format_version":"1.8.0", "render_controllers": {CONTROLLER: {
                "geometry":"Geometry.default", "materials":[{"*":"Material.default"}],
                "textures":["Texture.default"]
            }}
        }),
    )?);
    for (motive, border, texture) in parts {
        let identifier = motive.part_identifier(border);
        let part = if border { "border" } else { "front" };
        let geometry = format!("geometry.cinnabar.painting.{}.{}", motive.title, part);
        let entity = encode_document(
            unused_source_path(
                format!("entity/_cinnabar/painting_{}_{}.json", motive.title, part),
                payloads,
            ),
            json!({
                "format_version":"1.10.0", "minecraft:client_entity":{"description":{
                    "identifier":identifier, "materials":{"default":"entity_alphatest"},
                    "textures":{"default":texture}, "geometry":{"default":geometry},
                    "render_controllers":[CONTROLLER]
                }}
            }),
        )?;
        let geometry = encode_document(
            unused_source_path(
                format!(
                    "models/entity/_cinnabar/painting_{}_{}.json",
                    motive.title, part
                ),
                payloads,
            ),
            geometry_document(motive, border, &geometry),
        )?;
        let next_sources = sources_added + 2 + usize::from(controller.is_some());
        let next_bytes = bytes_added
            + entity.1.len()
            + geometry.1.len()
            + controller.as_ref().map_or(0, |(_, bytes)| bytes.len());
        if remaining_budget.is_some()
            && symbols.len() + 2 + usize::from(controller.is_some())
                > assets::MAX_ENTITY_ASSET_SYMBOLS
        {
            continue;
        }
        if remaining_budget.is_some_and(|(count, bytes)| next_sources > count || next_bytes > bytes)
        {
            continue;
        }
        for (path, bytes) in controller.take().into_iter().chain([entity, geometry]) {
            parse_source(&path, &root.join(&path), &bytes, symbols, geometries)?;
            sources.push(EntityAssetSource {
                path: path.clone().into(),
                source_bytes: bytes.len() as u32,
                source_sha256: Sha256::digest(&bytes).into(),
            });
            payloads.insert(path.into(), bytes.into());
        }
        sources_added = next_sources;
        bytes_added = next_bytes;
    }
    Ok(bytes_added)
}

/// Keeps every authored payload intact when its path matches a generated document.
pub(super) fn unused_source_path(path: String, payloads: &SourcePayloads) -> String {
    if !payloads.contains_key(path.as_str()) {
        return path;
    }
    let stem = path
        .strip_suffix(".json")
        .expect("generated documents are JSON");
    let mut suffix = 1;
    loop {
        let candidate = format!("{stem}_{suffix}.json");
        if !payloads.contains_key(candidate.as_str()) {
            return candidate;
        }
        suffix += 1;
    }
}

/// Serializes one original generated document before testing whole-part admission.
pub(super) fn encode_document(path: String, value: Value) -> Result<(String, Vec<u8>), AssetError> {
    let bytes =
        serde_json::to_vec(&value).map_err(|_| invalid("generated binding encoding failed"))?;
    Ok((path, bytes))
}

/// Builds the centered front plane or tiled one-pixel border independently of raster resolution.
fn geometry_document(motive: PaintingMotive, border: bool, identifier: &str) -> Value {
    let [width, height] = motive.size.map(f32::from);
    let mut cubes = Vec::new();
    let texture_size;
    let half_depth = assets::painting::PAINTING_HALF_DEPTH * 16.0;
    let [border_u, border_v] = assets::painting::PAINTING_BORDER_ORIGIN;
    let border_sample = assets::painting::PAINTING_BORDER_SAMPLE_PIXELS;
    if border {
        texture_size = [PAINTING_ATLAS_SIDE; 2];
        for x in 0..motive.size[0] / 16 {
            for y in 0..motive.size[1] / 16 {
                cubes.push(json!({
                    "origin":[-width/2.0 + f32::from(x)*16.0, -height/2.0 + f32::from(y)*16.0, -half_depth],
                    "size":[16,16,2.0*half_depth], "uv": {
                        "south":{"uv":[border_u,border_v],"uv_size":[16,16]},
                        "east":{"uv":[border_u+border_sample,border_v],"uv_size":[0,16]},
                        "west":{"uv":[border_u+border_sample,border_v],"uv_size":[0,16]},
                        "up":{"uv":[border_u,border_v],"uv_size":[16,border_sample]},
                        "down":{"uv":[border_u,border_v],"uv_size":[16,border_sample]}
                    }
                }));
            }
        }
    } else {
        let origin = motive.atlas_origin.unwrap_or([0, 0]);
        texture_size = if motive.atlas_origin.is_some() {
            [PAINTING_ATLAS_SIDE; 2]
        } else {
            motive.size.map(u16::from)
        };
        cubes.push(
            json!({"origin":[-width/2.0,-height/2.0,-half_depth],"size":[width,height,0],
            "uv":{"north":{"uv":origin,"uv_size":[width,height]}}}),
        );
    }
    json!({"format_version":"1.12.0", "minecraft:geometry":[{
        "description":{"identifier":identifier,"texture_width":texture_size[0],"texture_height":texture_size[1],
            "visible_bounds_width":width/16.0,"visible_bounds_height":height/16.0,"visible_bounds_offset":[0,0,0]},
        "bones":[{"name":"root","pivot":[0,0,0],"cubes":cubes}]
    }]})
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};
    use std::collections::BTreeSet;

    /// Encodes original artwork with a distinct colour for every logical atlas region.
    fn png(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(width, height, image::Rgba(color))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

    #[test]
    fn texture_only_pack_compiles_legacy_and_individual_painting_artwork() {
        let compiled = crate::compile_actor_pack(vec![
            (
                "textures/painting/kz.png".into(),
                png(512, 512, [40, 80, 120, 255]),
            ),
            (
                "textures/painting/meditative.png".into(),
                png(32, 32, [150, 30, 10, 255]),
            ),
        ])
        .unwrap()
        .expect("painting pack");
        let runtime = assets::RuntimeEntityAssets::from_compiled(compiled.entities).unwrap();
        for (title, border, texture) in [
            ("Kebab", false, "textures/painting/kz.png"),
            ("Kebab", true, "textures/painting/kz.png"),
            ("meditative", false, "textures/painting/meditative.png"),
        ] {
            let identifier = assets::painting::motive(title).part_identifier(border);
            let symbol = runtime
                .symbols()
                .iter()
                .position(|symbol| symbol.identifier.as_ref() == identifier)
                .expect("painting part binding");
            let rig = runtime
                .rig_bindings()
                .iter()
                .position(|rig| rig.entity_symbol as usize == symbol)
                .unwrap();
            let binding = compiled
                .bindings
                .iter()
                .find(|binding| binding.rig as usize == rig)
                .expect("painting part has decoded artwork");
            let raster = &compiled.textures[binding.texture as usize];
            assert_eq!(
                runtime.sources()[raster.source as usize].path.as_ref(),
                texture
            );
        }
        assert_eq!(compiled.skipped.unparsable, 0);
    }

    /// Supplies accepted authored documents and an atlas with the requested source count.
    fn capacity_pack(source_count: usize) -> Vec<(Box<str>, Vec<u8>)> {
        let mut files: Vec<(Box<str>, Vec<u8>)> = (0..source_count - 2)
            .map(|index| {
                (
                    format!("animations/empty_{index}.json").into(),
                    br#"{"format_version":"1.8.0","animations":{}}"#.to_vec(),
                )
            })
            .collect();
        files.push((
            "entity/fixture.json".into(),
            br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"fixture:actor"}}}"#
                .to_vec(),
        ));
        files.push((
            format!("{}.png", assets::painting::PAINTING_ATLAS).into(),
            png(256, 256, [30, 90, 150, 255]),
        ));
        files
    }

    #[test]
    fn painting_completion_preserves_authored_sources_near_capacity() {
        for remaining_slots in 0..=3 {
            let source_count = super::super::MAX_PACK_ENTITY_SOURCES - remaining_slots;
            let files = capacity_pack(source_count);
            let authored: BTreeSet<_> = files.iter().map(|(path, _)| path.clone()).collect();
            let compiled = crate::compile_entity_pack(files).unwrap().unwrap();
            assert_eq!(
                compiled
                    .assets
                    .sources
                    .iter()
                    .filter(|source| authored.contains(&source.path))
                    .count(),
                source_count,
                "every authored document and raster survives completion"
            );
            assert!(
                compiled
                    .assets
                    .symbols
                    .iter()
                    .any(|symbol| symbol.identifier.as_ref() == "fixture:actor")
            );
            assert_eq!(compiled.skipped, super::super::EntityPackSkips::default());
            let policy_count = super::super::policy_sources::records()
                .filter(|record| {
                    compiled.assets.sources.iter().any(|source| {
                        source.path == record.path
                            && source.source_bytes == record.source_bytes
                            && source.source_sha256 == record.source_sha256
                    })
                })
                .count();
            assert_eq!(policy_count, super::super::policy_sources::SOURCE_COUNT);
            let generated_count = compiled.assets.sources.len() - source_count - policy_count;
            assert_eq!(generated_count, if remaining_slots == 3 { 3 } else { 0 });
            assert!(compiled.assets.sources.len() <= assets::MAX_ENTITY_ASSET_SOURCES);
            assets::RuntimeEntityAssets::from_compiled(compiled.assets)
                .expect("authored content and generated paintings remain admissible");
        }
    }

    #[test]
    fn painting_completion_preserves_large_authored_geometry_catalogs() {
        let count = 4_097;
        let geometries = (0..count).map(|index| serde_json::json!({
            "description":{"identifier":format!("geometry.fixture.{index}"),"texture_width":16,"texture_height":16},
            "bones":[{"name":"root","cubes":[{"origin":[0,0,0],"size":[16,16,16],"uv":[0,0]}]}]
        })).collect::<Vec<_>>();
        let geometry = serde_json::to_vec(
            &serde_json::json!({"format_version":"1.12.0","minecraft:geometry":geometries}),
        )
        .unwrap();
        let entity = br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"fixture:actor","geometry":{"default":"geometry.fixture.0"}}}}"#;
        let compiled = crate::compile_entity_pack(vec![
            ("models/entity/fixture.json".into(), geometry),
            ("entity/fixture.json".into(), entity.to_vec()),
            (
                format!("{}.png", assets::painting::PAINTING_ATLAS).into(),
                png(256, 256, [30, 90, 150, 255]),
            ),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(compiled.skipped, super::super::EntityPackSkips::default());
        assert_eq!(
            compiled
                .assets
                .geometries
                .iter()
                .filter(|geometry| geometry.identifier.starts_with("geometry.fixture."))
                .count(),
            count
        );
        assert!(compiled.assets.geometries.len() > count);
        assert!(
            compiled
                .assets
                .symbols
                .iter()
                .any(|symbol| symbol.identifier.as_ref() == "fixture:actor")
        );
        assets::RuntimeEntityAssets::from_compiled(compiled.assets)
            .expect("full authored geometry remains admissible");
    }

    #[test]
    fn painting_completion_preserves_authored_symbols_at_catalog_capacity() {
        let controllers = (0..assets::MAX_ENTITY_ASSET_SYMBOLS - 2)
            .map(|index| {
                (
                    format!("controller.render.fixture.{index}"),
                    serde_json::json!({}),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        let controllers = serde_json::to_vec(
            &serde_json::json!({"format_version":"1.8.0","render_controllers":controllers}),
        )
        .unwrap();
        let entity = br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"fixture:actor"}}}"#;
        let compiled = crate::compile_entity_pack(vec![
            ("render_controllers/fixture.json".into(), controllers),
            ("entity/fixture.json".into(), entity.to_vec()),
            (
                format!("{}.png", assets::painting::PAINTING_ATLAS).into(),
                png(256, 256, [30, 90, 150, 255]),
            ),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(compiled.skipped, super::super::EntityPackSkips::default());
        assert_eq!(
            compiled.assets.symbols.len(),
            assets::MAX_ENTITY_ASSET_SYMBOLS
        );
        assert!(
            compiled
                .assets
                .symbols
                .iter()
                .any(|symbol| symbol.identifier.as_ref() == "fixture:actor")
        );
        assert_eq!(
            compiled
                .assets
                .symbols
                .iter()
                .filter(|symbol| symbol.identifier.starts_with("controller.render.fixture."))
                .count(),
            assets::MAX_ENTITY_ASSET_SYMBOLS - 2
        );
        assets::RuntimeEntityAssets::from_compiled(compiled.assets)
            .expect("full authored symbol inventory remains admissible");
    }

    #[test]
    fn painting_completion_preserves_authored_files_at_generated_source_paths() {
        for (path, identifier, authored) in [
            (
                "entity/_cinnabar/painting_Kebab_front.json",
                "fixture:actor",
                serde_json::json!({"format_version":"1.10.0", "minecraft:client_entity":{"description":{"identifier":"fixture:actor"}}}),
            ),
            (
                "models/entity/_cinnabar/painting_Kebab_front.json",
                "geometry.fixture",
                serde_json::json!({"format_version":"1.12.0", "minecraft:geometry":[{"description":{"identifier":"geometry.fixture"},"bones":[{"name":"root"}]}]}),
            ),
            (
                "render_controllers/_cinnabar/paintings.json",
                "controller.render.fixture",
                serde_json::json!({"format_version":"1.8.0", "render_controllers":{"controller.render.fixture":{}}}),
            ),
        ] {
            let authored = serde_json::to_vec(&authored).unwrap();
            let expected_hash: [u8; 32] = Sha256::digest(&authored).into();
            let compiled = crate::compile_actor_pack(vec![
                (path.into(), authored),
                (
                    format!("{}.png", assets::painting::PAINTING_ATLAS).into(),
                    png(256, 256, [30, 90, 150, 255]),
                ),
            ])
            .unwrap()
            .unwrap();
            assert_eq!(compiled.skipped, super::super::EntityPackSkips::default());
            let source = compiled
                .entities
                .sources
                .iter()
                .position(|source| source.path.as_ref() == path)
                .unwrap();
            assert_eq!(
                compiled.entities.sources[source].source_sha256,
                expected_hash
            );
            assert!(
                compiled
                    .entities
                    .symbols
                    .iter()
                    .any(|symbol| symbol.identifier.as_ref() == identifier
                        && symbol.source_index as usize == source)
            );
            let runtime = assets::RuntimeEntityAssets::from_compiled(compiled.entities)
                .expect("authored files and generated parts remain admissible together");
            let painting = assets::painting::motive("Kebab").part_identifier(false);
            assert!(
                runtime
                    .symbols()
                    .iter()
                    .any(|symbol| symbol.identifier.as_ref() == painting)
            );
        }
    }
}
