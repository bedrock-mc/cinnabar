use super::{BlockFact, Policy, RESERVED_BLOCK_NAME, Result, Scalar, check, manifest::Projection};
use assets::registry::{
    BeaconTint, BlockFlags, CollisionConfidence, CollisionSeed, ContributorRole, ModelFamily,
    ModelState, ModelStateField, RegistryProvenance, RegistryRecord,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// Constructs only source identity fields; every render fact must be admitted separately.
fn identity(f: &BlockFact) -> RegistryRecord {
    RegistryRecord {
        sequential_id: f.sequential_id,
        network_hash: f.network_hash,
        name: f.name.clone().into(),
        canonical_state: f.state.clone().into(),
        flags: BlockFlags::empty(),
        model_family: ModelFamily::Unknown,
        contributor_role: ContributorRole::Primary,
        model_state: ModelState::default(),
        face_coverage: 0,
        collision_seed: CollisionSeed::default(),
        provenance: RegistryProvenance::DRAGONFLY,
        beacon_tint: BeaconTint::Unresolved,
        traits: assets::registry::BlockTraits::empty(),
        survival: assets::registry::SurvivalRule::None,
        properties: Box::new([]),
    }
}

/// Expands the shared eligibility table into source block names; unlisted names do not tint.
fn beacon_tints() -> Result<BTreeMap<String, BeaconTint>> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Color {
        name: String,
        tint: u8,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Table {
        schema: u32,
        colors: Vec<Color>,
        variants: Vec<String>,
    }
    let table: Table =
        serde_json::from_slice(include_bytes!("../../../assets/data/beacon-tints.json"))?;
    check(
        table.schema == 1 && !table.colors.is_empty() && !table.variants.is_empty(),
        "beacon tint table is empty or has an unsupported version",
    )?;
    let mut tints = BTreeMap::new();
    for color in &table.colors {
        let tint = BeaconTint::read(color.tint)
            .ok()
            .filter(|t| t.dye_colour().is_some() && !color.name.is_empty())
            .ok_or("beacon tint table has an invalid color")?;
        for variant in &table.variants {
            let name = match variant.strip_prefix("hard_") {
                Some(rest) => format!("minecraft:hard_{}_{rest}", color.name),
                None => format!("minecraft:{}_{variant}", color.name),
            };
            check(tints.insert(name, tint).is_none(), "duplicate beacon tint")?;
        }
    }
    Ok(tints)
}

/// Forms the historical canonical lookup and fingerprint key.
fn key(name: &str, state: &str) -> String {
    format!("{name}\0{state}")
}

/// Removes only neighbour-derived properties from a canonical source state.
fn reduced(state: &str) -> Result<String> {
    let mut fields: BTreeMap<String, serde_json::Value> = serde_json::from_str(state)?;
    fields
        .retain(|name, _| !name.starts_with("minecraft:connection_") && name != "minecraft:corner");
    Ok(serde_json::to_string(&fields)?)
}

/// Fingerprints denied source identities before their render disposition is neutralized.
fn fingerprint(hash: &mut Sha256, f: &BlockFact) {
    hash.update(f.sequential_id.to_le_bytes());
    hash.update(f.network_hash.to_le_bytes());
    hash.update(key(&f.name, &f.state).as_bytes());
    hash.update([0]);
}

/// Converts exact source boxes into the existing fixed-point carrier coordinates.
pub(super) fn collision(f: &BlockFact) -> Result<CollisionSeed> {
    let mut boxes = Vec::new();
    for raw in f.boxes.iter().flatten() {
        let mut v = [0i32; 6];
        for (i, value) in raw.iter().enumerate() {
            let scaled = (value * assets::block_shapes::COLLISION_UNITS_PER_BLOCK as f64).round();
            check(
                scaled.is_finite() && (-100_000_000.0..=200_000_000.0).contains(&scaled),
                "source collision coordinate outside halo",
            )?;
            v[i] = scaled as i32;
        }
        check(
            v[0] <= v[3] && v[1] <= v[4] && v[2] <= v[5],
            "inverted source collision",
        )?;
        boxes.push(assets::CollisionBox {
            min_x: v[0],
            min_y: v[1],
            min_z: v[2],
            max_x: v[3],
            max_y: v[4],
            max_z: v[5],
        });
    }
    check(
        boxes.len() <= super::format_generated::MAX_COLLISION_BOXES,
        "source collision exceeds carrier box count",
    )?;
    Ok(CollisionSeed {
        shape_id: f.shape_id,
        confidence: CollisionConfidence::CollisionOnly,
        boxes: boxes.into(),
    })
}

/// Admits supported Education blocks using their explicit state schemas.
fn education(f: &BlockFact) -> Result<Option<RegistryRecord>> {
    if !matches!(
        f.name.as_str(),
        "minecraft:allow" | "minecraft:deny" | "minecraft:border_block" | "minecraft:chalkboard"
    ) {
        return Ok(None);
    }
    let fields: BTreeMap<String, Scalar> = serde_json::from_str(&f.state)?;
    let mut record = identity(f);
    if f.name == "minecraft:chalkboard" {
        let direction = fields
            .get("direction")
            .ok_or("chalkboard direction missing")?;
        check(
            fields.len() == 1
                && direction.kind == "int"
                && direction.value.as_u64().is_some_and(|value| value < 16),
            "invalid chalkboard state",
        )?;
        return Ok(Some(record));
    }
    if f.name != "minecraft:border_block" {
        check(fields.is_empty(), "Education cube has unexpected state")?;
        record.flags = BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE;
        record.model_family = ModelFamily::Cube;
        record.face_coverage = 0x3f;
        return Ok(Some(record));
    }
    check(fields.len() == 5, "Education border state count mismatch")?;
    let mut connections = 0;
    for (i, direction) in ["north", "east", "south", "west"].iter().enumerate() {
        let field = fields
            .get(&format!("wall_connection_type_{direction}"))
            .ok_or("Education connection missing")?;
        check(field.kind == "string", "Education connection type mismatch")?;
        let value = match field.value.as_str() {
            Some("none") => 0,
            Some("short") => 1,
            Some("tall") => 2,
            _ => return Err("Education connection unknown".into()),
        };
        connections |= value << (2 * i);
    }
    let post = fields
        .get("wall_post_bit")
        .ok_or("Education post bit missing")?;
    let bit = post.value.as_u64().ok_or("invalid Education post bit")?;
    check(
        post.kind == "byte" && bit <= 1,
        "invalid Education post bit",
    )?;
    connections |= (bit as u32) << 8;
    record.model_family = ModelFamily::Wall;
    record
        .model_state
        .set(ModelStateField::Connections, connections);
    Ok(Some(record))
}

/// Applies client admission while preserving every source ID and network hash.
pub(super) fn project(
    source: &[BlockFact],
    legacy: &[RegistryRecord],
    allowed: &BTreeSet<String>,
    policy: &Policy,
) -> Result<(Vec<RegistryRecord>, Projection)> {
    let old: BTreeMap<_, _> = legacy
        .iter()
        .filter(|r| r.name.as_ref() != RESERVED_BLOCK_NAME)
        .map(|r| (key(&r.name, &r.canonical_state), r))
        .collect();
    let mut stats = Projection {
        states: source.len(),
        twin_names: policy.twins.len(),
        unresolved_retail: policy.unresolved_retail.clone(),
        ..Default::default()
    };
    let mut denied = Sha256::new();
    let mut twins = Sha256::new();
    for (name, twin) in &policy.twins {
        twins.update(format!("{name}\0{}\0{}\0", twin.twin, twin.item).as_bytes());
    }
    stats.twin_fingerprint = format!("{:x}", twins.finalize());
    #[derive(Deserialize)]
    struct Routes {
        schema: u32,
        crossed_planes: Vec<String>,
    }
    let routes: Routes = serde_json::from_slice(include_bytes!(
        "../../../assets/data/block-family-routing.json"
    ))?;
    check(
        routes.schema == 1
            && !routes.crossed_planes.is_empty()
            && routes.crossed_planes.windows(2).all(|p| p[0] < p[1]),
        "invalid block-family routing catalog",
    )?;
    let tints = beacon_tints()?;
    let mut records = Vec::with_capacity(source.len());
    for f in source {
        let mut record = if let Some(record) = education(f)? {
            stats.education_states += 1;
            record
        } else if allowed.contains(&f.name)
            && policy.block_rules.iter().any(|rule| {
                rule.fact == "render:AuthoredModel"
                    && serde_json::from_str(&f.state)
                        .is_ok_and(|state| rule.matches(&f.name, &state, ModelFamily::Unknown))
            })
        {
            stats.authored_states += 1;
            identity(f)
        } else {
            let exact = old.get(&key(&f.name, &f.state));
            let reduced = reduced(&f.state)?;
            let reduced_match = old.get(&key(&f.name, &reduced));
            let adopted = if let Some(record) = exact {
                stats.legacy_exact_states += 1;
                Some(*record)
            } else if let Some(record) = reduced_match {
                stats.legacy_reduced_states += 1;
                Some(*record)
            } else {
                let twin = policy.twins.get(&f.name);
                let item = twin.map_or(f.name.as_str(), |t| t.item.as_str());
                if !allowed.contains(item) || policy.unresolved_retail.contains(&f.name) {
                    None
                } else if let Some(twin) = twin {
                    stats.twin_states += 1;
                    Some(
                        *old.get(&key(&twin.twin, &reduced))
                            .ok_or("reviewed twin has no legacy state")?,
                    )
                } else {
                    return Err(
                        format!("retail state {} has no reviewed render facts", f.name).into(),
                    );
                }
            };
            if let Some(old) = adopted {
                let mut record = (*old).clone();
                record.sequential_id = f.sequential_id;
                record.network_hash = f.network_hash;
                record.name = f.name.clone().into();
                record.canonical_state = f.state.clone().into();
                record
            } else {
                stats.denied_count += 1;
                fingerprint(&mut denied, f);
                let mut record = identity(f);
                record.name = RESERVED_BLOCK_NAME.into();
                record.canonical_state = format!(
                    "{{\"reserved_id\":{{\"type\":\"int\",\"value\":{}}}}}",
                    f.sequential_id
                )
                .into();
                record
            }
        };
        if record.name.as_ref() != RESERVED_BLOCK_NAME {
            record.model_family =
                super::block_rules::family(&policy.model_family_rules, f, record.model_family)?;
            record.collision_seed = collision(f)?;
        }
        if routes
            .crossed_planes
            .binary_search_by(|name| name.as_str().cmp(&record.name))
            .is_ok()
        {
            record.model_family = ModelFamily::Cross;
        }
        record.beacon_tint = tints.get(&f.name).copied().unwrap_or(BeaconTint::NoTinting);
        records.push(record);
    }
    check(
        stats.education_states == 2 + 3usize.pow(4) * 2 + 16,
        "Education construction palette count mismatch",
    )?;
    check(
        stats.states
            == stats.legacy_exact_states
                + stats.legacy_reduced_states
                + stats.authored_states
                + stats.twin_states
                + stats.denied_count
                + stats.education_states,
        "projection classifications do not account for every source state",
    )?;
    stats.denied_fingerprint = format!("{:x}", denied.finalize());
    Ok((records, stats))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chalkboard_admission_preserves_every_direction_without_cube_flags() {
        for direction in 0..16 {
            let fact = BlockFact {
                sequential_id: direction,
                network_hash: direction + 100,
                name: "minecraft:chalkboard".into(),
                state: format!(r#"{{"direction":{{"type":"int","value":{direction}}}}}"#),
                shape_id: 0,
                boxes: None,
                friction: 0.6,
                emission: 0,
                filter: 0,
                can_contain_liquid_source: false,
            };
            let record = education(&fact)
                .unwrap()
                .expect("chalkboard states must not be reserved placeholders");
            assert_eq!(record.name.as_ref(), fact.name);
            assert_eq!(record.canonical_state.as_ref(), fact.state);
            assert_eq!(record.sequential_id, direction);
            assert!(!record.flags.contains(BlockFlags::CUBE_GEOMETRY));
        }
    }
}
