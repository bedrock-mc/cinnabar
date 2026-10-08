use std::sync::OnceLock;

const TABLE: &str = include_str!("../../data/server-defined-blocks-v2193.tsv");

/// Canonical states admitted to a remote palette only by server block definitions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServerDefinedBlock {
    pub name: &'static str,
    pub first_internal_id: u32,
    pub state_count: u32,
}

struct Metadata {
    registry_sha256: [u8; 32],
    blocks: Box<[ServerDefinedBlock]>,
}

fn metadata() -> &'static Metadata {
    static METADATA: OnceLock<Metadata> = OnceLock::new();
    METADATA.get_or_init(|| {
        let hash = TABLE
            .lines()
            .find_map(|line| line.strip_prefix("# registry_sha256="))
            .expect("server-defined block table registry binding");
        assert_eq!(hash.len(), 64, "server-defined block registry hash length");
        let registry_sha256 = std::array::from_fn(|index| {
            u8::from_str_radix(&hash[index * 2..index * 2 + 2], 16)
                .expect("server-defined block registry hash")
        });
        let mut end = 0;
        let blocks = TABLE
            .lines()
            .filter(|line| !line.starts_with('#') && !line.is_empty())
            .map(|line| {
                let mut columns = line.split('\t');
                let name = columns.next().expect("server-defined block name");
                let first_internal_id = columns
                    .next()
                    .expect("server-defined block first id")
                    .parse::<u32>()
                    .expect("server-defined block numeric first id");
                let state_count = columns
                    .next()
                    .expect("server-defined block state count")
                    .parse::<u32>()
                    .expect("server-defined block numeric state count");
                assert!(columns.next().is_none());
                assert!(state_count > 0 && first_internal_id >= end);
                end = first_internal_id
                    .checked_add(state_count)
                    .expect("server-defined block range");
                ServerDefinedBlock {
                    name,
                    first_internal_id,
                    state_count,
                }
            })
            .collect();
        Metadata {
            registry_sha256,
            blocks,
        }
    })
}

/// Definition ranges for the table's pinned canonical registry.
#[must_use]
pub fn server_defined_blocks() -> &'static [ServerDefinedBlock] {
    &metadata().blocks
}

/// Returns definition ranges only when their canonical registry binding matches.
#[must_use]
pub fn server_defined_blocks_for_registry(
    registry_sha256: [u8; 32],
) -> Option<&'static [ServerDefinedBlock]> {
    let metadata = metadata();
    (metadata.registry_sha256 == registry_sha256).then_some(metadata.blocks.as_ref())
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, path::Path};

    use sha2::{Digest, Sha256};

    use super::{server_defined_blocks, server_defined_blocks_for_registry};
    use crate::{
        SequentialIdRemap, active_content_registry_protocol, pinned_block_registry_bytes,
        read_registry_for_protocol, vanilla_source,
    };

    #[test]
    fn dense_palette_metadata_is_bound_to_exact_canonical_ranges() {
        let bytes = pinned_block_registry_bytes();
        let records =
            read_registry_for_protocol(bytes, active_content_registry_protocol()).unwrap();
        let hash: [u8; 32] = Sha256::digest(bytes).into();
        let blocks = server_defined_blocks_for_registry(hash).expect("matching registry binding");
        assert_eq!(blocks, server_defined_blocks());
        assert!(server_defined_blocks_for_registry([0; 32]).is_none());
        let mut names = BTreeSet::new();
        let mut end = 0;
        for block in blocks {
            assert!(names.insert(block.name));
            assert!(block.first_internal_id >= end);
            end = block.first_internal_id + block.state_count;
            let range = &records[block.first_internal_id as usize..end as usize];
            assert!(range.iter().all(|record| {
                record.name.as_ref() == block.name || record.name.as_ref() == "cinnabar:reserved"
            }));
            let named = records
                .iter()
                .filter(|record| record.name.as_ref() == block.name)
                .collect::<Vec<_>>();
            if !named.is_empty() {
                assert_eq!(named.len(), block.state_count as usize);
                assert_eq!(named[0].sequential_id, block.first_internal_id);
            }
        }
    }

    #[test]
    fn dense_palette_metadata_matches_pinned_behavior_definitions() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(vanilla_source().cache_dir.as_ref())
            .join("behavior_pack/blocks");
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                eprintln!(
                    "missing fixture: pinned behavior definitions {}",
                    directory.display()
                );
                return;
            }
            Err(error) => panic!("read pinned behavior definitions: {error}"),
        };
        let mut names = BTreeSet::new();
        for entry in entries {
            let path = entry.unwrap().path();
            if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                let definition: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
                names.insert(
                    definition["minecraft:block"]["description"]["identifier"]
                        .as_str()
                        .unwrap()
                        .to_owned(),
                );
            }
        }
        assert_eq!(
            names,
            server_defined_blocks()
                .iter()
                .map(|block| block.name.to_owned())
                .collect()
        );
    }

    #[test]
    fn dense_palette_omission_preserves_captured_remote_block_identities() {
        let records = read_registry_for_protocol(
            pinned_block_registry_bytes(),
            active_content_registry_protocol(),
        )
        .unwrap();
        let blocks = server_defined_blocks();
        let mapping = records
            .iter()
            .filter(|record| {
                !blocks.iter().any(|block| {
                    (block.first_internal_id..block.first_internal_id + block.state_count)
                        .contains(&record.sequential_id)
                })
            })
            .map(|record| record.sequential_id)
            .collect::<Vec<_>>();
        let wire_count = mapping.len() as u32;
        let remap = SequentialIdRemap::from_palette(mapping, records.len() as u32);
        for (wire, name) in [
            (15_844, "minecraft:air"),
            (3_219, "minecraft:stone"),
            (9_650, "minecraft:polished_blackstone_bricks"),
            (19_595, "minecraft:brown_terracotta"),
            (17_951, "minecraft:gray_concrete"),
        ] {
            let internal = remap.to_internal(wire);
            assert_eq!(records[internal as usize].name.as_ref(), name);
            assert_eq!(remap.to_wire(internal), wire);
        }
        assert_eq!(remap.to_internal(wire_count), u32::MAX);
        for block in blocks {
            for internal in block.first_internal_id..block.first_internal_id + block.state_count {
                assert_eq!(remap.to_wire(internal), u32::MAX);
            }
        }
    }
}
