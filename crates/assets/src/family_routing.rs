//! Admission compatibility for the shared block-family routing catalog.

use std::sync::OnceLock;

use serde::Deserialize;

use crate::{AssetError, ModelFamily};

const ROUTING: &[u8] = include_bytes!("../data/block-family-routing.json");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    schema: u32,
    crossed_planes: Box<[Box<str>]>,
}

/// Rejects an unsupported catalog or noncanonical, unordered identifiers.
fn parse(bytes: &[u8]) -> Result<Catalog, String> {
    let catalog: Catalog = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    if catalog.schema != 1
        || catalog.crossed_planes.is_empty()
        || !catalog
            .crossed_planes
            .windows(2)
            .all(|pair| pair[0] < pair[1])
        || catalog.crossed_planes.iter().any(|name| {
            !name.starts_with("minecraft:")
                || name.len() > 256
                || name.bytes().any(|byte| byte.is_ascii_control())
        })
    {
        return Err("invalid block-family routing catalog".into());
    }
    Ok(catalog)
}

/// Keeps historical carriers readable and requires current carriers to contain their routes.
pub(crate) fn resolve(
    historical: bool,
    name: &str,
    family: ModelFamily,
) -> Result<ModelFamily, AssetError> {
    static CATALOG: OnceLock<Result<Catalog, String>> = OnceLock::new();
    let catalog = CATALOG
        .get_or_init(|| parse(ROUTING))
        .as_ref()
        .map_err(|detail| AssetError::InvalidCompiledAssets {
            detail: detail.clone().into(),
        })?;
    if catalog
        .crossed_planes
        .binary_search_by(|candidate| candidate.as_ref().cmp(name))
        .is_err()
    {
        return Ok(family);
    }
    if historical || family == ModelFamily::Cross {
        Ok(ModelFamily::Cross)
    } else {
        Err(AssetError::InvalidCompiledAssets {
            detail: format!("block-family route for {name} is stale; rebuild with make assets")
                .into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_routes_are_required_while_historical_routes_are_normalized() {
        let catalog = parse(ROUTING).unwrap();
        for name in catalog.crossed_planes.iter() {
            assert!(resolve(false, name, ModelFamily::Cube).is_err());
            assert_eq!(
                resolve(false, name, ModelFamily::Cross).unwrap(),
                ModelFamily::Cross
            );
            assert_eq!(
                resolve(true, name, ModelFamily::Cube).unwrap(),
                ModelFamily::Cross
            );
        }
        assert_eq!(
            resolve(false, "custom:red_tulip", ModelFamily::Cube).unwrap(),
            ModelFamily::Cube
        );
    }

    #[test]
    fn catalogs_reject_duplicate_routes_and_unknown_schema() {
        for bytes in [
            br#"{"schema":1,"crossed_planes":["minecraft:plant","minecraft:plant"]}"#.as_slice(),
            br#"{"schema":2,"crossed_planes":["minecraft:plant"]}"#.as_slice(),
        ] {
            assert!(parse(bytes).is_err());
        }
    }
}
