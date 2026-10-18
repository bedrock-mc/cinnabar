//! Verified delivery of runtime carriers, without paths pretending to be a filesystem.

use crate::carriers::{self as carriers, ACTOR, Carrier, Recipe, required};
use crate::{
    AssetError, PhysicsRegistry, RuntimeActorCatalog, RuntimeAssets, RuntimeAtmosphereAssets,
    RuntimeEntityAssets, RuntimeHudCatalog, RuntimeIconCatalog, RuntimeLangCatalog,
    RuntimeUiAssets, UiAtlasPage, VANILLA_EN_US_LANG_SHA256, VANILLA_SOURCE_MANIFEST,
    active_content_registry_protocol, canonical_source_manifest_sha256,
    pinned_block_registry_bytes, read_physics_registry_for_protocol, read_registry_for_protocol,
};
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

/// The required delivery set plus supported optional artwork, from the canonical table.
fn delivery_carriers() -> impl Iterator<Item = &'static Carrier> {
    startup_carriers().chain([&carriers::FONT, &carriers::AUDIO])
}

impl DeliveryManifest {
    /// Resolves indexed carriers through the same canonical table used for admission.
    pub fn carriers(&self) -> impl Iterator<Item = &'static Carrier> + '_ {
        delivery_carriers()
            .filter(|carrier| self.entries.iter().any(|entry| entry.name == carrier.name))
    }

    /// Checks the required set, supported optional carriers, unique names and byte bounds.
    pub fn validate(&self) -> Result<(), DeliveryError> {
        if self.version != DELIVERY_VERSION {
            return Err(DeliveryError("unsupported delivery manifest".into()));
        }
        let mut names = std::collections::HashSet::new();
        let mut total = 0_u64;
        for entry in &self.entries {
            if !names.insert(entry.name.as_str())
                || !delivery_carriers().any(|carrier| carrier.name == entry.name)
                || entry.bytes == 0
                || entry.bytes > MAX_DELIVERY_ENTRY_BYTES
            {
                return Err(DeliveryError(format!(
                    "invalid delivery entry for {}",
                    entry.name
                )));
            }
            total = total
                .checked_add(entry.bytes)
                .filter(|total| *total <= MAX_DELIVERY_BYTES)
                .ok_or_else(|| DeliveryError("delivery exceeds memory limit".into()))?;
        }
        for carrier in startup_carriers() {
            if !matches!(
                carrier.recipe,
                Recipe::World
                    | Recipe::ItemFacts
                    | Recipe::EntityFacts
                    | Recipe::Atmosphere
                    | Recipe::Entity
                    | Recipe::Actor
                    | Recipe::Lang
                    | Recipe::Ui
                    | Recipe::Icon
                    | Recipe::BlockPhysics
                    | Recipe::Hud
            ) {
                return Err(DeliveryError(format!(
                    "unsupported startup carrier {}",
                    carrier.output
                )));
            }
            if !names.contains(carrier.name) {
                return Err(missing(carrier));
            }
        }
        Ok(())
    }

