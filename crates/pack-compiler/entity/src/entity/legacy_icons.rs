//! Legacy vanilla icon routes: the atlas key and variant the client assigns to items without
//! an icon component, with potion and tipped-arrow icons keyed by aux. Keys absent from the
//! pinned atlas are skipped, never invented.

use assets::AssetError;

use super::invalid;

pub(super) const SOURCE_PATH: &str = "registry/legacy-icon-routes-26.30.tsv";
pub(super) const SOURCE_BYTES: &[u8] =
    include_bytes!("../../../../assets/data/legacy-icon-routes-26.30.tsv");

pub(super) struct LegacyIcon<'a> {
    pub(super) identifier: &'a str,
    pub(super) metadata: u32,
    pub(super) atlas_key: &'a str,
    pub(super) variant: usize,
}

/// Load the checked-in item and atlas routes plus the tipped arrows, sorted by item key.
pub(super) fn reviewed() -> Result<Vec<LegacyIcon<'static>>, AssetError> {
    let mut routes = parse(SOURCE_BYTES)?;
    routes.extend(
        (1..=assets::potion::MAX_TIPPED_ARROW_AUXILIARY).filter_map(|metadata| {
            Some(LegacyIcon {
                identifier: assets::facts::ARROW_IDENTIFIER,
                metadata,
                atlas_key: assets::facts::TIPPED_ARROW_ATLAS_KEY,
                variant: usize::from(assets::potion::tipped_arrow_icon_variant(metadata)?),
            })
        }),
    );
    routes.sort_by(|left, right| {
        (left.identifier, left.metadata).cmp(&(right.identifier, right.metadata))
    });
    if routes.windows(2).any(|pair| {
        (pair[0].identifier, pair[0].metadata) == (pair[1].identifier, pair[1].metadata)
    }) {
        return Err(invalid(
            "tipped arrow icon routes duplicate a reviewed route",
        ));
    }
    Ok(routes)
}

