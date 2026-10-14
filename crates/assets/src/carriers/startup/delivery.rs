//! Verified delivery of runtime carriers, without paths pretending to be a filesystem.

use crate::carriers::{self as carriers, ACTOR, Carrier, Recipe, required};
use crate::{
    AssetError, PhysicsRegistry, RuntimeActorCatalog, RuntimeAssets, RuntimeAtmosphereAssets,
    RuntimeEntityAssets, RuntimeHudCatalog, RuntimeIconCatalog, RuntimeLangCatalog,
    RuntimeUiAssets, UiAtlasPage, VANILLA_EN_US_LANG_SHA256, VANILLA_SOURCE_MANIFEST,
    active_content_registry_protocol, canonical_source_manifest_sha256,
    pinned_block_registry_bytes, read_physics_registry_for_protocol, read_registry_for_protocol,
};
#[cfg(test)]
use crate::{UiFile, encode_ui_catalog, entity};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path, sync::Arc};

pub const DELIVERY_VERSION: u32 = 1;
pub const MAX_DELIVERY_MANIFEST_BYTES: usize = 16 * 1024;
pub const MAX_DELIVERY_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_DELIVERY_ENTRY_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
#[error("{0}; rebuild with make assets")]
pub struct DeliveryError(String);
/// One whole carrier, identified by the canonical carrier table.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeliveryEntry {
    pub name: String,
    pub bytes: u64,
    pub sha256: [u8; 32],
}
/// An HTTPS delivery index; carrier decoders remain the authority for its contents.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeliveryManifest {
    pub version: u32,
    pub entries: Vec<DeliveryEntry>,
}
/// Startup also needs the actor carrier bound to the entity catalog.
pub fn startup_carriers() -> impl Iterator<Item = &'static Carrier> {
    required().chain(std::iter::once(&ACTOR))
}

impl DeliveryManifest {
    /// Checks the schema, exact startup set, duplicate names and aggregate byte bounds.
    pub fn validate(&self) -> Result<(), DeliveryError> {
        if self.version != DELIVERY_VERSION || self.entries.len() != startup_carriers().count() {
            return Err(DeliveryError(
                "unsupported or incomplete delivery manifest".into(),
            ));
        }
        let mut total = 0_u64;
        let mut recipes = std::collections::HashSet::new();
        for carrier in startup_carriers() {
            if !recipes.insert(carrier.recipe)
                || !matches!(
                    carrier.recipe,
                    Recipe::World
                        | Recipe::ItemFacts
                        | Recipe::Atmosphere
                        | Recipe::Entity
                        | Recipe::Actor
                        | Recipe::Lang
                        | Recipe::Ui
                        | Recipe::Icon
                        | Recipe::BlockPhysics
                        | Recipe::Hud
                )
            {
                return Err(DeliveryError(format!(
                    "unsupported startup carrier {}",
                    carrier.output
                )));
            }
            let mut entries = self
                .entries
                .iter()
                .filter(|entry| entry.name == carrier.name);
            let entry = entries.next().ok_or_else(|| missing(carrier))?;
            if entries.next().is_some()
                || entry.bytes == 0
                || entry.bytes > MAX_DELIVERY_ENTRY_BYTES
            {
                return Err(DeliveryError(format!(
                    "invalid delivery entry for {}",
                    carrier.output
                )));
            }
            total = total
                .checked_add(entry.bytes)
                .filter(|total| *total <= MAX_DELIVERY_BYTES)
                .ok_or_else(|| DeliveryError("delivery exceeds memory limit".into()))?;
        }
        Ok(())
    }

    /// Generates the index from locally compiled carriers; no game bytes enter the index.
    pub fn from_directory(directory: &Path) -> Result<Self, DeliveryError> {
        let entries = startup_carriers()
            .map(|carrier| {
                let path = directory.join(carrier.output);
                let length = std::fs::metadata(&path)
                    .map_err(|_| missing(carrier))?
                    .len();
                if length == 0 || length > MAX_DELIVERY_ENTRY_BYTES {
                    return Err(DeliveryError(format!(
                        "invalid size for {}",
                        carrier.output
                    )));
                }
                let bytes = std::fs::read(&path).map_err(|_| missing(carrier))?;
                Ok(DeliveryEntry {
                    name: carrier.name.into(),
                    bytes: bytes.len() as u64,
                    sha256: Sha256::digest(&bytes).into(),
                })
            })
            .collect::<Result<_, _>>()?;
        let manifest = Self {
            version: DELIVERY_VERSION,
            entries,
        };
        manifest.validate()?;
        Ok(manifest)
    }
}
/// Immutable, digest-checked carrier bytes supplied directly to the existing decoders.
pub struct MemoryCarriers {
    manifest: DeliveryManifest,
    bytes: BTreeMap<String, Arc<[u8]>>,
}

