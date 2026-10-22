//! Identifier checks for every committed asset and compiler data table.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    file: String,
    pointer: String,
    domain: String,
    identifier: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Exception {
    reference: Reference,
    reason: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Exceptions {
    schema: u32,
    entries: Vec<Exception>,
}

struct Vocabulary {
    domains: BTreeMap<String, BTreeSet<String>>,
}

impl Vocabulary {
    /// Loads positive registries and keeps pack, retail and legacy domains distinct.
    fn load(root: &Path) -> Self {
        let mut domains: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let text = include_str!("../../data/vanilla-pack-vocabulary-1.26.50.tsv");
        assert!(text.lines().zip(text.lines().skip(1)).all(|(a, b)| a < b));
        for row in text.lines() {
            let (domain, identifier) = row.split_once('\t').expect("vocabulary row");
            assert!(!identifier.is_empty() && !identifier.contains('\t'));
            assert!(
                domains
                    .entry(domain.into())
                    .or_default()
                    .insert(identifier.into())
            );
        }
        domains.insert(
            "item_tag".into(),
            include_str!("../../../protocol/data/item_tags_native.tsv")
                .lines()
                .filter(|row| !row.starts_with('#') && !row.is_empty())
                .map(|row| row.split_once('\t').unwrap().0.to_owned())
                .collect(),
        );
        domains.insert(
            "retail_item".into(),
            include_str!("../../../protocol/data/retail_items_1_26_50.tsv")
                .lines()
                .map(|row| row.split_once('\t').unwrap().1.to_owned())
                .collect(),
        );
        let actors: BTreeSet<String> =
            include_str!("../../../protocol/data/retail_actors_1_26_50.txt")
                .lines()
                .map(str::to_owned)
                .collect();
        assert_eq!(
            domains["actor"], actors,
            "actor vocabulary differs from its source projection"
        );
        // Fact carriers may include reviewed compatibility constructors beyond sample metadata.
        // Their declarations still undergo the independent actor-vocabulary check below.
        let policy = json(&root.join("crates/asset-compiler/data/registry-policy.json"));
        let mut fact_actors = actors.clone();
        fact_actors.extend(
            policy["entity_compatibility"]
                .as_array()
                .unwrap()
                .iter()
                .map(|name| name.as_str().unwrap().to_owned()),
        );
        domains.insert("entity_actor".into(), fact_actors);
        for (version, block_domain, property_domain) in [
            ("v1001", "legacy_block", "legacy_property"),
            ("v2193", "registry_block", "registry_property"),
        ] {
            let records =
                registry(&root.join(format!("crates/assets/data/block-registry-{version}.bin")));
            for record in &records {
                domains
                    .entry(block_domain.into())
                    .or_default()
                    .insert(record.name.to_string());
                let state: Value = serde_json::from_str(&record.canonical_state).unwrap();
                domains
                    .entry(property_domain.into())
                    .or_default()
                    .extend(state.as_object().unwrap().keys().cloned());
            }
        }
        let registry_blocks = domains["registry_block"].clone();
        domains.get_mut("block").unwrap().extend(registry_blocks);
        Self { domains }
    }

    /// Checks only the requested namespace; an actor name cannot validate an item field.
    fn contains(&self, reference: &Reference) -> bool {
        self.domains
            .get(&reference.domain)
            .is_some_and(|names| names.contains(&reference.identifier))
    }
}

/// Locates the checkout that owns the data being checked.
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Reads a data document without depending on installed packs or generated local carriers.
fn json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

/// Decodes a pinned block registry using the protocol in its checked header.
fn registry(path: &Path) -> Box<[assets::RegistryRecord]> {
    let bytes = fs::read(path).unwrap();
    let protocol = assets::registry_header_protocol(&bytes).unwrap();
    assets::read_registry_for_protocol(&bytes, protocol).unwrap()
}

/// Escapes one field name for an exact JSON pointer.
fn pointer(parent: &str, key: &str) -> String {
    format!("{parent}/{}", key.replace('~', "~0").replace('/', "~1"))
}

/// Adds one identifier occurrence with the domain required by its field.
fn add(refs: &mut BTreeSet<Reference>, file: &str, path: &str, domain: &str, identifier: &str) {
    refs.insert(Reference {
        file: file.into(),
        pointer: path.into(),
        domain: domain.into(),
        identifier: identifier.into(),
    });
}

/// Selects the registry namespace for a JSON table, failing when a new table has no policy.
fn table_domain(file: &str) -> &'static str {
    match file {
        "entity-components-1.26.50.json" => "actor",
        "block-properties-v2193.json" => "property",
        "item-use-profiles.json"
        | "item-armour-v2193.json"
        | "item-durability-v2193.json"
        | "default-sprite-bindings-1.26.50.json" => "retail_item",
        "block-item-routes-v1001.json"
        | "block-entities-v1001.json"
        | "visual-coverage-v1001.json"
        | "visual-invisible-v1001.json" => "legacy_block",
        "block-entities-v2193.json"
        | "block-item-routes-v2193.json"
        | "native-materials.json"
        | "block-family-routing.json"
        | "registry-policy.json" => "block",
        "beacon-tints.json" | "registry-schema.json" | "neutral-actor-materials-v1.json" => "block",
        _ => panic!("missing identifier policy for {file}"),
    }
}

