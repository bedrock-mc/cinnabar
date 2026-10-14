use super::{BlockFact, RESERVED_BLOCK_NAME, Result, Scalar, check};
use assets::registry::RegistryRecord;
use std::collections::BTreeMap;

/// Replaces only the state-dependent emission accessors with client-specific facts.
fn emission(record: &RegistryRecord) -> Result<Option<u8>> {
    let property = match record.name.as_ref() {
        "minecraft:trial_spawner" => "trial_spawner_state",
        "minecraft:vault" => "vault_state",
        "minecraft:respawn_anchor" => "respawn_anchor_charge",
        "minecraft:sculk_sensor" | "minecraft:calibrated_sculk_sensor" => "sculk_sensor_phase",
        _ => return Ok(None),
    };
    let fields: BTreeMap<String, Scalar> = serde_json::from_str(&record.canonical_state)?;
    let field = fields
        .get(property)
        .ok_or("state-dependent light property missing")?;
    if property == "vault_state" {
        return Ok(Some(match field.value.as_str() {
            Some("inactive") => 6,
            Some("active" | "unlocking" | "ejecting") => 12,
            _ => return Err("unknown vault state".into()),
        }));
    }
    let number = field.value.as_u64().ok_or("invalid light state number")?;
    let value = match property {
        "trial_spawner_state" => match number {
            0 | 5 => 0,
            1 => 4,
            2..=4 => 8,
            _ => return Err("unknown trial spawner state".into()),
        },
        "respawn_anchor_charge" => {
            check(number < 5, "unknown respawn anchor charge")?;
            (number * 15 / 4) as u8
        }
        _ => {
            check(number <= 2, "unknown sculk sensor phase")?;
            u8::from(number == 1)
        }
    };
    Ok(Some(value))
}

/// Combines raw library lighting with the selected client compatibility and state rules.
pub(super) fn compile(records: &[RegistryRecord], facts: &[BlockFact]) -> Result<Vec<u8>> {
    check(records.len() == facts.len(), "light source count mismatch")?;
    let mut result = Vec::with_capacity(records.len());
    for (record, fact) in records.iter().zip(facts) {
        if record.name.as_ref() == RESERVED_BLOCK_NAME {
            result.push(0);
            continue;
        }
        check(
            record.network_hash == fact.network_hash && record.name.as_ref() == fact.name,
            "light source identity mismatch",
        )?;
        check(
            fact.emission <= 15 && fact.filter <= 15,
            "source light outside nibble range",
        )?;
        let mut value = fact.emission | fact.filter << 4;
        match record.name.as_ref() {
            "minecraft:portal" => value = 11,
            "minecraft:snow_layer" => value &= 15,
            "minecraft:ice" | "minecraft:frosted_ice" => value = value & 15 | 3 << 4,
            "minecraft:water" => value = value & 15 | 1 << 4,
            "minecraft:flowing_water" => value = value & 15 | 2 << 4,
            _ => {
                if let Some(emission) = emission(record)? {
                    value = value & 0xf0 | emission;
                }
            }
        }
        result.push(value);
    }
    Ok(result)
}
