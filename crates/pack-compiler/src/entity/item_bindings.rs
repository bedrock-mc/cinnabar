//! Reviewed default-icon bindings, independent of texture filename spelling.
//!
//! This bounded crosswalk adds canonical inventory keys to the existing atlas
//! routes. It establishes neither auxiliary icon states nor metadata policy.
//! Current Item::initClient reads components.minecraft:icon;
//! the table records native packaged seed components omitted from the sample pack.

use std::collections::BTreeSet;

use assets::AssetError;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::invalid;

pub(super) const SOURCE_PATH: &str = "registry/default-sprite-bindings-1.26.50.json";
pub(super) const SOURCE_BYTES: &[u8] =
    include_bytes!("../../../assets/data/default-sprite-bindings-1.26.50.json");
pub(super) const RETAIL_ITEMS: &[u8] =
    include_bytes!("../../../protocol/data/retail_items_1_26_50.tsv");
const RETAIL_SHA256: &str = "6f186e8f781c611722cd28ece47f643112732a89e18cd9beab9d414243750821";
const ATLAS_SHA256: &str = "b203a6a4daef52efe98a1e7569a1b69ac7c1a08ca824e8556d141be34233107a";
const ROUTE_COUNT: usize = 33;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingTable {
    schema: u32,
    game_version: Box<str>,
    source_tag: Box<str>,
    source_commit: Box<str>,
    source_url: Box<str>,
    archive_sha256: Box<str>,
    atlas_sha256: Box<str>,
    retail_allowlist_sha256: Box<str>,
    coverage: Box<str>,
    native_item_witness: NativeItemWitness,
    routes: Box<[DefaultBinding]>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeItemWitness {
    app_version: Box<str>,
    archive: Box<str>,
    archive_sha256: Box<str>,
    version_relation: Box<str>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DefaultBinding {
    pub(super) identifier: Box<str>,
    pub(super) default_alias: Box<str>,
    pub(super) atlas_variant: u32,
    evidence_file: Box<str>,
    evidence_sha256: Box<str>,
}

pub(super) fn reviewed() -> Result<Box<[DefaultBinding]>, AssetError> {
    parse(SOURCE_BYTES)
}

fn parse(bytes: &[u8]) -> Result<Box<[DefaultBinding]>, AssetError> {
    let table: BindingTable = serde_json::from_slice(bytes).map_err(|source| AssetError::Json {
        path: SOURCE_PATH.into(),
        source,
    })?;
    let retail_hash = format!("{:x}", Sha256::digest(RETAIL_ITEMS));
    // The table is audited against the pinned pack; a pack bump fails closed until re-audited.
    let pinned = assets::vanilla_source();
    if table.schema != 1
        || !pinned.tag.starts_with(&format!("v{}.", table.game_version))
        || table.source_tag.as_ref() != pinned.tag.as_ref()
        || table.source_commit.as_ref() != pinned.commit.as_ref()
        || table.source_url.as_ref()
            != format!(
                "https://github.com/Mojang/bedrock-samples/tree/{}",
                pinned.commit
            )
        || table.archive_sha256.as_ref() != pinned.sha256.as_ref()
        || table.atlas_sha256.as_ref() != ATLAS_SHA256
        || table.retail_allowlist_sha256.as_ref() != RETAIL_SHA256
        || retail_hash != RETAIL_SHA256
        || table.coverage.is_empty()
        || table.native_item_witness.app_version.is_empty()
        || !table.native_item_witness.archive.starts_with("native/")
        || !table
            .native_item_witness
            .archive
            .ends_with("items.brarchive")
        || !sha256_text(&table.native_item_witness.archive_sha256)
        || table.native_item_witness.version_relation.is_empty()
        || table.routes.len() != ROUTE_COUNT
    {
        return Err(invalid(
            "default sprite binding provenance does not match reviewed inputs",
        ));
    }
    let retail = std::str::from_utf8(RETAIL_ITEMS)
        .map_err(|_| invalid("retail item allowlist is not UTF-8"))?
        .lines()
        .filter_map(|line| line.split_once('\t').map(|(_, identifier)| identifier))
        .collect::<BTreeSet<_>>();
    let mut previous: Option<&str> = None;
    for binding in &table.routes {
        let native_entry = format!(
            "{}#{}.json",
            table.native_item_witness.archive,
            binding.identifier.strip_prefix("minecraft:").unwrap_or("")
        );
        if !retail.contains(binding.identifier.as_ref())
            || previous.is_some_and(|value| value >= binding.identifier.as_ref())
            || binding.atlas_variant != 0
            || binding.default_alias.is_empty()
            || binding.default_alias.len() > 256
            || !binding
                .default_alias
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
            || !(binding.evidence_file.starts_with("behavior_pack/items/")
                || binding.evidence_file.as_ref() == native_entry)
            || !binding.evidence_file.ends_with(".json")
            || binding.evidence_file.contains("..")
            || binding.evidence_file.contains('\\')
            || !sha256_text(&binding.evidence_sha256)
        {
            return Err(invalid(
                "default sprite binding is noncanonical, unsupported or unordered",
            ));
        }
        previous = Some(&binding.identifier);
    }
    Ok(table.routes)
}

fn sha256_text(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn altered(change: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
        let mut table = serde_json::from_slice(SOURCE_BYTES).unwrap();
        change(&mut table);
        serde_json::to_vec(&table).unwrap()
    }

    #[test]
    fn reviewed_table_has_exact_coverage_and_rejects_provenance_drift() {
        assert_eq!(reviewed().unwrap().len(), ROUTE_COUNT);
        for field in [
            "source_commit",
            "atlas_sha256",
            "retail_allowlist_sha256",
            "archive_sha256",
        ] {
            assert!(parse(&altered(|table| table[field] = "wrong".into())).is_err());
        }
    }

    #[test]
    fn native_item_component_witness_rejects_invalid_archive_and_entry_provenance() {
        assert!(
            parse(&altered(|table| {
                table["native_item_witness"]["archive_sha256"] = "invalid".into();
            }))
            .is_err()
        );
        assert!(
            parse(&altered(|table| {
                let seed = table["routes"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|row| row["identifier"] == "minecraft:wheat_seeds")
                    .unwrap();
                seed["evidence_file"] = "native/other/items.brarchive#wheat_seeds.json".into();
            }))
            .is_err()
        );
    }

    #[test]
    fn duplicate_nonretail_and_nondefault_bindings_fail_closed() {
        assert!(
            parse(&altered(
                |table| table["routes"][1] = table["routes"][0].clone()
            ))
            .is_err()
        );
        assert!(
            parse(&altered(
                |table| table["routes"][0]["identifier"] = "minecraft:invented".into()
            ))
            .is_err()
        );
        assert!(
            parse(&altered(
                |table| table["routes"][0]["atlas_variant"] = 1.into()
            ))
            .is_err()
        );
        assert!(
            parse(&altered(
                |table| table["routes"][0]["default_alias"] = "../apple".into()
            ))
            .is_err()
        );
        assert!(
            parse(&altered(
                |table| table["routes"][0]["evidence_sha256"] = "invalid".into()
            ))
            .is_err()
        );
    }
}
