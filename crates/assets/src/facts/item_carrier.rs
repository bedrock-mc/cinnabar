use super::{ContainerRoles, ItemFact, ItemFactsPayload, ItemRepairMaterial, ItemTag};
use crate::{
    item::{armour::ItemArmour, durability::ItemDurability, retail},
    item_use::ItemUseProfile,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, OnceLock},
};
use thiserror::Error;

pub const ITEM_FACTS_SOURCE_PATH: &str = "crates/assets/data/item-facts-v2193.bin";
pub const ITEM_FACTS_FILENAME: &str = "item-facts-v2193.bin";
pub const ITEM_FACTS_EXPANSION_PATH: &str = "crates/assets/data/item-facts-expansion-v2193.tsv";
pub const MAX_ITEM_FACTS_BYTES: usize = 4 * 1024 * 1024;
/// Tracked inputs shared by the facts compiler and asset preparation recipe.
pub const ITEM_FACTS_INPUTS: &[&str] = &[
    retail::INPUT_PATH,
    "crates/protocol/data/item_capacity_1_26_50.tsv",
    "crates/protocol/data/item_tags_native.tsv",
    "crates/assets/data/item-armour-v2193.json",
    "crates/assets/data/item-durability-v2193.json",
    "crates/assets/data/item-fuel-v2193.tsv",
    "crates/assets/data/item-glint-v2193.tsv",
    "crates/assets/data/item-grips-v2193.tsv",
    "crates/assets/data/item-use-profiles.json",
    "crates/asset-compiler/data/registry-policy.json",
    "crates/assets/data/vocabulary-exceptions.json",
];
const MAGIC: &[u8; 8] = b"IFAC0002";
const MAX_ID_SPAN: usize = 65_536;
const MAX_PROFILES: usize = 512;

/// A carrier error always names a bounded validation failure.
#[derive(Clone, Debug, Error)]
#[error("invalid item facts: {0}")]
pub struct ItemFactsError(pub(super) String);

/// Immutable item facts with dense numeric lookup and cold identity admission.
#[derive(Clone, Debug)]
pub struct ItemFacts {
    payload: ItemFactsPayload,
    first_id: i32,
    indices: Box<[Option<usize>]>,
    names: BTreeMap<Box<str>, usize>,
    beacon_payments: Box<[usize]>,
}