impl MemoryCarriers {
    /// Admits only a complete, bounded delivery index.
    pub fn new(manifest: DeliveryManifest) -> Result<Self, DeliveryError> {
        manifest.validate()?;
        Ok(Self {
            manifest,
            bytes: BTreeMap::new(),
        })
    }

    /// Verifies size and SHA-256 before publishing bytes; failed replacements retain old data.
    pub fn insert(&mut self, name: &str, bytes: Vec<u8>) -> Result<(), DeliveryError> {
        let entry = self
            .manifest
            .entries
            .iter()
            .find(|entry| entry.name == name)
            .ok_or_else(|| DeliveryError("unlisted carrier".into()))?;
        if bytes.len() as u64 != entry.bytes
            || <[u8; 32]>::from(Sha256::digest(&bytes)) != entry.sha256
        {
            return Err(DeliveryError(format!(
                "delivery hash or length mismatch for {name}"
            )));
        }
        self.bytes.insert(name.into(), bytes.into());
        Ok(())
    }

    /// Reads a canonical carrier from memory, with no filesystem or URL interpretation.
    pub fn read(&self, carrier: &Carrier) -> Result<&[u8], DeliveryError> {
        self.bytes
            .get(carrier.name)
            .map(AsRef::as_ref)
            .ok_or_else(|| missing(carrier))
    }

    /// Decodes and pins every startup carrier, including the entity-linked actor artwork.
    pub fn decode_startup(&self) -> Result<StartupCarriers, DeliveryError> {
        let decode = || -> Result<_, Box<dyn std::error::Error>> {
            crate::facts::ItemFacts::validate_pinned(self.read(&carriers::ITEM_FACTS)?)?;
            let item_facts = Arc::clone(crate::facts::ItemFacts::builtin()?);
            let world = RuntimeAssets::decode(self.read(&carriers::WORLD)?)?;
            super::world_provenance::verify_world_carrier(
                Path::new(carriers::WORLD.output),
                &world,
            )?;
            let atmosphere = RuntimeAtmosphereAssets::decode(self.read(&carriers::ATMOSPHERE)?)?;
            super::world_provenance::verify_atmosphere_carrier(
                Path::new(carriers::ATMOSPHERE.output),
                &atmosphere,
            )?;
            let entities = RuntimeEntityAssets::decode(self.read(&carriers::ENTITY)?)?;
            let actors = RuntimeActorCatalog::decode(self.read(&ACTOR)?, &entities)?;
            let language = RuntimeLangCatalog::decode(self.read(&carriers::LANG)?)?;
            let (ui, item_glint) = self.decode_ui()?;
            let icons = RuntimeIconCatalog::decode(self.read(&carriers::ICON)?)?;
            let expected = canonical_source_manifest_sha256(VANILLA_SOURCE_MANIFEST.as_bytes());
            if entities.source_manifest_sha256() != expected
                || language.source_manifest_sha256() != expected
                || language.lang_source_sha256() != VANILLA_EN_US_LANG_SHA256
                || ui.source_manifest_sha256() != expected
                || icons.source_manifest_sha256() != expected
            {
                return Err("carrier source identity mismatch".into());
            }
            let registry = pinned_block_registry_bytes();
            let protocol = active_content_registry_protocol();
            let records = read_registry_for_protocol(registry, protocol)?;
            if format!("{:x}", Sha256::digest(self.read(&carriers::BLOCK_PHYSICS)?))
                != include_str!("../../../data/block-physics-v2193.sha256").trim()
            {
                return Err("physics carrier does not match its startup pin".into());
            }
            let physics = read_physics_registry_for_protocol(
                self.read(&carriers::BLOCK_PHYSICS)?,
                registry,
                &records,
                protocol,
            )?;
            Ok(StartupCarriers {
                world,
                item_facts,
                atmosphere,
                entities,
                actors,
                language,
                ui,
                item_glint,
                physics,
                hud: RuntimeHudCatalog::decode(self.read(&carriers::HUD)?)?,
                icons,
            })
        };
        decode().map_err(|error| DeliveryError(format!("invalid startup carriers: {error}")))
    }

