use std::{error::Error, fmt, path::Path};

use assets::{
    LightProperties, RegistryRecord, read_light_registry, read_light_registry_for_protocol,
    read_registry, read_registry_for_protocol,
};

/// Decoded block records paired with their header-declared wire protocol.
pub(super) type BlockRegistryInput = (Box<[RegistryRecord]>, u32);

const BLOCK_REGISTRY_MAGIC: &[u8; 8] = b"BREG1003";
const LIGHT_REGISTRY_MAGIC: &[u8; 8] = b"LREG1001";
/// The protocol assumed only when a header is too malformed to name one; the
/// strict readers then reject it with their canonical legacy decode error.
const LEGACY_REGISTRY_PROTOCOL: u32 = 1001;

/// Typed failure for a registry triple whose headers disagree on their wire
/// protocol or declare an unsupported version. Every variant names the exact
/// input files so a mixed-version handoff is attributable per file.
#[derive(Debug)]
pub(super) enum RegistryVersionError {
    /// The block and light registries declare different wire protocols.
    Mismatch {
        block_registry: std::path::PathBuf,
        block_registry_protocol: u32,
        light_registry: std::path::PathBuf,
        light_registry_protocol: u32,
    },
    /// One registry header declares a protocol this compiler cannot consume.
    UnsupportedProtocol {
        path: std::path::PathBuf,
        protocol: u32,
    },
}

impl fmt::Display for RegistryVersionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mismatch {
                block_registry,
                block_registry_protocol,
                light_registry,
                light_registry_protocol,
            } => write!(
                formatter,
                "mixed-version registry triple: {} declares protocol {block_registry_protocol} while {} declares protocol {light_registry_protocol}",
                block_registry.display(),
                light_registry.display()
            ),
            Self::UnsupportedProtocol { path, protocol } => write!(
                formatter,
                "{} declares unsupported registry wire protocol {protocol}",
                path.display()
            ),
        }
    }
}

impl Error for RegistryVersionError {}

/// Peeks the wire protocol from a decoded registry header without consuming
/// the input. Returns `None` when the header itself is malformed so the
/// strict readers below produce their canonical legacy decode error.
fn decoded_registry_protocol(magic: &[u8; 8], bytes: &[u8]) -> Option<u32> {
    if bytes.len() >= magic.len() + 4 && &bytes[..magic.len()] == magic {
        Some(u32::from_le_bytes(
            bytes[magic.len()..magic.len() + 4]
                .try_into()
                .expect("fixed-width protocol"),
        ))
    } else {
        None
    }
}

fn supported_registry_protocol(path: &Path, protocol: u32) -> Result<u32, RegistryVersionError> {
    match protocol {
        1001 | 2168 => Ok(protocol),
        _ => Err(RegistryVersionError::UnsupportedProtocol {
            path: path.to_path_buf(),
            protocol,
        }),
    }
}

/// Decodes the bounded block registry for the wire protocol its own header
/// declares, returning `(records, declared_protocol)` so the light-registry
/// leg can reject a mixed-version triple before decoding it.
pub(super) fn read_block_registry_input(
    path: &Path,
    bytes: &[u8],
) -> Result<BlockRegistryInput, Box<dyn Error>> {
    match decoded_registry_protocol(BLOCK_REGISTRY_MAGIC, bytes) {
        Some(protocol) => {
            let protocol = supported_registry_protocol(path, protocol)?;
            Ok((read_registry_for_protocol(bytes, protocol)?, protocol))
        }
        // Malformed headers keep today's exact strict-reader failure.
        None => Ok((read_registry(bytes)?, LEGACY_REGISTRY_PROTOCOL)),
    }
}

