//! Legacy vanilla icon routes: the atlas key and variant the retail client's
//! `VanillaItems::initClientData` assigns to items without an icon component,
//! with potion icons keyed by aux. Each row cites its call site in the 26.30
//! client; keys absent from the pinned atlas are skipped, never invented.

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

pub(super) fn reviewed() -> Result<Vec<LegacyIcon<'static>>, AssetError> {
    parse(SOURCE_BYTES)
}

fn parse(bytes: &[u8]) -> Result<Vec<LegacyIcon<'_>>, AssetError> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| invalid("legacy icon routes are not UTF-8"))?;
    let mut routes: Vec<LegacyIcon<'_>> = Vec::new();
    for line in text.lines() {
        let [identifier, metadata, atlas_key, variant, evidence] = line
            .split('\t')
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| invalid("legacy icon route needs five columns"))?;
        let key_ok = !atlas_key.is_empty()
            && atlas_key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
        let (Ok(metadata), Ok(variant)) = (metadata.parse::<u32>(), variant.parse::<usize>())
        else {
            return Err(invalid("legacy icon route has a non-numeric field"));
        };
        if !identifier.starts_with("minecraft:") || !key_ok || !evidence.starts_with("0x") {
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
    fn unsorted_or_malformed_rows_fail_closed() {
        assert!(parse(b"minecraft:b\t0\tb\t0\t0x1\nminecraft:a\t0\ta\t0\t0x2").is_err());
        assert!(parse(b"minecraft:a\t0\t../a\t0\t0x1").is_err());
        assert!(parse(b"minecraft:a\t0\ta\t0").is_err());
    }
}