    /// Generates the index from locally compiled carriers; no game bytes enter the index.
    pub fn from_directory(directory: &Path) -> Result<Self, DeliveryError> {
        let entries = delivery_carriers()
            .filter(|carrier| {
                startup_carriers().any(|required| required.name == carrier.name)
                    || directory.join(carrier.output).is_file()
            })
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

    /// Resolves only indexed carrier names to canonical filenames before host downloads.
    pub fn entries(&self) -> impl Iterator<Item = &'static Carrier> + '_ {
        self.manifest.carriers()
    }

    /// Decodes the optional bundled font; an absent or unusable font keeps diagnostic text.
    fn font(&self) -> Option<Arc<crate::RuntimeFontCatalog>> {
        let manifest = carriers::FONT.font_face?.manifest;
        crate::RuntimeFontCatalog::decode(
            self.read(&carriers::FONT).ok()?,
            canonical_source_manifest_sha256(manifest),
        )
        .ok()
        .map(Arc::new)
    }

    /// Keeps startup muted when the optional catalog is absent, invalid or from another source.
    fn audio(&self) -> Option<Arc<crate::RuntimeAudioCatalog>> {
        crate::RuntimeAudioCatalog::decode(self.read(&carriers::AUDIO).ok()?)
            .ok()
            .filter(|audio| {
                audio.source_manifest_sha256()
                    == canonical_source_manifest_sha256(VANILLA_SOURCE_MANIFEST.as_bytes())
            })
            .map(Arc::new)
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
            crate::facts::entity::EntityFacts::validate_pinned(
                self.read(&carriers::ENTITY_FACTS)?,
            )?;
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
                audio: self.audio(),
                font: self.font(),
                world: Arc::new(world),
                item_facts,
                atmosphere: Arc::new(atmosphere),
                atmosphere_identity: Sha256::digest(self.read(&carriers::ATMOSPHERE)?).into(),
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
    pub audio: Option<Arc<crate::RuntimeAudioCatalog>>,
    pub font: Option<Arc<crate::RuntimeFontCatalog>>,
    pub world: Arc<RuntimeAssets>,
    pub item_facts: Arc<crate::facts::ItemFacts>,
    pub atmosphere: Arc<RuntimeAtmosphereAssets>,
    pub atmosphere_identity: [u8; 32],
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
    use crate::{UiFile, encode_ui_catalog, entity};

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
    fn absent_optional_audio_keeps_startup_muted() {
        let missing = MemoryCarriers::new(manifest(b"unused")).unwrap();
        assert!(missing.audio().is_none());
    }

    #[test]
    fn valid_optional_audio_uses_the_pinned_catalog() {
        let pinned = canonical_source_manifest_sha256(VANILLA_SOURCE_MANIFEST.as_bytes());
        let source = delivered_audio(crate::encode_audio_catalog(pinned, [0; 32], &[]).unwrap());
        assert_eq!(source.audio().unwrap().source_manifest_sha256(), pinned);
    }

    /// Delivers audio with a matching transport digest so catalog validation decides admission.
    fn delivered_audio(bytes: Vec<u8>) -> MemoryCarriers {
        let mut index = manifest(b"unused");
        index.entries.push(DeliveryEntry {
            name: carriers::AUDIO.name.into(),
            bytes: bytes.len() as u64,
            sha256: Sha256::digest(&bytes).into(),
        });
        let mut source = MemoryCarriers::new(index).unwrap();
        source.insert(carriers::AUDIO.name, bytes).unwrap();
        source
    }

    #[test]
    fn malformed_optional_audio_keeps_startup_muted() {
        let source = delivered_audio(b"invalid audio".to_vec());
        assert!(source.audio().is_none());
    }

    #[test]
    fn stale_optional_audio_keeps_startup_muted() {
        let source = delivered_audio(crate::encode_audio_catalog([0; 32], [0; 32], &[]).unwrap());
        assert!(source.audio().is_none());
    }
    #[test]
    fn optional_font_is_indexed_but_unknown_or_duplicate_entries_are_rejected() {
        let mut index = manifest(b"font bytes");
        let entry = DeliveryEntry {
            name: carriers::FONT.name.into(),
            bytes: 10,
            sha256: Sha256::digest(b"font bytes").into(),
        };
        index.entries.push(entry.clone());
        let mut source = MemoryCarriers::new(index.clone()).unwrap();
        assert!(
            source
                .entries()
                .any(|carrier| carrier.name == carriers::FONT.name)
        );
        assert!(source.font().is_none());
        source
            .insert(carriers::FONT.name, b"font bytes".to_vec())
            .unwrap();
        assert!(
            source.font().is_none(),
            "a delivery digest does not bypass font validation"
        );
        index.entries.push(entry);
        assert!(index.validate().is_err());
        index.entries.pop();
        index.entries.last_mut().unwrap().name = "unknown".into();
        assert!(index.validate().is_err());
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
    fn memory_startup_requires_the_same_entity_facts_pin_as_file_startup() {
        use crate::facts::entity::{EntityFacts, SOURCE_BYTES};
        let directory = tempfile::tempdir().unwrap();
        let world = directory.path().join(carriers::WORLD.output);
        super::super::test_carriers::write_pinned_facts(&world);
        let item_bytes = std::fs::read(world.with_file_name(carriers::ITEM_FACTS.output)).unwrap();
        let mut changed =
            EntityFacts::decode(SOURCE_BYTES, active_content_registry_protocol()).unwrap();
        changed.records[0].fact.provisional = !changed.records[0].fact.provisional;
        for (pinned, entity_bytes) in [
            (true, SOURCE_BYTES.to_vec()),
            (false, changed.encode().unwrap()),
        ] {
            let mut index = manifest(b"not loaded");
            for (carrier, bytes) in [
                (carriers::ITEM_FACTS, item_bytes.as_slice()),
                (carriers::ENTITY_FACTS, entity_bytes.as_slice()),
            ] {
                let entry = index
                    .entries
                    .iter_mut()
                    .find(|entry| entry.name == carrier.name)
                    .unwrap();
                entry.bytes = bytes.len() as u64;
                entry.sha256 = Sha256::digest(bytes).into();
            }
            let mut source = MemoryCarriers::new(index).unwrap();
            source
                .insert(carriers::ITEM_FACTS.name, item_bytes.clone())
                .unwrap();
            source
                .insert(carriers::ENTITY_FACTS.name, entity_bytes)
                .unwrap();
            let error = source.decode_startup().err().unwrap().to_string();
            if pinned {
                assert!(error.contains(carriers::WORLD.output), "{error}");
            } else {
                assert!(
                    error.contains("entity facts differ from the pinned carrier"),
                    "{error}"
                );
            }
        }
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