impl ItemFacts {
    /// Returns the builtin carrier after one hash and payload validation.
    pub fn builtin() -> Result<&'static Arc<Self>, ItemFactsError> {
        static FACTS: OnceLock<Result<Arc<ItemFacts>, ItemFactsError>> = OnceLock::new();
        FACTS
            .get_or_init(|| {
                let bytes = include_bytes!("../../data/item-facts-v2193.bin");
                Self::validate_pinned(bytes)?;
                Self::decode(bytes).map(Arc::new)
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    /// Checks installed bytes against the reviewed carrier's external fingerprint.
    pub fn validate_pinned(bytes: &[u8]) -> Result<(), ItemFactsError> {
        ensure(bytes.len() <= MAX_ITEM_FACTS_BYTES, "carrier size")?;
        let expected = include_str!("../../data/item-facts-v2193.sha256").trim();
        ensure(
            format!("{:x}", Sha256::digest(bytes)) == expected,
            "pinned carrier hash",
        )
    }

    /// Decodes a hash-sealed, version-bound carrier before exposing any facts.
    pub fn decode(bytes: &[u8]) -> Result<Self, ItemFactsError> {
        ensure(
            bytes.len() >= MAGIC.len() + 32 && bytes.len() <= MAX_ITEM_FACTS_BYTES,
            "carrier size",
        )?;
        let (content, checksum) = bytes.split_at(bytes.len() - 32);
        ensure(content.starts_with(MAGIC), "carrier format")?;
        ensure(
            Sha256::digest(content).as_slice() == checksum,
            "carrier checksum",
        )?;
        let payload: ItemFactsPayload = serde_json::from_slice(&content[MAGIC.len()..])
            .map_err(|error| ItemFactsError(error.to_string()))?;
        Self::from_payload(payload)
    }

    /// Validates the compiler payload against the independent retail inventory.
    pub fn from_payload(payload: ItemFactsPayload) -> Result<Self, ItemFactsError> {
        validate(&payload)?;
        let first_id = payload
            .records
            .first()
            .ok_or_else(|| ItemFactsError("empty inventory".into()))?
            .network_id;
        let last_id = payload.records.last().unwrap().network_id;
        let span = i64::from(last_id) - i64::from(first_id) + 1;
        ensure((1..=MAX_ID_SPAN as i64).contains(&span), "item ID span")?;
        let mut indices = vec![None; span as usize];
        let mut names = BTreeMap::new();
        let mut beacon_payments = Vec::new();
        for (index, record) in payload.records.iter().enumerate() {
            indices[(i64::from(record.network_id) - i64::from(first_id)) as usize] = Some(index);
            names.insert(record.identifier.clone(), index);
            if record.fact.roles.contains(ContainerRoles::BEACON_PAYMENT) {
                beacon_payments.push(index);
            }
        }
        Ok(Self {
            payload,
            first_id,
            indices: indices.into_boxed_slice(),
            names,
            beacon_payments: beacon_payments.into_boxed_slice(),
        })
    }

    /// Returns facts by the pinned signed retail ID, preserving gaps.
    pub fn get(&self, network_id: i32) -> Option<&ItemFact> {
        let offset = usize::try_from(i64::from(network_id) - i64::from(self.first_id)).ok()?;
        self.indices
            .get(offset)?
            .map(|index| &self.payload.records[index].fact)
    }

    /// Resolves a negotiated identifier once, independently from its server ID.
    pub fn by_identifier(&self, identifier: &str) -> Option<&ItemFact> {
        self.names
            .get(identifier)
            .map(|&index| &self.payload.records[index].fact)
    }

    /// Resolves compiled book forms and the read-only identity missing from the retail census.
    pub fn book_editable(&self, identifier: &str) -> Option<bool> {
        self.by_identifier(identifier)
            .and_then(|fact| fact.book_editable)
            .or_else(|| (identifier == super::item::WRITTEN_BOOK_IDENTIFIER).then_some(false))
    }

    /// Returns the pinned ID for cold repair-material admission.
    pub fn network_id(&self, identifier: &str) -> Option<i32> {
        self.names
            .get(identifier)
            .map(|&index| self.payload.records[index].network_id)
    }

    /// Admits an exact tag name into the carrier's numeric vocabulary.
    pub fn tag(&self, name: &str) -> Option<ItemTag> {
        self.payload
            .tags
            .binary_search_by(|tag| tag.name.as_ref().cmp(name))
            .ok()
            .map(|index| ItemTag(index as u8))
    }

    /// Adds declared tags while preserving pinned tier and exact repair overrides.
    pub fn apply_tags<'a>(&self, fact: &mut ItemFact, tags: impl IntoIterator<Item = &'a str>) {
        for name in tags {
            if let Some(tag) = self.tag(name) {
                fact.tags.insert(tag);
                let definition = &self.payload.tags[tag.0 as usize];
                fact.traits.insert(definition.traits);
                fact.roles.insert(definition.roles);
                fact.provisional_traits
                    .insert(definition.provisional_traits);
                fact.provisional_roles.insert(definition.provisional_roles);
            }
        }
        if fact.repair.is_none() {
            let mut repairs = self
                .payload
                .tags
                .iter()
                .enumerate()
                .filter_map(|(index, tag)| {
                    fact.tags
                        .contains(ItemTag(index as u8))
                        .then_some(tag.repair)
                        .flatten()
                });
            fact.repair = repairs.next();
            fact.repair_provisional = self.payload.tags.iter().enumerate().any(|(index, tag)| {
                fact.tags.contains(ItemTag(index as u8)) && tag.repair_provisional
            });
            if repairs.any(|repair| Some(repair) != fact.repair) {
                fact.repair = None;
            }
        }
        if fact.tier.is_none() {
            let mut tiers = self
                .payload
                .tags
                .iter()
                .enumerate()
                .filter_map(|(index, tag)| {
                    fact.tags
                        .contains(ItemTag(index as u8))
                        .then_some(tag.tier)
                        .flatten()
                });
            fact.tier = tiers.next();
            if tiers.next().is_some() {
                fact.tier = None;
            }
        }
    }

    /// Borrows the cached payment icon routes, derived once from validated roles.
    pub fn beacon_payments(&self) -> impl Iterator<Item = (i64, &str)> {
        self.beacon_payments.iter().map(|&index| {
            let record = &self.payload.records[index];
            (
                record
                    .fact
                    .beacon_payment_key
                    .expect("validated beacon key"),
                record.identifier.as_ref(),
            )
        })
    }

    /// Borrows a compiled use profile by its admitted numeric index.
    pub fn use_profile(&self, index: u16) -> Option<&ItemUseProfile> {
        self.payload.profiles.get(usize::from(index))
    }

    /// Exposes the validated inventory for compiler and completeness checks.
    pub fn payload(&self) -> &ItemFactsPayload {
        &self.payload
    }
}

/// Encodes exactly the payload the runtime accepts, with a trailing SHA-256 seal.
pub fn encode_item_facts(payload: ItemFactsPayload) -> Result<Vec<u8>, ItemFactsError> {
    let facts = ItemFacts::from_payload(payload)?;
    let mut bytes = MAGIC.to_vec();
    bytes.extend(
        serde_json::to_vec(&facts.payload).map_err(|error| ItemFactsError(error.to_string()))?,
    );
    bytes.extend_from_slice(&Sha256::digest(&bytes));
    ensure(bytes.len() <= MAX_ITEM_FACTS_BYTES, "carrier size")?;
    Ok(bytes)
}

/// Rejects unsupported payload fields before constructing lookup indexes.
fn validate(payload: &ItemFactsPayload) -> Result<(), ItemFactsError> {
    ensure(
        payload.protocol == crate::active_content_registry_protocol(),
        "protocol mismatch",
    )?;
    ensure(
        payload.retail_sha256 == <[u8; 32]>::from(Sha256::digest(retail::SOURCE_BYTES)),
        "retail identity hash",
    )?;
    ensure(
        !payload.tags.is_empty()
            && payload.tags.len() <= 128
            && payload.profiles.len() <= MAX_PROFILES,
        "tag or profile count",
    )?;
    let allowed_tags = if payload.tags.len() == 128 {
        u128::MAX
    } else {
        (1 << payload.tags.len()) - 1
    };
    let mut previous = None;
    for tag in &payload.tags {
        ensure(
            tag.name.starts_with("minecraft:")
                && tag.name.len() <= 256
                && !tag.name.chars().any(char::is_control)
                && previous.is_none_or(|name: &str| name < tag.name.as_ref()),
            "tag identity order",
        )?;
        validate_masks(
            tag.traits.0,
            tag.roles.0,
            tag.provisional_traits.0,
            tag.provisional_roles.0,
        )?;
        previous = Some(&*tag.name);
    }
    let retail: BTreeMap<i32, &str> = std::str::from_utf8(retail::SOURCE_BYTES)
        .unwrap()
        .lines()
        .map(|row| {
            let (id, name) = row.split_once('\t').unwrap();
            (id.parse().unwrap(), name)
        })
        .collect();
    ensure(payload.records.len() == retail.len(), "retail coverage")?;
    let ids: BTreeSet<_> = payload
        .records
        .iter()
        .map(|record| record.network_id)
        .collect();
    let mut beacon_keys = BTreeSet::new();
    for (record, (&id, &name)) in payload.records.iter().zip(&retail) {
        ensure(
            record.network_id == id && record.identifier.as_ref() == name,
            "retail identity order",
        )?;
        let fact = record.fact;
        validate_masks(
            fact.traits.0,
            fact.roles.0,
            fact.provisional_traits.0,
            fact.provisional_roles.0,
        )?;
        ensure(
            fact.tags.0 & !allowed_tags == 0
                && fact.max_stack_size != Some(0)
                && fact
                    .use_profile
                    .is_none_or(|index| usize::from(index) < payload.profiles.len())
                && !matches!(fact.durability, ItemDurability::Maximum { maximum: 0 }),
            "item values",
        )?;
        ensure(
            fact.roles.contains(ContainerRoles::BEACON_PAYMENT)
                == fact.beacon_payment_key.is_some(),
            "beacon icon coverage",
        )?;
        if let Some(key) = fact.beacon_payment_key {
            ensure(key > 0 && beacon_keys.insert(key), "beacon icon identity")?;
        }
        if let ItemArmour::Wearable { slot, .. } = fact.armour {
            ensure(fact.armour_slot == Some(slot), "armour slot mismatch")?;
        }
        match fact.repair {
            Some(ItemRepairMaterial::Item(id)) => {
                ensure(ids.contains(&id), "repair item identity")?
            }
            Some(ItemRepairMaterial::Tag(index)) => ensure(
                usize::from(index) < payload.tags.len(),
                "repair tag identity",
            )?,
            None => {}
        }
    }
    for profile in &payload.profiles {
        if let ItemUseProfile::Provisional {
            movement_bits,
            cooldown,
            duration_alias,
            ..
        } = profile
        {
            let movement = f64::from_bits(*movement_bits);
            ensure(
                movement.is_finite()
                    && movement >= 0.0
                    && cooldown.as_ref().is_none_or(|value| {
                        !value.category.is_empty() && value.category.len() <= 256
                    })
                    && duration_alias
                        .as_ref()
                        .is_none_or(|value| value.len() <= 256),
                "use profile values",
            )?;
        }
    }
    Ok(())
}

/// Validates all known masks and keeps provisional flags within admitted flags.
fn validate_masks(
    traits: u64,
    roles: u64,
    provisional_traits: u64,
    provisional_roles: u64,
) -> Result<(), ItemFactsError> {
    ensure(
        traits & !super::ItemTraits::ALL.0 == 0
            && roles & !super::ContainerRoles::ALL.0 == 0
            && provisional_traits & !traits == 0
            && provisional_roles & !roles == 0,
        "unsupported fact masks",
    )
}

/// Converts an invariant failure into a useful carrier error.
fn ensure(condition: bool, message: &str) -> Result<(), ItemFactsError> {
    if condition {
        Ok(())
    } else {
        Err(ItemFactsError(message.into()))
    }
}
