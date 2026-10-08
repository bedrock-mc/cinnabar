//! Legacy vanilla icon routes: the atlas key and variant vanilla 26.30
//! assigns to items without an icon component, with potion icons keyed by aux.
//! Keys absent from the pinned atlas are skipped, never invented.

use assets::AssetError;

use super::invalid;

pub(super) const SOURCE_PATH: &str = "registry/legacy-icon-routes-26.30.tsv";
pub(super) const SOURCE_BYTES: &[u8] =
    include_bytes!("../../../assets/data/legacy-icon-routes-26.30.tsv");

pub(super) struct LegacyIcon<'a> {
    pub(super) identifier: &'a str,
    pub(super) metadata: u32,
    pub(super) atlas_key: &'a str,
    pub(super) variant: usize,
}

/// Load the checked-in item and atlas routes.
pub(super) fn reviewed() -> Result<Vec<LegacyIcon<'static>>, AssetError> {
    parse(SOURCE_BYTES)
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
}