/// Reject malformed rows and duplicate or unordered item keys.
fn parse(bytes: &[u8]) -> Result<Vec<LegacyIcon<'_>>, AssetError> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| invalid("legacy icon routes are not UTF-8"))?;
    let mut routes: Vec<LegacyIcon<'_>> = Vec::new();
    for line in text.lines() {
        let [identifier, metadata, atlas_key, variant] = line
            .split('\t')
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| invalid("legacy icon route needs four columns"))?;
        let key_ok = !atlas_key.is_empty()
            && atlas_key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
        let (Ok(metadata), Ok(variant)) = (metadata.parse::<u32>(), variant.parse::<usize>())
        else {
            return Err(invalid("legacy icon route has a non-numeric field"));
        };
        if !identifier.starts_with("minecraft:") || !key_ok {
            return Err(invalid("legacy icon route is noncanonical"));
        }
        if routes
            .last()
            .is_some_and(|last| (last.identifier, last.metadata) >= (identifier, metadata))
        {
            return Err(invalid("legacy icon routes are not strictly sorted"));
        }
        routes.push(LegacyIcon {
            identifier,
            metadata,
            atlas_key,
            variant,
        });
    }
    Ok(routes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reviewed_routes_carry_the_retail_combat_icons() {
        let routes = reviewed().unwrap();
        let find = |identifier: &str, metadata: u32| {
            routes
                .iter()
                .find(|route| route.identifier == identifier && route.metadata == metadata)
                .map(|route| (route.atlas_key, route.variant))
        };
        assert_eq!(find("minecraft:diamond_sword", 0), Some(("sword", 4)));
        assert_eq!(find("minecraft:compass", 0), Some(("compass_item", 0)));
        assert_eq!(find("minecraft:diamond_helmet", 0), Some(("helmet", 4)));
        assert_eq!(find("minecraft:golden_apple", 0), Some(("apple_golden", 0)));
    }

    #[test]
    fn four_column_rows_preserve_item_metadata_and_atlas_variants() {
        let routes = parse(b"minecraft:a\t7\ticon_key\t3").unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].identifier, "minecraft:a");
        assert_eq!(routes[0].metadata, 7);
        assert_eq!(routes[0].atlas_key, "icon_key");
        assert_eq!(routes[0].variant, 3);
    }

    #[test]
    fn unsorted_or_malformed_rows_fail_closed() {
        for bytes in [
            b"minecraft:b\t0\tb\t0\nminecraft:a\t0\ta\t0".as_slice(),
            b"minecraft:a\t0\ta\t0\nminecraft:a\t0\ta\t0",
            b"minecraft:a\t0\t../a\t0",
            b"minecraft:a\t0\ta",
            b"minecraft:a\t0\ta\t0\textra",
            b"minecraft:a\tnan\ta\t0",
            b"minecraft:a\t0\ta\t-1",
        ] {
            assert!(parse(bytes).is_err());
        }
    }

    fn pinned_atlas() -> Option<serde_json::Map<String, serde_json::Value>> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join(assets::vanilla_source().resource_pack_dir())
            .join("textures/item_texture.json");
        let Ok(bytes) = std::fs::read(&path) else {
            eprintln!(
                "skipping missing fixture: pinned item atlas at {}",
                path.display()
            );
            return None;
        };
        let atlas = pack_compile_input::json::parse_semantic_json(&path, &bytes).unwrap();
        Some(atlas["texture_data"].as_object().unwrap().clone())
    }

    fn atlas_variants(
        atlas: &serde_json::Map<String, serde_json::Value>,
        key: &str,
    ) -> Vec<String> {
        match &atlas[key]["textures"] {
            serde_json::Value::String(path) => vec![path.clone()],
            serde_json::Value::Array(paths) => paths
                .iter()
                .map(|path| path.as_str().unwrap().to_owned())
                .collect(),
            other => panic!("{key}: {other}"),
        }
    }

    /// Rows naming a key the pinned atlas lacks are skipped at compile time; only these may.
    #[test]
    fn reviewed_rows_resolve_in_the_pinned_atlas() {
        let Some(atlas) = pinned_atlas() else {
            return;
        };
        let dead = reviewed()
            .unwrap()
            .into_iter()
            .filter(|route| {
                !atlas.contains_key(route.atlas_key)
                    || route.variant >= atlas_variants(&atlas, route.atlas_key).len()
            })
            .map(|route| route.identifier)
            .collect::<Vec<_>>();
        // Education Edition items have no art in the pinned pack, `mangrove_propagule` keeps
        // its block route and `recovery_compass` draws its dial frames.
        assert_eq!(
            dead,
            [
                "minecraft:balloon",
                "minecraft:bleach",
                "minecraft:compound",
                "minecraft:ice_bomb",
                "minecraft:mangrove_propagule",
                "minecraft:medicine",
                "minecraft:photo_item",
                "minecraft:rapid_fertilizer",
                "minecraft:recovery_compass",
                "minecraft:sparkler",
            ]
        );
    }

    /// An atlas texture named exactly after a registry item marks a legacy icon the table must
    /// carry; this catches new items in a pack update.
    #[test]
    fn same_named_atlas_textures_have_reviewed_rows() {
        let Some(atlas) = pinned_atlas() else {
            return;
        };
        let routed = reviewed()
            .unwrap()
            .into_iter()
            .map(|route| route.identifier.to_owned())
            .chain(
                super::super::item_bindings::reviewed()
                    .unwrap()
                    .iter()
                    .map(|binding| binding.identifier.to_string()),
            )
            .collect::<std::collections::BTreeSet<_>>();
        let mut stems = std::collections::BTreeMap::<String, Vec<(String, usize)>>::new();
        for key in atlas.keys() {
            for (variant, path) in atlas_variants(&atlas, key).into_iter().enumerate() {
                let stem = path.rsplit('/').next().unwrap().to_owned();
                stems.entry(stem).or_default().push((key.clone(), variant));
            }
        }
        let unrouted = assets::item::names::identifiers()
            .filter(|identifier| !routed.contains(*identifier))
            .filter_map(|identifier| {
                let name = identifier.strip_prefix("minecraft:")?;
                let [(key, variant)] = stems.get(name)?.as_slice() else {
                    return None;
                };
                (key != name).then(|| format!("{identifier} -> {key}[{variant}]"))
            })
            .collect::<Vec<_>>();
        assert!(
            unrouted.is_empty(),
            "missing legacy icon rows: {unrouted:?}"
        );
    }
}