/// Walks namespaced identities and state properties, including encoded canonical states.
fn walk(refs: &mut BTreeSet<Reference>, file: &str, value: &Value, path: &str, domain: &str) {
    match value {
        Value::Object(object) => {
            let state = path.ends_with("/block_state")
                || path.ends_with("/canonical_state")
                || (file == "block-entities-v1001.json"
                    && path.rsplit_once('/').is_some_and(|(parent, index)| {
                        parent.ends_with("/states") && index.parse::<usize>().is_ok()
                    }));
            for (key, value) in object {
                let child = pointer(path, key);
                let identity_map = matches!(
                    path,
                    "/entries" | "/frames" | "/equipment" | "/materials" | "/twins"
                );
                if state
                    || identity_map
                    || key.starts_with("minecraft:")
                    || key == "cinnabar:reserved"
                {
                    let key_domain = if file == "registry-policy.json" {
                        match path {
                            "/pack_rules/egg_aliases" | "/pack_rules/superseded_eggs" => "actor",
                            "/pack_rules/item_visuals" => "retail_item",
                            _ => domain,
                        }
                    } else {
                        domain
                    };
                    let key_domain = if state {
                        if domain == "legacy_block" {
                            "legacy_property"
                        } else {
                            "property"
                        }
                    } else {
                        key_domain
                    };
                    add(refs, file, &child, key_domain, key);
                }
                let child_domain = match key.as_str() {
                    "VanillaItem" if file == "registry-policy.json" => "retail_item",
                    "VanillaEntity" if file == "registry-policy.json" => "actor",
                    "state_property" | "state_properties" | "state_values" => "property",
                    "blocks" if file == "block-properties-v2193.json" => "block",
                    "identifier" if file.starts_with("block-item-routes-") => "item",
                    "entity_rules" | "entity_compatibility" if file == "registry-policy.json" => {
                        "actor"
                    }
                    "twin" if file == "registry-policy.json" => "legacy_block",
                    "item_rules" if file == "registry-policy.json" => "retail_item",
                    "item_icons" if file == "registry-policy.json" && path == "/presentation" => {
                        "retail_item"
                    }

                    "tag" if file == "registry-policy.json" && path.starts_with("/item_rules/") => {
                        "item_tag"
                    }
                    "duration_alias" => "behavior_item",
                    "default_alias" => "item_texture",
                    "canonical_state" => domain,
                    _ => domain,
                };
                let child_domain = if file == "registry-policy.json"
                    && path == "/pack_rules/egg_aliases"
                {
                    "retail_item"
                } else if file == "registry-policy.json" && path == "/pack_rules/superseded_eggs" {
                    "actor"
                } else {
                    child_domain
                };
                let child_domain = if file == "registry-policy.json" && key == "item" {
                    "retail_item"
                } else {
                    child_domain
                };
                let identity_field = (file == "registry-policy.json"
                    && path.starts_with("/item_rules/")
                    && key == "tag")
                    || matches!(
                        key.as_str(),
                        "identifier"
                            | "block_name"
                            | "twin"
                            | "item"
                            | "duration_alias"
                            | "default_alias"
                    )
                    || (key == "name"
                        && (file == "registry-policy.json"
                            || file == "block-properties-v2193.json"
                            || file == "visual-coverage-v1001.json"
                            || path.contains("/backing_blocks/")));
                if identity_field {
                    if let Some(identifier) = value.as_str() {
                        add(refs, file, &child, child_domain, identifier);
                    } else {
                        assert!(
                            key == "duration_alias" && value.is_null(),
                            "invalid identifier field in {file}{child}"
                        );
                    }
                } else if key == "canonical_state" {
                    let decoded: Value = serde_json::from_str(value.as_str().unwrap()).unwrap();
                    walk(refs, file, &decoded, &child, child_domain);
                } else if key == "material_names" {
                    for (i, name) in value.as_array().unwrap().iter().enumerate() {
                        add(
                            refs,
                            file,
                            &format!("{child}/{i}"),
                            "material",
                            name.as_str().unwrap(),
                        );
                    }
                } else {
                    walk(refs, file, value, &child, child_domain);
                }
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                let child = format!("{path}/{index}");
                if file == "registry-policy.json" && path.starts_with("/presentation/dimensions/") {
                    // Dimension names are wire-schema tokens; their fallback values are biome identities.
                    if index == 1 {
                        add(refs, file, &child, "biome", value.as_str().unwrap());
                    }
                    continue;
                }

                if (file == "registry-policy.json"
                    && path.starts_with("/item_rules/")
                    && path.ends_with("/members"))
                    || matches!(
                        path,
                        "/crossed_planes"
                            | "/unresolved_retail"
                            | "/movement_census/unmodeled"
                            | "/movement_census/unresolved"
                    )
                    || (path.is_empty() && file == "visual-invisible-v1001.json")
                {
                    add(refs, file, &child, domain, value.as_str().unwrap());
                } else {
                    walk(refs, file, value, &child, domain);
                }
            }
        }
        Value::String(name)
            if name.starts_with("minecraft:")
                || name == "cinnabar:reserved"
                || (file == "registry-policy.json"
                    && (path.contains("/members/")
                        || path.starts_with("/entity_compatibility/"))) =>
        {
            add(refs, file, path, domain, name);
        }
        _ => {}
    }
}

