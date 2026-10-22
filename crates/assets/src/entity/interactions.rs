//! Compiled interaction dispositions resolved once when an actor enters a session.

use std::{borrow::Cow, collections::BTreeMap};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    CompiledEntityAssets, EntityAssetKind, EntityAssetSymbol, RuntimeEntityAssets, invalid,
};
use crate::AssetError;

pub const SOURCE_PATH: &str = "registry/entity-interactions.json";
const SOURCE_BYTES: &[u8] = crate::facts::entity::SOURCE_BYTES;

/// Unsupported identities retain no interaction disposition.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum EntityInteractionTraits {
    #[default]
    Unresolved,
    /// Existing compatibility routing; complete native class and component coverage is open.
    Provisional {
        pickable: bool,
        obstructs_placement: bool,
        aim_assist: bool,
    },
}

impl EntityInteractionTraits {
    /// Compatibility disposition for a declared generic-mob construction.
    pub const GENERIC_MOB: Self = Self::Provisional {
        pickable: true,
        obstructs_placement: true,
        aim_assist: true,
    };

    /// Returns selectable-box eligibility without resolving an identifier.
    pub const fn pickable(self) -> Option<bool> {
        match self {
            Self::Unresolved => None,
            Self::Provisional { pickable, .. } => Some(pickable),
        }
    }

    /// Returns obstruction independently from selectable-box eligibility.
    pub const fn obstructs_placement(self) -> Option<bool> {
        match self {
            Self::Unresolved => None,
            Self::Provisional {
                obstructs_placement,
                ..
            } => Some(obstructs_placement),
        }
    }

    /// Returns aim eligibility without resolving an identifier.
    pub const fn aim_assist(self) -> Option<bool> {
        match self {
            Self::Unresolved => None,
            Self::Provisional { aim_assist, .. } => Some(aim_assist),
        }
    }
}

/// One compiler-owned identity and its independent interaction dispositions.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityInteractionDescriptor {
    pub identifier: Box<str>,
    pub traits: EntityInteractionTraits,
    pub offered_use: super::offered_use::EntityUseAdmission,
}

/// Hashes the compiled facts source used by entity carriers.
pub fn source_identity() -> (u32, [u8; 32]) {
    (
        SOURCE_BYTES.len() as u32,
        Sha256::digest(SOURCE_BYTES).into(),
    )
}

/// Normalizes a factory identity only at packet or carrier admission.
fn factory_identifier(identifier: &str) -> Cow<'_, str> {
    let identifier = identifier.split('<').next().unwrap_or(identifier);
    if identifier.contains(':') {
        Cow::Borrowed(identifier)
    } else {
        Cow::Owned(format!("minecraft:{identifier}"))
    }
}

/// Provides compatibility traits for diagnostics and factory admission, never for frame queries.
pub fn builtin(identifier: &str) -> EntityInteractionTraits {
    crate::facts::entity::EntityFacts::pinned()
        .get(factory_identifier(identifier).as_ref())
        .interactions
}

/// Checks the independently collected entity symbols, retaining unsupported entries explicitly.
pub fn compile(symbols: &[EntityAssetSymbol]) -> Box<[EntityInteractionDescriptor]> {
    inventory(
        symbols,
        crate::facts::entity::EntityFacts::pinned()
            .records
            .iter()
            .map(|r| (r.identifier.clone(), r.fact.interactions))
            .collect(),
    )
}

/// Retains explicit unsupported entries for every independently collected entity identity.
fn unresolved_inventory(symbols: &[EntityAssetSymbol]) -> Box<[EntityInteractionDescriptor]> {
    inventory(symbols, BTreeMap::new())
}

/// Adds the independent inventory to an admitted source without replacing its dispositions.
fn inventory(
    symbols: &[EntityAssetSymbol],
    mut entries: BTreeMap<Box<str>, EntityInteractionTraits>,
) -> Box<[EntityInteractionDescriptor]> {
    for symbol in symbols
        .iter()
        .filter(|symbol| symbol.kind == EntityAssetKind::Entity)
    {
        entries.entry(symbol.identifier.clone()).or_default();
    }
    entries
        .into_iter()
        .map(|(identifier, traits)| EntityInteractionDescriptor {
            identifier,
            traits,
            offered_use: Default::default(),
        })
        .collect()
}

/// Rejects omitted identities, forged dispositions and altered source identities before retention.
pub(super) fn validate(compiled: &CompiledEntityAssets) -> Result<(), AssetError> {
    let source = compiled
        .sources
        .iter()
        .find(|source| source.path.as_ref() == SOURCE_PATH);
    let Some(source) = source else {
        return if compiled.interaction_traits == unresolved_inventory(&compiled.symbols) {
            Ok(())
        } else {
            Err(invalid(
                "unresolved interaction traits disagree with the entity inventory",
            ))
        };
    };
    let (length, hash) = source_identity();
    if source.source_bytes != length || source.source_sha256 != hash {
        return Err(invalid("interaction trait source identity changed"));
    }
    if compiled
        .interaction_traits
        .iter()
        .map(|entry| (&entry.identifier, entry.traits))
        .ne(compile(&compiled.symbols)
            .iter()
            .map(|entry| (&entry.identifier, entry.traits)))
    {
        return Err(invalid(
            "interaction traits disagree with the collected entity inventory",
        ));
    }
    super::offered_use::validate(compiled)?;
    Ok(())
}

impl CompiledEntityAssets {
    /// Fills the independent entity inventory with explicit unsupported interactions.
    /// A compatibility source must be supplied separately to admit supported dispositions.
    pub fn with_unresolved_interactions(mut self) -> Self {
        self.interaction_traits = unresolved_inventory(&self.symbols);
        self
    }
}

impl RuntimeEntityAssets {
    /// Resolves a compiled factory identity at actor admission; unsupported identities stay absent.
    pub fn interaction_traits_for(&self, identifier: &str) -> EntityInteractionTraits {
        let key = factory_identifier(identifier);
        self.interaction_traits
            .binary_search_by(|entry| entry.identifier.as_ref().cmp(key.as_ref()))
            .ok()
            .map(|index| self.interaction_traits[index].traits)
            .unwrap_or_default()
    }
}
