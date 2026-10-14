//! Joins pinned item tables and reviewed rules into one validated carrier.

use super::{Result, check, manifest, read};
use assets::{
    facts::*,
    item::{
        armour::{ItemArmour, ItemArmourSlot},
        durability::ItemDurability,
        fuel::ItemFuel,
        glint::ItemGlint,
        grip::ItemGrip,
    },
    item_use::ItemUseProfile,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

/// Source files fingerprinted by preparation and read by this compiler.
pub const INPUTS: &[&str] = ITEM_FACTS_INPUTS;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ItemRule {
    id: String,
    status: Status,
    selector: Selector,
    #[serde(default)]
    traits: Vec<Trait>,
    #[serde(default)]
    roles: Vec<Role>,
    tier: Option<ItemTier>,
    armour_slot: Option<ItemArmourSlot>,
    allow_off_hand: Option<bool>,
    beacon_payment_key: Option<i64>,
    repair: Option<Repair>,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Verified,
    Provisional,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum Selector {
    Members(Vec<String>),
    Tag(String),
    Suffix(String),
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum Repair {
    Item(String),
    Tag(String),
}

macro_rules! names {
    ($name:ident, $flags:ident, $($variant:ident => $flag:ident),+ $(,)?) => {
        #[derive(Clone, Copy, Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum $name { $($variant),+ }
        const _: () = assert!($flags::ALL.0 == (0 $(| $flags::$flag.0)+));
        impl $name {
            /// Converts a reviewed rule name into its carrier flag.
            fn flag(self) -> $flags { match self { $(Self::$variant => $flags::$flag),+ } }
        }
    };
}
names!(Trait, ItemTraits,
    Glider => GLIDER, PowderSnowWalker => POWDER_SNOW_WALKER, CreativeNoBreak => CREATIVE_NO_BREAK,
    CosmeticHead => COSMETIC_HEAD, Book => BOOK, EnchantedBook => ENCHANTED_BOOK,
    Sword => SWORD, Axe => AXE, Pickaxe => PICKAXE, Shovel => SHOVEL, Hoe => HOE,
    Shears => SHEARS, Spear => SPEAR, Trident => TRIDENT, Bow => BOW, Crossbow => CROSSBOW,
    Shield => SHIELD, FishingRod => FISHING_ROD, FlintAndSteel => FLINT_AND_STEEL,
    CarrotStick => CARROT_STICK, FungusStick => FUNGUS_STICK, Compass => COMPASS,
    Brush => BRUSH, Mace => MACE, Spyglass => SPYGLASS,
);
names!(Role, ContainerRoles,
    BrewingFuel => BREWING_FUEL, BrewingBottle => BREWING_BOTTLE, LoomBanner => LOOM_BANNER,
    LoomDye => LOOM_DYE, LoomPattern => LOOM_PATTERN, SmithingInput => SMITHING_INPUT,
    SmithingMaterial => SMITHING_MATERIAL, SmithingTemplate => SMITHING_TEMPLATE,
    HorseSaddle => HORSE_SADDLE, HorseArmour => HORSE_ARMOUR, HorseDecor => HORSE_DECOR,
    BeaconPayment => BEACON_PAYMENT, CartographyMap => CARTOGRAPHY_MAP, EnchantingLapis => ENCHANTING_LAPIS,
);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArmourTable {
    schema: u32,
    protocol: u32,
    items: Vec<ArmourEntry>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArmourEntry {
    identifier: String,
    armour: ItemArmour,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DurabilityTable {
    schema: u32,
    maximums: Vec<DurabilityEntry>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DurabilityEntry {
    identifier: String,
    maximum: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UseTable {
    version: u32,
    entries: BTreeMap<String, ItemUseProfile>,
}

/// Compiles every retail identity, rejecting stale source rows and empty selectors.
pub(super) fn compile(root: &Path) -> Result<(ItemFactsPayload, String)> {
    let retail = read(&root.join(INPUTS[0]))?;
    check(
        retail == assets::item::retail::SOURCE_BYTES,
        "retail item source mismatch",
    )?;
    let mut records = BTreeMap::new();
    let mut names = BTreeMap::new();
    for (id, name) in rows(&retail)? {
        let id: i32 = id.parse()?;
        check(
            names.insert(name.to_owned(), id).is_none(),
            "duplicate retail item name",
        )?;
        check(
            records
                .insert(
                    id,
                    ItemFactRecord {
                        network_id: id,
                        identifier: name.into(),
                        fact: ItemFact::default(),
                    },
                )
                .is_none(),
            "duplicate retail item ID",
        )?;
    }
    let capacity = read(&root.join(INPUTS[1]))?;
    let mut capacities = BTreeSet::new();
    for (name, size) in rows(&capacity)? {
        check(capacities.insert(name), "duplicate item capacity")?;
        let size: u8 = size.parse()?;
        check(size > 0, "zero item capacity")?;
        fact_mut(&mut records, &names, name)?.max_stack_size = Some(size);
    }
    check(capacities.len() == records.len(), "item capacity coverage")?;
    let tags_bytes = read(&root.join(INPUTS[2]))?;
    let mut memberships = BTreeMap::new();
    for (name, members) in rows(&tags_bytes)? {
        check(
            memberships
                .insert(
                    name.to_owned(),
                    members.split_whitespace().collect::<BTreeSet<_>>(),
                )
                .is_none(),
            "duplicate item tag",
        )?;
    }
    check(memberships.len() <= 128, "item tag bitset exceeded")?;
    let mut tags: Vec<_> = memberships
        .keys()
        .map(|name| ItemTagDefinition {
            name: name.as_str().into(),
            traits: ItemTraits::default(),
            roles: ContainerRoles::default(),
            provisional_traits: ItemTraits::default(),
            provisional_roles: ContainerRoles::default(),
            tier: None,
            repair: None,
            repair_provisional: false,
        })
        .collect();
    for (index, (_, members)) in memberships.iter().enumerate() {
        for member in members {
            // Tag inputs may retain non-retail identities; those get no retail record.
            if let Some(id) = names.get(*member) {
                records.get_mut(id).unwrap().fact.tags.0 |= 1 << index;
            }
        }
    }
    let armour: ArmourTable = serde_json::from_slice(&read(&root.join(INPUTS[3]))?)?;
    check(
        armour.schema == 1 && armour.protocol == assets::active_content_registry_protocol(),
        "item armour schema",
    )?;
    let mut seen = BTreeSet::new();
    for entry in armour.items {
        check(
            seen.insert(entry.identifier.clone()),
            "duplicate item armour",
        )?;
        let fact = fact_mut(&mut records, &names, &entry.identifier)?;
        fact.armour = entry.armour;
        fact.armour_slot = match entry.armour {
            ItemArmour::Wearable { slot, .. } => Some(slot),
            _ => None,
        };
    }
    let durability: DurabilityTable = serde_json::from_slice(&read(&root.join(INPUTS[4]))?)?;
    check(durability.schema == 1, "item durability schema")?;
    seen.clear();
    for entry in durability.maximums {
        check(
            entry.maximum > 0 && seen.insert(entry.identifier.clone()),
            "invalid item durability",
        )?;
        fact_mut(&mut records, &names, &entry.identifier)?.durability = ItemDurability::Maximum {
            maximum: entry.maximum,
        };
    }
    for record in records.values_mut() {
        record.fact.fuel = ItemFuel::Provisional {
            eligible: false,
            ingredient_first: false,
            burn_ticks: None,
        };
        record.fact.grip = ItemGrip::Provisional {
            upright: false,
            mirrored_art: false,
        };
        record.fact.glint = ItemGlint::Provisional { enabled: false };
    }
    for (input, fuel) in [(INPUTS[5], true), (INPUTS[7], false)] {
        seen.clear();
        let bytes = read(&root.join(input))?;
        for (name, flag) in rows(&bytes)? {
            check(
                seen.insert(name.to_owned()),
                "duplicate item compatibility fact",
            )?;
            let flag = match flag {
                "0" => false,
                "1" => true,
                _ => return Err("invalid compatibility flag".into()),
            };
            let fact = fact_mut(&mut records, &names, name)?;
            if fuel {
                fact.fuel = ItemFuel::Provisional {
                    eligible: true,
                    ingredient_first: flag,
                    burn_ticks: None,
                };
            } else {
                fact.grip = ItemGrip::Provisional {
                    upright: true,
                    mirrored_art: flag,
                };
            }
        }
    }
    seen.clear();
    let glint = read(&root.join(INPUTS[6]))?;
    for name in std::str::from_utf8(&glint)?
        .lines()
        .filter(|row| !row.is_empty() && !row.starts_with('#'))
    {
        check(seen.insert(name.to_owned()), "duplicate item glint")?;
        fact_mut(&mut records, &names, name)?.glint = ItemGlint::Provisional { enabled: true };
    }
    let profiles: UseTable = serde_json::from_slice(&read(&root.join(INPUTS[8]))?)?;
    check(profiles.version == 2, "item use schema")?;
    let exceptions: serde_json::Value = serde_json::from_slice(&read(&root.join(INPUTS[10]))?)?;
    check(
        exceptions["schema"] == 1,
        "item vocabulary exceptions schema",
    )?;
    let mut unresolved_profiles = Vec::new();
    let mut uses = Vec::new();
    for (name, profile) in profiles.entries {
        let index = u16::try_from(uses.len())?;
        if names.contains_key(&name) {
            fact_mut(&mut records, &names, &name)?.use_profile = Some(index);
        } else {
            check(
                exceptions["entries"]
                    .as_array()
                    .ok_or("item vocabulary exceptions missing")?
                    .iter()
                    .any(|entry| {
                        entry["reference"]["file"] == "item-use-profiles.json"
                            && entry["reference"]["identifier"] == name
                            && entry["reference"]["pointer"] == format!("/entries/{name}")
                            && entry["reason"]
                                .as_str()
                                .is_some_and(|reason| !reason.is_empty())
                    }),
                format!("unknown item-use source identity {name}"),
            )?;
            unresolved_profiles.push(name);
        }
        uses.push(profile);
    }
    let policy: super::Policy = serde_json::from_slice(&read(&root.join(INPUTS[9]))?)?;
    let mut rule_ids = BTreeSet::new();
    let mut expansion =
        String::from("identifier\ttraits\troles\tprovisional_traits\tprovisional_roles\trules\n");
    let mut applied = BTreeMap::<i32, Vec<&str>>::new();
    for rule in &policy.item_rules {
        check(
            !rule.id.is_empty()
                && !rule.id.chars().any(char::is_control)
                && rule_ids.insert(&rule.id),
            "item rule identity",
        )?;
        let selected: Vec<i32> = match &rule.selector {
            Selector::Members(members) => members
                .iter()
                .map(|name| {
                    names
                        .get(name)
                        .copied()
                        .ok_or_else(|| format!("unknown rule item {name}").into())
                })
                .collect::<Result<_>>()?,
            Selector::Tag(tag) => {
                let index = tags
                    .iter()
                    .position(|definition| definition.name.as_ref() == tag)
                    .ok_or("unknown rule tag")?;
                check(
                    rule.armour_slot.is_none()
                        && rule.allow_off_hand.is_none()
                        && rule.beacon_payment_key.is_none(),
                    "tag rule needs portable facts",
                )?;
                let definition = &mut tags[index];
                for trait_ in &rule.traits {
                    definition.traits.insert(trait_.flag());
                    if matches!(rule.status, Status::Provisional) {
                        definition.provisional_traits.insert(trait_.flag());
                    }
                }
                for role in &rule.roles {
                    definition.roles.insert(role.flag());
                    if matches!(rule.status, Status::Provisional) {
                        definition.provisional_roles.insert(role.flag());
                    }
                }
                check(
                    rule.tier.is_none()
                        || definition.tier.is_none()
                        || definition.tier == rule.tier,
                    "conflicting tag tiers",
                )?;
                if rule.tier.is_some() {
                    definition.tier = rule.tier;
                }
                records
                    .values()
                    .filter(|record| record.fact.tags.0 & (1 << index) != 0)
                    .map(|record| record.network_id)
                    .collect()
            }
            Selector::Suffix(suffix) => {
                check(!suffix.is_empty(), "empty item suffix")?;
                names
                    .iter()
                    .filter(|(name, _)| name.ends_with(suffix))
                    .map(|(_, &id)| id)
                    .collect()
            }
        };
        check(
            !selected.is_empty(),
            format!("item rule {} matches no retail items", rule.id),
        )?;
        let repair = match &rule.repair {
            Some(Repair::Item(name)) => Some(ItemRepairMaterial::Item(
                *names.get(name).ok_or("unknown repair item")?,
            )),
            Some(Repair::Tag(name)) => Some(ItemRepairMaterial::Tag(u8::try_from(
                tags.iter()
                    .position(|tag| tag.name.as_ref() == name)
                    .ok_or("unknown repair tag")?,
            )?)),
            None => None,
        };
        if let Selector::Tag(name) = &rule.selector
            && let Some(repair) = repair
        {
            let definition = tags
                .iter_mut()
                .find(|tag| tag.name.as_ref() == name)
                .unwrap();
            check(
                definition.repair.is_none() || definition.repair == Some(repair),
                "conflicting tag repair",
            )?;
            definition.repair = Some(repair);
            definition.repair_provisional = matches!(rule.status, Status::Provisional);
        }
        for id in selected {
            let fact = &mut records.get_mut(&id).unwrap().fact;
            for trait_ in &rule.traits {
                fact.traits.insert(trait_.flag());
                if matches!(rule.status, Status::Provisional) {
                    fact.provisional_traits.insert(trait_.flag());
                }
            }
            for role in &rule.roles {
                fact.roles.insert(role.flag());
                if matches!(rule.status, Status::Provisional) {
                    fact.provisional_roles.insert(role.flag());
                }
            }
            if let Some(tier) = rule.tier {
                check(
                    fact.tier.is_none() || fact.tier == Some(tier),
                    "conflicting item tiers",
                )?;
                fact.tier = Some(tier);
            }
            if let Some(slot) = rule.armour_slot {
                check(
                    fact.armour_slot.is_none() || fact.armour_slot == Some(slot),
                    "conflicting armour slots",
                )?;
                fact.armour_slot = Some(slot);
            }
            if let Some(key) = rule.beacon_payment_key {
                check(
                    fact.beacon_payment_key.is_none() && key > 0,
                    "conflicting beacon icon key",
                )?;
                fact.beacon_payment_key = Some(key);
            }
            if let Some(offhand) = rule.allow_off_hand {
                check(
                    fact.allow_off_hand.is_none() || fact.allow_off_hand == Some(offhand),
                    "conflicting offhand facts",
                )?;
                fact.allow_off_hand = Some(offhand);
                fact.offhand_provisional = matches!(rule.status, Status::Provisional);
            }
            if let Some(repair) = repair {
                check(
                    fact.repair.is_none()
                        || fact.repair == Some(repair)
                        || matches!(rule.selector, Selector::Members(_)),
                    "conflicting repair materials",
                )?;
                fact.repair = Some(repair);
                fact.repair_provisional = matches!(rule.status, Status::Provisional);
            }
            applied.entry(id).or_default().push(&rule.id);
        }
    }
    for record in records.values() {
        let fact = record.fact;
        expansion.push_str(&format!(
            "{}\t{:x}\t{:x}\t{:x}\t{:x}\t{}\n",
            record.identifier,
            fact.traits.0,
            fact.roles.0,
            fact.provisional_traits.0,
            fact.provisional_roles.0,
            applied
                .get(&record.network_id)
                .map(|rules| rules.join(","))
                .unwrap_or_else(|| "-".to_owned())
        ));
    }
    for name in unresolved_profiles {
        expansion.push_str(&format!("{name}\t-\t-\t-\t-\tmissing_retail_id\n"));
    }
    Ok((
        ItemFactsPayload {
            protocol: assets::active_content_registry_protocol(),
            retail_sha256: Sha256::digest(&retail).into(),
            tags: tags.into_boxed_slice(),
            profiles: uses.into_boxed_slice(),
            records: records.into_values().collect(),
        },
        expansion,
    ))
}

/// Writes the checked carrier and an optional reviewed selector expansion.
pub fn write(root: &Path, out: &Path, expansion: Option<&Path>) -> Result<()> {
    let (payload, expanded) = compile(root)?;
    let bytes = encode_item_facts(payload)?;
    manifest::write_blob(out, &bytes, false)?;
    if let Some(path) = expansion {
        manifest::write(path, expanded.as_bytes())?;
    }
    Ok(())
}

/// Reads strict two-column rows while allowing explanatory header comments.
fn rows(bytes: &[u8]) -> Result<Vec<(&str, &str)>> {
    std::str::from_utf8(bytes)?
        .lines()
        .filter(|row| !row.is_empty() && !row.starts_with('#'))
        .map(|row| {
            row.split_once('\t')
                .ok_or_else(|| "invalid item source row".into())
        })
        .collect()
}

/// Rejects facts whose identity is absent from the independent retail inventory.
fn fact_mut<'a>(
    records: &'a mut BTreeMap<i32, ItemFactRecord>,
    names: &BTreeMap<String, i32>,
    name: &str,
) -> Result<&'a mut ItemFact> {
    let id = names
        .get(name)
        .ok_or_else(|| format!("unknown item source identity {name}"))?;
    Ok(&mut records.get_mut(id).unwrap().fact)
}

#[cfg(test)]
mod tests;