/// Expands the same colour/variant names as the compiler before checking their membership.
fn beacon_refs(refs: &mut BTreeSet<Reference>, document: &Value) {
    for (i, color) in document["colors"].as_array().unwrap().iter().enumerate() {
        let color = color["name"].as_str().unwrap();
        for (j, variant) in document["variants"].as_array().unwrap().iter().enumerate() {
            let variant = variant.as_str().unwrap();
            let name = match variant.strip_prefix("hard_") {
                Some(rest) => format!("minecraft:hard_{color}_{rest}"),
                None => format!("minecraft:{color}_{variant}"),
            };
            add(
                refs,
                "beacon-tints.json",
                &format!("/colors/{i}/variants/{j}"),
                "block",
                &name,
            );
        }
    }
}

/// Checks every TSV identity column and the sprite keys of retained legacy routes.
fn tsv_refs(refs: &mut BTreeSet<Reference>, file: &str, text: &str) {
    if file == "item-facts-expansion-v2193.tsv" {
        for (index, line) in text.lines().skip(1).enumerate() {
            let fields: Vec<_> = line.split('\t').collect();
            let domain = if fields.last() == Some(&"missing_retail_id") {
                "behavior_item"
            } else {
                "retail_item"
            };
            add(refs, file, &format!("/{index}/0"), domain, fields[0]);
        }
        return;
    }
    let domain = match file {
        "item-name-identities-1.26.50.tsv" => "item",
        "item-fuel-v2193.tsv"
        | "item-glint-v2193.tsv"
        | "item-grips-v2193.tsv"
        | "legacy-icon-routes-26.30.tsv" => "retail_item",
        "entity-facts-expansion-v2193.tsv" => "entity_actor",
        "server-defined-blocks-v2193.tsv"
        | "block-destroy-v2193.tsv"
        | "block-components-v2193.tsv"
        | "facts-expansion-v2193.tsv"
        | "block-placement-v2193.tsv" => "block",
        _ => panic!("missing identifier policy for {file}"),
    };
    for (i, line) in text
        .lines()
        .filter(|line| {
            !line.starts_with('#') && !line.starts_with("identifier\t") && !line.is_empty()
        })
        .enumerate()
    {
        let fields: Vec<_> = line.split('\t').collect();
        let domain = if file == "facts-expansion-v2193.tsv"
            && fields
                .get(3)
                .is_some_and(|value| value.split(',').any(|status| status == "historical-render"))
        {
            "legacy_block"
        } else {
            domain
        };
        add(refs, file, &format!("/{i}/0"), domain, fields[0]);
        if file == "block-placement-v2193.tsv" && fields[3] != "-" {
            add(refs, file, &format!("/{i}/3"), "block", fields[3]);
        }
        if file == "legacy-icon-routes-26.30.tsv" {
            add(refs, file, &format!("/{i}/2"), "item_texture", fields[2]);
        }
    }
}