/// Decodes the bounded light registry bound to the already-decoded block
/// registry, failing closed with a typed per-file error when the two headers
/// declare different wire protocols.
pub(super) fn read_light_registry_input(
    path: &Path,
    bytes: &[u8],
    block_registry: &Path,
    block_registry_bytes: &[u8],
    block_registry_protocol: u32,
    expected_count: usize,
) -> Result<Box<[LightProperties]>, Box<dyn Error>> {
    let Some(light_registry_protocol) = decoded_registry_protocol(LIGHT_REGISTRY_MAGIC, bytes)
    else {
        return Ok(read_light_registry(
            bytes,
            block_registry_bytes,
            expected_count,
        )?);
    };
    let light_registry_protocol = supported_registry_protocol(path, light_registry_protocol)?;
    if light_registry_protocol != block_registry_protocol {
        return Err(Box::new(RegistryVersionError::Mismatch {
            block_registry: block_registry.to_path_buf(),
            block_registry_protocol,
            light_registry: path.to_path_buf(),
            light_registry_protocol,
        }));
    }
    Ok(read_light_registry_for_protocol(
        bytes,
        block_registry_bytes,
        expected_count,
        light_registry_protocol,
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::path::PathBuf;

    struct SyntheticAir {
        sequential_id: u32,
        network_hash: u32,
    }

    /// Encodes one-record BREG1003 bytes carrying a synthetic canonical air.
    fn encode_block_registry(protocol: u32, air: &SyntheticAir) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"BREG1003");
        bytes.extend_from_slice(&protocol.to_le_bytes());
        // One unique name and state, fully provenanced outside Valentine, so
        // no Valentine overlap bookkeeping is required.
        for value in [1_u32, 1, 0, 0, 1, 1] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&air.sequential_id.to_le_bytes());
        bytes.extend_from_slice(&air.network_hash.to_le_bytes());
        bytes.push(assets::BlockFlags::AIR.bits()); // flags
        bytes.push(assets::ModelFamily::Air as u8);
        bytes.push(assets::ContributorRole::Air as u8);
        bytes.push(0); // model-state mask
        bytes.push(0); // face coverage
        bytes.push(0); // collision confidence: none
        bytes.push(assets::RegistryProvenance::PMMP.bits());
        bytes.push(0); // collision box count
        bytes.extend_from_slice(&0_u16.to_le_bytes()); // shape id
        bytes.extend_from_slice(&("minecraft:air".len() as u16).to_le_bytes());
        bytes.extend_from_slice(&("{}".len() as u32).to_le_bytes()); // state length
        for _ in 0..8 {
            bytes.extend_from_slice(&0_u32.to_le_bytes());
        }
        bytes.extend_from_slice(b"minecraft:air");
        bytes.extend_from_slice(b"{}");
        bytes
    }

    /// Encodes LREG1001 bytes binding one dark nibble per BREG record.
    fn encode_light_registry(protocol: u32, block_registry: &[u8], records: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"LREG1001");
        bytes.extend_from_slice(&protocol.to_le_bytes());
        bytes.extend_from_slice(&(records as u32).to_le_bytes());
        bytes.extend_from_slice(&Sha256::digest(block_registry));
        bytes.extend(std::iter::repeat_n(0_u8, records));
        let digest = Sha256::digest(&bytes);
        bytes.extend_from_slice(&digest);
        bytes
    }

    #[test]
    fn a_v2168_triple_decodes_through_its_header_derived_protocol() {
        let block_registry = encode_block_registry(
            2168,
            &SyntheticAir {
                sequential_id: 13_629,
                network_hash: 0x2d65_8dd8,
            },
        );
        let light_registry = encode_light_registry(2168, &block_registry, 1);

        let (records, protocol) =
            read_block_registry_input(Path::new("block.bin"), &block_registry)
                .expect("decode v2168 block registry");
        assert_eq!(protocol, 2168);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].sequential_id, 13_629);

        let lights = read_light_registry_input(
            Path::new("light.bin"),
            &light_registry,
            Path::new("block.bin"),
            &block_registry,
            protocol,
            records.len(),
        )
        .expect("decode v2168 light registry");
        assert_eq!(lights.len(), 1);
    }

    #[test]
    fn a_v1001_triple_still_decodes_through_the_legacy_path() {
        let block_registry = encode_block_registry(
            1001,
            &SyntheticAir {
                sequential_id: 13_094,
                network_hash: 0xdbf4_4120,
            },
        );
        let light_registry = encode_light_registry(1001, &block_registry, 1);

        let (records, protocol) =
            read_block_registry_input(Path::new("block.bin"), &block_registry)
                .expect("decode v1001 block registry");
        assert_eq!(protocol, 1001);
        let lights = read_light_registry_input(
            Path::new("light.bin"),
            &light_registry,
            Path::new("block.bin"),
            &block_registry,
            protocol,
            records.len(),
        )
        .expect("decode v1001 light registry");
        assert_eq!(lights.len(), 1);
    }

    #[test]
    fn a_mixed_version_triple_fails_closed_naming_both_files() {
        let block_registry = encode_block_registry(
            2168,
            &SyntheticAir {
                sequential_id: 13_629,
                network_hash: 0x2d65_8dd8,
            },
        );
        let light_registry = encode_light_registry(1001, &block_registry, 1);

        let (records, protocol) =
            read_block_registry_input(Path::new("block.bin"), &block_registry)
                .expect("block leg decodes");
        assert_eq!(protocol, 2168);
        let error = read_light_registry_input(
            Path::new("light.bin"),
            &light_registry,
            Path::new("block.bin"),
            &block_registry,
            protocol,
            records.len(),
        )
        .expect_err("mixed-version triple must fail closed");

        let version_error = error
            .downcast_ref::<RegistryVersionError>()
            .expect("typed version error");
        assert!(matches!(
            version_error,
            RegistryVersionError::Mismatch {
                block_registry_protocol: 2168,
                light_registry_protocol: 1001,
                ..
            }
        ));
        let message = version_error.to_string();
        assert!(message.contains("block.bin"), "{message}");
        assert!(message.contains("light.bin"), "{message}");
    }

    #[test]
    fn an_unsupported_header_protocol_fails_closed_naming_the_file() {
        let block_registry = encode_block_registry(
            9999,
            &SyntheticAir {
                sequential_id: 1,
                network_hash: 2,
            },
        );

        let error = read_block_registry_input(Path::new("future.bin"), &block_registry)
            .expect_err("unsupported protocol must fail closed");
        let version_error = error
            .downcast_ref::<RegistryVersionError>()
            .expect("typed version error");
        assert!(matches!(
            version_error,
            RegistryVersionError::UnsupportedProtocol { protocol: 9999, .. }
        ));
        assert!(version_error.to_string().contains("future.bin"));
    }

    #[test]
    fn a_malformed_header_defers_to_the_strict_legacy_decode_error() {
        use assets::AssetError;

        let error = read_block_registry_input(Path::new("garbage.bin"), b"not-a-registry-at-all")
            .expect_err("malformed header must fail closed");
        assert!(error.is::<AssetError>());
        assert!(!error.is::<RegistryVersionError>());
    }

    #[test]
    fn version_errors_display_without_placeholder_paths() {
        let mismatch = RegistryVersionError::Mismatch {
            block_registry: PathBuf::from("a.bin"),
            block_registry_protocol: 2168,
            light_registry: PathBuf::from("b.bin"),
            light_registry_protocol: 1001,
        };
        let unsupported = RegistryVersionError::UnsupportedProtocol {
            path: PathBuf::from("c.bin"),
            protocol: 7,
        };
        assert_eq!(
            mismatch.to_string(),
            "mixed-version registry triple: a.bin declares protocol 2168 while b.bin declares protocol 1001"
        );
        assert_eq!(
            unsupported.to_string(),
            "c.bin declares unsupported registry wire protocol 7"
        );
    }
}