    /// Validates the in-memory UI carrier before startup accepts its artwork.
    fn decode_ui(&self) -> Result<(RuntimeUiAssets, UiAtlasPage), DeliveryError> {
        let bytes = self.read(&carriers::UI)?;
        let decode = || -> Result<_, AssetError> {
            let ui = RuntimeUiAssets::decode(bytes)?;
            let glint = ui.item_glint_raster()?;
            Ok((ui, glint))
        };
        decode().map_err(|error| DeliveryError(format!("invalid {}: {error}", carriers::UI.output)))
    }
}
/// Validated domain data for client composition; no browser objects escape the web host.
pub struct StartupCarriers {
    pub world: RuntimeAssets,
    pub item_facts: Arc<crate::facts::ItemFacts>,
    pub atmosphere: RuntimeAtmosphereAssets,
    pub entities: RuntimeEntityAssets,
    pub actors: RuntimeActorCatalog,
    pub language: RuntimeLangCatalog,
    pub ui: RuntimeUiAssets,
    pub item_glint: UiAtlasPage,
    pub physics: PhysicsRegistry,
    pub hud: RuntimeHudCatalog,
    pub icons: RuntimeIconCatalog,
}
/// Names the missing carrier and the common rebuild command.
fn missing(carrier: &Carrier) -> DeliveryError {
    DeliveryError(format!("missing {}", carrier.output))
}
#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a bounded delivery index over the same synthetic bytes for every carrier.
    fn manifest(bytes: &[u8]) -> DeliveryManifest {
        DeliveryManifest {
            version: DELIVERY_VERSION,
            entries: startup_carriers()
                .map(|carrier| DeliveryEntry {
                    name: carrier.name.into(),
                    bytes: bytes.len() as u64,
                    sha256: Sha256::digest(bytes).into(),
                })
                .collect(),
        }
    }
    #[test]
    fn good_hash_bad_hash_and_missing_entry() {
        let mut source = MemoryCarriers::new(manifest(b"original")).unwrap();
        source
            .insert(carriers::WORLD.name, b"original".to_vec())
            .unwrap();
        assert_eq!(source.read(&carriers::WORLD).unwrap(), b"original");
        assert!(
            source
                .insert(carriers::WORLD.name, b"modified".to_vec())
                .is_err()
        );
        assert_eq!(source.read(&carriers::WORLD).unwrap(), b"original");
        let mut missing = manifest(b"original");
        missing.entries.pop();
        assert!(MemoryCarriers::new(missing).is_err());
        assert!(source.read(&ACTOR).is_err());
    }
    #[test]
    fn valid_delivery_hash_does_not_bypass_carrier_validators() {
        let mut source = MemoryCarriers::new(manifest(b"invalid format")).unwrap();
        for carrier in startup_carriers() {
            source
                .insert(carrier.name, b"invalid format".to_vec())
                .unwrap();
        }
        assert!(source.decode_startup().is_err());
    }
    #[test]
    fn memory_source_feeds_entity_validator() {
        let bytes = super::super::test_carriers::synthetic_entity_blob(1);
        let mut source = MemoryCarriers::new(manifest(&bytes)).unwrap();
        source
            .insert(carriers::ENTITY.name, bytes.to_vec())
            .unwrap();
        let entities =
            RuntimeEntityAssets::decode(source.read(&carriers::ENTITY).unwrap()).unwrap();
        assert_eq!(
            entities.source_manifest_sha256(),
            canonical_source_manifest_sha256(VANILLA_SOURCE_MANIFEST.as_bytes())
        );
    }

    #[test]
    fn memory_ui_rejects_missing_or_invalid_required_glint() {
        for glint in [None, Some(b"invalid PNG".as_slice())] {
            let files = glint.map(|bytes| UiFile {
                path: format!("{}.png", entity::ITEM_GLINT_TEXTURE_IDENTIFIER).into(),
                bytes: bytes.into(),
            });
            let bytes = encode_ui_catalog(
                canonical_source_manifest_sha256(VANILLA_SOURCE_MANIFEST.as_bytes()),
                &[],
                &[],
                &[],
                &files.into_iter().collect::<Vec<_>>(),
            )
            .unwrap();
            let mut source = MemoryCarriers::new(manifest(&bytes)).unwrap();
            source.insert(carriers::UI.name, bytes).unwrap();
            let error = source.decode_ui().unwrap_err().to_string();
            assert!(error.contains("item glint"));
            assert!(error.contains(carriers::UI.output));
            assert!(error.contains("make assets"));
        }
    }
}