/// Discovers all tables so adding a file cannot silently bypass vocabulary validation.
fn references(root: &Path) -> BTreeSet<Reference> {
    let mut refs = BTreeSet::new();
    for directory in ["crates/assets/data", "crates/asset-compiler/data"] {
        for entry in fs::read_dir(root.join(directory)).unwrap() {
            let path = entry.unwrap().path();
            let file = path.file_name().unwrap().to_str().unwrap();
            match path.extension().and_then(|ext| ext.to_str()).unwrap() {
                "json" if file == "vocabulary-exceptions.json" => {}
                "json" => {
                    let document = json(&path);
                    walk(&mut refs, file, &document, "", table_domain(file));
                    if file == "beacon-tints.json" {
                        beacon_refs(&mut refs, &document);
                    }
                }
                "tsv" if file == "vanilla-pack-vocabulary-1.26.50.tsv" => {}
                "txt" if file == "vocabulary-v2193.txt" => {}
                "tsv" => tsv_refs(&mut refs, file, &fs::read_to_string(&path).unwrap()),
                "bin" if file == assets::facts::entity::FILENAME => {
                    let facts = assets::facts::entity::EntityFacts::decode(
                        &fs::read(&path).unwrap(),
                        assets::active_content_registry_protocol(),
                    )
                    .unwrap();
                    for (index, record) in facts.records.iter().enumerate() {
                        add(
                            &mut refs,
                            file,
                            &format!("/{index}"),
                            "entity_actor",
                            &record.identifier,
                        );
                    }
                }
                // Light, physics and fallback carriers use checked block IDs, not identifier text.
                "bin" if file.starts_with("block-registry-") => {
                    let legacy = file.contains("v1001");
                    let records = registry(&path);
                    for record in &records {
                        let pointer = format!("/{}", record.sequential_id);
                        if record.name.as_ref() != "cinnabar:reserved" {
                            add(
                                &mut refs,
                                file,
                                &pointer,
                                if legacy { "legacy_block" } else { "block" },
                                &record.name,
                            );
                            let state: Value =
                                serde_json::from_str(&record.canonical_state).unwrap();
                            walk(
                                &mut refs,
                                file,
                                &state,
                                &format!("{pointer}/canonical_state"),
                                if legacy { "legacy_block" } else { "block" },
                            );
                        }
                    }
                }
                "bin" if file.starts_with("item-facts-") => {
                    let facts =
                        assets::facts::ItemFacts::decode(&fs::read(&path).unwrap()).unwrap();
                    for record in &facts.payload().records {
                        add(
                            &mut refs,
                            file,
                            &format!("/{}", record.network_id),
                            "retail_item",
                            &record.identifier,
                        );
                    }
                    for (index, tag) in facts.payload().tags.iter().enumerate() {
                        add(
                            &mut refs,
                            file,
                            &format!("/tags/{index}"),
                            "item_tag",
                            &tag.name,
                        );
                    }
                }
                "bin" if file.starts_with("biome-registry-") => {
                    for record in assets::read_biome_registry(&fs::read(&path).unwrap()).unwrap() {
                        add(
                            &mut refs,
                            file,
                            &format!("/{}", record.id),
                            "biome",
                            &record.name,
                        );
                    }
                }
                "bin"
                    if file.starts_with("block-light-registry-")
                        || file.starts_with("block-physics-")
                        || file.starts_with("vanilla-fallback-") => {}
                // Generated from the pinned GeyserMC mappings: Java ids, which are outside these
                // vocabularies, paired with Geyser's Bedrock ids, which include non-retail items
                // (command blocks, filled maps, `minecraft:air`) and item frames as entities.
                "bin" if file.starts_with("java-mapping-") => {}
                "sha256" => {}
                _ => panic!("unclassified asset data file {}", path.display()),
            }
        }
    }
    refs
}

/// Rejects missing identities and exceptions that no longer cover an actual failure.
fn failures(
    vocabulary: &Vocabulary,
    refs: &BTreeSet<Reference>,
    exceptions: &[Exception],
) -> Vec<String> {
    let mut allowed = BTreeSet::new();
    let mut errors = Vec::new();
    for exception in exceptions {
        if exception.reason.trim().is_empty() || !allowed.insert(exception.reference.clone()) {
            errors.push(format!(
                "empty or duplicate exception: {:?}",
                exception.reference
            ));
        }
        if !refs.contains(&exception.reference) || vocabulary.contains(&exception.reference) {
            errors.push(format!("stale exception: {:?}", exception.reference));
        }
    }
    for reference in refs {
        if !vocabulary.contains(reference) && !allowed.contains(reference) {
            errors.push(format!(
                "unknown {} identifier {} in {}{}",
                reference.domain, reference.identifier, reference.file, reference.pointer
            ));
        }
    }
    errors
}

#[test]
fn committed_data_identifiers_belong_to_their_pinned_vocabularies() {
    let root = root();
    let mut vocabulary = Vocabulary::load(&root);
    vocabulary.domains.insert(
        "biome".into(),
        include_str!("../../../protocol/data/retail_biomes_1_26_50.txt")
            .lines()
            .map(str::to_owned)
            .collect(),
    );
    let exceptions: Exceptions = serde_json::from_value(json(
        &root.join("crates/assets/data/vocabulary-exceptions.json"),
    ))
    .unwrap();
    assert_eq!(exceptions.schema, 1);
    let errors = failures(&vocabulary, &references(&root), &exceptions.entries);
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn pack_vocabulary_has_the_pinned_projection_fingerprint() {
    assert_eq!(
        format!(
            "{:x}",
            Sha256::digest(include_bytes!(
                "../../data/vanilla-pack-vocabulary-1.26.50.tsv"
            ))
        ),
        "0ff03cd80e7098ffc11ee0a443a94c2162155c2026f9c37ced615d5111c14d30"
    );
}

#[test]
fn wrong_domains_and_stale_exceptions_are_rejected() {
    let vocabulary = Vocabulary::load(&root());
    let document = json(&root().join("crates/asset-compiler/data/registry-policy.json"));
    let mut collected = BTreeSet::new();
    walk(
        &mut collected,
        "registry-policy.json",
        &document,
        "",
        "block",
    );
    let mut reference = collected
        .iter()
        .find(|reference| !vocabulary.domains["retail_item"].contains(&reference.identifier))
        .unwrap()
        .clone();
    reference.domain = "retail_item".into();
    let refs = BTreeSet::from([reference]);
    assert_eq!(failures(&vocabulary, &refs, &[]).len(), 1);
    let exception = Exception {
        reference: refs.first().unwrap().clone(),
        reason: "Retained compatibility entry.".into(),
    };
    assert!(failures(&vocabulary, &refs, &[exception]).is_empty());
    let exception = Exception {
        reference: refs.first().unwrap().clone(),
        reason: "Retained compatibility entry.".into(),
    };
    assert_eq!(
        failures(&vocabulary, &BTreeSet::new(), &[exception]).len(),
        1
    );
}

#[test]
fn twin_items_are_checked_against_the_positive_retail_surface() {
    let vocabulary = Vocabulary::load(&root());
    let mut document = json(&root().join("crates/asset-compiler/data/registry-policy.json"));
    let non_item = vocabulary.domains["block"]
        .iter()
        .find(|name| {
            name.starts_with("minecraft:") && !vocabulary.domains["retail_item"].contains(*name)
        })
        .unwrap();
    let twin = document["twins"]
        .as_object_mut()
        .unwrap()
        .values_mut()
        .next()
        .unwrap();
    twin["item"] = Value::String(non_item.clone());
    let mut refs = BTreeSet::new();
    walk(&mut refs, "registry-policy.json", &document, "", "block");
    assert!(
        refs.iter()
            .any(|reference| reference.domain == "retail_item"
                && reference.identifier == *non_item
                && !vocabulary.contains(reference))
    );
}

#[test]
fn every_pinned_actor_has_an_explicit_interaction_disposition() {
    let vocabulary = Vocabulary::load(&root());
    let document = json(&root().join("crates/asset-compiler/data/registry-policy.json"));
    let declared: BTreeSet<_> = document["entity_rules"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|rule| rule["facts"].get("interactions").is_some())
        .flat_map(|rule| {
            rule["members"]
                .as_array()
                .unwrap()
                .iter()
                .map(|member| member.as_str().unwrap())
        })
        .collect();
    for actor in &vocabulary.domains["actor"] {
        assert!(
            declared.contains(actor.as_str()),
            "missing explicit disposition for {actor}"
        );
        assert_ne!(
            assets::facts::entity::EntityFacts::pinned()
                .get(actor)
                .class_key,
            0,
            "missing actor {actor}"
        );
    }
}

#[test]
fn an_unqualified_actor_member_cannot_disappear_from_validation() {
    let vocabulary = Vocabulary::load(&root());
    let mut document = json(&root().join("crates/asset-compiler/data/registry-policy.json"));
    let actor = document["entity_rules"][0]["members"][0]
        .as_str()
        .unwrap()
        .to_owned();
    let bare = actor.strip_prefix("minecraft:").unwrap().to_owned();
    document["entity_rules"][0]["members"][0] = Value::String(bare.clone());
    let mut refs = BTreeSet::new();
    walk(&mut refs, "registry-policy.json", &document, "", "block");
    assert!(
        refs.iter()
            .any(|reference| reference.identifier == bare && !vocabulary.contains(reference))
    );
}

#[test]
fn facts_migration_presentation_references_use_item_block_and_biome_domains() {
    let root = root();
    let mut vocabulary = Vocabulary::load(&root);
    vocabulary.domains.insert(
        "biome".into(),
        include_str!("../../../protocol/data/retail_biomes_1_26_50.txt")
            .lines()
            .map(str::to_owned)
            .collect(),
    );
    let document = json(&root.join("crates/asset-compiler/data/registry-policy.json"));
    let mut collected = BTreeSet::new();
    walk(
        &mut collected,
        "registry-policy.json",
        &document["presentation"],
        "/presentation",
        "block",
    );
    for domain in ["retail_item", "block", "biome"] {
        assert!(collected.iter().any(|reference| reference.domain == domain));
    }
    let errors = failures(&vocabulary, &collected, &[]);
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn pack_rules_keep_actor_and_item_vocabulary_domains_distinct() {
    let vocabulary = Vocabulary::load(&root());
    let document = serde_json::json!({"pack_rules": {
        "egg_aliases": {"minecraft:evocation_illager": "minecraft:evoker_spawn_egg"},
        "superseded_eggs": {"minecraft:villager": "minecraft:villager_v2"},
        "item_visuals": {"minecraft:banner": "BlockModel"}
    }});
    let mut refs = BTreeSet::new();
    walk(&mut refs, "registry-policy.json", &document, "", "block");
    assert_eq!(refs.len(), 5);
    assert!(refs.iter().all(|reference| vocabulary.contains(reference)));
    assert_eq!(
        refs.iter()
            .filter(|reference| reference.domain == "actor")
            .count(),
        3
    );
    assert_eq!(
        refs.iter()
            .filter(|reference| reference.domain == "retail_item")
            .count(),
        2
    );
}
