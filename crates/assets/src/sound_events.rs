//! Parsed `sounds.json` routing: event names to sound-definition names with volume/pitch ranges.
//!
//! Block, entity, individual and interactive (per-material footstep) sections share one route
//! model. An explicit empty sound string is silence, distinct from an absent event.

use std::collections::HashMap;

use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloatRange {
    pub min: f32,
    pub max: f32,
}

impl FloatRange {
    pub const ONE: Self = Self { min: 1.0, max: 1.0 };

    /// Value at `unit` in `[0, 1]` between the bounds.
    pub fn sample(self, unit: f32) -> f32 {
        self.min + (self.max - self.min) * unit.clamp(0.0, 1.0)
    }

    fn scaled(self, other: Self) -> Self {
        Self {
            min: self.min * other.min,
            max: self.max * other.max,
        }
    }

    fn from_json(value: &Value) -> Option<Self> {
        let number = |value: &Value| value.as_f64().map(|n| n as f32).filter(|n| n.is_finite());
        match value {
            Value::Array(pair) if pair.len() == 2 => {
                let (a, b) = (number(&pair[0])?, number(&pair[1])?);
                Some(Self {
                    min: a.min(b),
                    max: a.max(b),
                })
            }
            other => number(other).map(|n| Self { min: n, max: n }),
        }
    }
}

/// A resolved sound-definition name with its volume/pitch ranges.
#[derive(Clone, Debug, PartialEq)]
pub struct SoundRoute {
    pub sound: Box<str>,
    pub volume: FloatRange,
    pub pitch: FloatRange,
}

#[derive(Clone, Debug, PartialEq)]
enum Entry {
    Silent,
    Route(SoundRoute),
    /// Per-material alternatives keyed by material name plus `default`.
    ByMaterial(HashMap<Box<str>, Option<SoundRoute>>),
}

#[derive(Clone, Debug, PartialEq)]
struct EventSet {
    volume: FloatRange,
    pitch: FloatRange,
    events: HashMap<Box<str>, Entry>,
}

impl Default for EventSet {
    fn default() -> Self {
        Self {
            volume: FloatRange::ONE,
            pitch: FloatRange::ONE,
            events: HashMap::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct EntitySet {
    base: EventSet,
    variants: HashMap<Box<str>, EventSet>,
}

/// `sounds.json` routing plus the `blocks.json` block-to-material map.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SoundEventTables {
    blocks: HashMap<Box<str>, EventSet>,
    entity_defaults: EventSet,
    entities: HashMap<Box<str>, EntitySet>,
    individual: HashMap<Box<str>, Option<SoundRoute>>,
    interactive_blocks: HashMap<Box<str>, EventSet>,
    interactive_defaults: EventSet,
    interactive_entities: HashMap<Box<str>, EventSet>,
    materials: HashMap<Box<str>, Box<str>>,
}

fn route(value: &Value) -> Option<SoundRoute> {
    match value {
        Value::String(name) if !name.is_empty() => Some(SoundRoute {
            sound: name.as_str().into(),
            volume: FloatRange::ONE,
            pitch: FloatRange::ONE,
        }),
        Value::Object(map) => {
            let sound = map.get("sound")?.as_str().filter(|name| !name.is_empty())?;
            Some(SoundRoute {
                sound: sound.into(),
                volume: map
                    .get("volume")
                    .and_then(FloatRange::from_json)
                    .unwrap_or(FloatRange::ONE),
                pitch: map
                    .get("pitch")
                    .and_then(FloatRange::from_json)
                    .unwrap_or(FloatRange::ONE),
            })
        }
        _ => None,
    }
}

fn entry(value: &Value) -> Entry {
    if let Some(found) = route(value) {
        return Entry::Route(found);
    }
    match value {
        Value::Object(map) if !map.contains_key("sound") && !map.is_empty() => Entry::ByMaterial(
            map.iter()
                .map(|(material, value)| (material.as_str().into(), route(value)))
                .collect(),
        ),
        _ => Entry::Silent,
    }
}

fn event_set(value: &Value) -> Option<EventSet> {
    let map = value.as_object()?;
    let mut set = EventSet::default();
    if let Some(range) = map.get("volume").and_then(FloatRange::from_json) {
        set.volume = range;
    }
    if let Some(range) = map.get("pitch").and_then(FloatRange::from_json) {
        set.pitch = range;
    }
    if let Some(Value::Object(events)) = map.get("events") {
        set.events = events
            .iter()
            .map(|(name, value)| (name.as_str().into(), entry(value)))
            .collect();
    }
    Some(set)
}

fn set_map(value: Option<&Value>) -> HashMap<Box<str>, EventSet> {
    value
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(name, value)| Some((name.as_str().into(), event_set(value)?)))
        .collect()
}

fn entity_map(value: Option<&Value>) -> HashMap<Box<str>, EntitySet> {
    value
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(name, value)| {
            let base = event_set(value)?;
            let variants = value
                .pointer("/variants/map")
                .map(|map| set_map(Some(map)))
                .unwrap_or_default();
            Some((name.as_str().into(), EntitySet { base, variants }))
        })
        .collect()
}

fn bare(identifier: &str) -> &str {
    identifier.strip_prefix("minecraft:").unwrap_or(identifier)
}

/// Outcome of an event lookup; `Silent` is an explicit empty sound, `Absent` means no entry.
#[derive(Clone, Debug, PartialEq)]
pub enum RouteLookup {
    Absent,
    Silent,
    Route(SoundRoute),
}

impl RouteLookup {
    pub fn route(self) -> Option<SoundRoute> {
        match self {
            Self::Route(route) => Some(route),
            _ => None,
        }
    }
}

fn lookup(set: &EventSet, event: &str, material: Option<&str>) -> RouteLookup {
    let Some(found) = set.events.get(event).or_else(|| set.events.get("default")) else {
        return RouteLookup::Absent;
    };
    let base = match found {
        Entry::Silent => return RouteLookup::Silent,
        Entry::Route(route) => route.clone(),
        Entry::ByMaterial(map) => {
            match map
                .get(material.unwrap_or("default"))
                .or_else(|| map.get("default"))
                .cloned()
                .flatten()
            {
                Some(route) => route,
                None => return RouteLookup::Silent,
            }
        }
    };
    RouteLookup::Route(SoundRoute {
        sound: base.sound,
        volume: base.volume.scaled(set.volume),
        pitch: base.pitch.scaled(set.pitch),
    })
}

fn resolve(set: &EventSet, event: &str, material: Option<&str>) -> Option<SoundRoute> {
    lookup(set, event, material).route()
}

impl SoundEventTables {
    /// Parses `sounds.json` (already JSONC-normalized) and a `block -> material` map.
    #[allow(clippy::field_reassign_with_default)]
    pub fn from_json(sounds: &Value, block_materials: &Value) -> Self {
        let mut tables = Self::default();
        tables.blocks = set_map(sounds.get("block_sounds"));
        if let Some(defaults) = sounds
            .pointer("/entity_sounds/defaults")
            .and_then(event_set)
        {
            tables.entity_defaults = defaults;
        }
        tables.entities = entity_map(sounds.pointer("/entity_sounds/entities"));
        for section in [
            "/individual_event_sounds/events",
            "/individual_named_sounds/sounds",
        ] {
            if let Some(Value::Object(events)) = sounds.pointer(section) {
                for (name, value) in events {
                    if let Some(found) = route(value) {
                        tables.individual.insert(name.as_str().into(), Some(found));
                    } else if value.as_str() == Some("")
                        || value.get("sound").and_then(Value::as_str) == Some("")
                    {
                        tables.individual.insert(name.as_str().into(), None);
                    }
                }
            }
        }
        tables.interactive_blocks = set_map(sounds.pointer("/interactive_sounds/block_sounds"));
        if let Some(defaults) = sounds
            .pointer("/interactive_sounds/entity_sounds/defaults")
            .and_then(event_set)
        {
            tables.interactive_defaults = defaults;
        }
        tables.interactive_entities =
            set_map(sounds.pointer("/interactive_sounds/entity_sounds/entities"));
        if let Value::Object(map) = block_materials {
            tables.materials = map
                .iter()
                .filter_map(|(block, material)| {
                    Some((bare(block).into(), material.as_str()?.into()))
                })
                .collect();
        }
        tables
    }

    /// Later tables replace same-named entries; used for server pack `sounds.json`.
    pub fn merge(&mut self, later: Self) {
        self.blocks.extend(later.blocks);
        self.entities.extend(later.entities);
        self.individual.extend(later.individual);
        self.interactive_blocks.extend(later.interactive_blocks);
        self.interactive_entities.extend(later.interactive_entities);
        self.materials.extend(later.materials);
        if !later.entity_defaults.events.is_empty() {
            self.entity_defaults = later.entity_defaults;
        }
        if !later.interactive_defaults.events.is_empty() {
            self.interactive_defaults = later.interactive_defaults;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty() && self.entities.is_empty() && self.individual.is_empty()
    }

    /// Sound material name (`stone`, `wood`, ...) of a block identifier.
    pub fn material_of(&self, block_identifier: &str) -> Option<&str> {
        // Vanilla loads block textures and `sound` from the same
        // blocks.json entry. The pack
        // still calls grass_block `grass`; apply the shared texture alias
        // after the exact entry, without inventing a default sound material.
        self.materials
            .get(bare(block_identifier))
            .or_else(|| {
                self.materials
                    .get(crate::legacy_resource_pack_block_alias(block_identifier)?)
            })
            .map(AsRef::as_ref)
    }

    /// Break/place/hit/etc. route of a block sound material.
    pub fn block(&self, material: &str, event: &str) -> Option<SoundRoute> {
        resolve(self.blocks.get(material)?, event, None)
    }

    /// Route of an actor event; `variant` selects a `variants.map` entry when the base lacks it.
    pub fn entity(
        &self,
        identifier: &str,
        event: &str,
        variant: Option<&str>,
    ) -> Option<SoundRoute> {
        self.entity_lookup(identifier, event, variant).route()
    }

    /// Like [`Self::entity`] but distinguishing an explicit silent entry from no entry.
    pub fn entity_lookup(
        &self,
        identifier: &str,
        event: &str,
        variant: Option<&str>,
    ) -> RouteLookup {
        let Some(entity) = self.entities.get(bare(identifier)) else {
            return lookup(&self.entity_defaults, event, None);
        };
        let variant_set = variant
            .and_then(|key| entity.variants.get(key))
            .or_else(|| entity.variants.get("default"));
        if let Some(set) = variant_set.filter(|set| set.events.contains_key(event)) {
            return lookup(set, event, None);
        }
        if entity.base.events.contains_key(event) {
            return lookup(&entity.base, event, None);
        }
        match lookup(&self.entity_defaults, event, None) {
            RouteLookup::Route(found) => RouteLookup::Route(SoundRoute {
                volume: found.volume.scaled(entity.base.volume),
                pitch: found.pitch.scaled(entity.base.pitch),
                ..found
            }),
            other => other,
        }
    }

    /// Footstep/fall/jump/land route of an actor moving over a block sound material. Block routes are
    /// scaled by the actor's interactive volume/pitch (the shared default when it has none).
    pub fn interactive(&self, entity: &str, event: &str, material: &str) -> Option<SoundRoute> {
        let explicit = self.interactive_entities.get(bare(entity));
        if let Some(set) = explicit.filter(|set| set.events.contains_key(event)) {
            return resolve(set, event, Some(material));
        }
        let scale = explicit.unwrap_or(&self.interactive_defaults);
        if let Some(set) = self
            .interactive_blocks
            .get(material)
            .filter(|set| set.events.contains_key(event))
        {
            return resolve(set, event, Some(material)).map(|route| SoundRoute {
                volume: route.volume.scaled(scale.volume),
                pitch: route.pitch.scaled(scale.pitch),
                ..route
            });
        }
        resolve(&self.interactive_defaults, event, Some(material))
    }

    /// Distinguishes explicit individual silence from an absent event before fallback routing.
    pub fn individual_lookup(&self, event: &str) -> RouteLookup {
        match self.individual.get(event) {
            Some(Some(route)) => RouteLookup::Route(route.clone()),
            Some(None) => RouteLookup::Silent,
            None => RouteLookup::Absent,
        }
    }

    /// Named individual sound event (`bucket.fill.water`, `random.click`, ...).
    pub fn individual(&self, event: &str) -> Option<&SoundRoute> {
        self.individual.get(event).and_then(Option::as_ref)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn review_explicit_individual_silence_overrides_an_earlier_route() {
        for silent in [serde_json::json!(""), serde_json::json!({"sound":""})] {
            let mut base = super::SoundEventTables::from_json(
                &serde_json::json!({"individual_event_sounds":{"events":{"click":"base.click"}}}),
                &serde_json::json!({}),
            );
            let later = super::SoundEventTables::from_json(
                &serde_json::json!({"individual_event_sounds":{"events":{"click":silent}}}),
                &serde_json::json!({}),
            );
            base.merge(later);
            assert!(base.individual("click").is_none());
            assert_eq!(base.individual_lookup("click"), super::RouteLookup::Silent);
            assert_eq!(
                base.individual_lookup("missing"),
                super::RouteLookup::Absent
            );
        }
    }

    use super::*;
    use serde_json::json;

    #[test]
    fn level_event_silence_prevents_the_default_sound() {
        let absent = SoundEventTables::default();
        assert_eq!(
            level_event_sound_route(&absent, 1000, 0.0).unwrap().sound.as_ref(),
            "random.click"
        );
        let silent = SoundEventTables::from_json(
            &json!({"individual_event_sounds":{"events":{"block.click":""}}}),
            &json!({}),
        );
        assert!(level_event_sound_route(&silent, 1000, 0.0).is_none());
        let custom = SoundEventTables::from_json(
            &json!({"individual_event_sounds":{"events":{"block.click":{
                "sound":"custom.click", "volume":0.25, "pitch":[0.5,0.75]
            }}}}),
            &json!({}),
        );
        let route = level_event_sound_route(&custom, 1000, 0.0).unwrap();
        assert_eq!(route.sound.as_ref(), "custom.click");
        assert_eq!(route.volume.sample(0.0), 0.25);
        assert_eq!(route.pitch.sample(1.0), 0.75);
    }

    fn tables() -> SoundEventTables {
        SoundEventTables::from_json(
            &json!({
                "block_sounds": {"stone": {"volume": 0.5, "pitch": [1.0, 2.0], "events": {
                    "break": {"sound": "dig.stone", "volume": 0.8},
                    "default": ""}}},
                "entity_sounds": {
                    "defaults": {"events": {"hurt": "game.hurt", "eat": {"sound": "random.eat"}}},
                    "entities": {"cow": {"pitch": [0.8, 1.2], "events": {"hurt": "mob.cow.hurt", "step": ""}}}
                },
                "individual_event_sounds": {"events": {"random.click": {"sound": "random.click"}}},
                "interactive_sounds": {"block_sounds": {"stone": {"events": {
                    "step": {"sound": "step.stone", "volume": 0.3}}}}}
            }),
            &json!({"minecraft:stone": "stone", "dirt": "gravel"}),
        )
    }

    #[test]
    fn block_route_scales_by_set_ranges_and_default_is_silent() {
        let route = tables().block("stone", "break").expect("break route");
        assert_eq!(&*route.sound, "dig.stone");
        assert!((route.volume.min - 0.4).abs() < 1e-6);
        assert_eq!(route.pitch, FloatRange { min: 1.0, max: 2.0 });
        assert!(tables().block("stone", "place").is_none());
    }

    #[test]
    fn entity_override_beats_defaults_and_empty_string_is_silence() {
        let tables = tables();
        assert_eq!(
            &*tables.entity("minecraft:cow", "hurt", None).unwrap().sound,
            "mob.cow.hurt"
        );
        assert!(tables.entity("cow", "step", None).is_none());
        let eat = tables.entity("minecraft:cow", "eat", None).unwrap();
        assert_eq!(eat.pitch, FloatRange { min: 0.8, max: 1.2 });
        assert_eq!(
            &*tables.entity("pig", "hurt", None).unwrap().sound,
            "game.hurt"
        );
    }

    #[test]
    fn material_and_interactive_lookup() {
        let tables = tables();
        assert_eq!(tables.material_of("minecraft:stone"), Some("stone"));
        let step = tables.interactive("player", "step", "stone").unwrap();
        assert_eq!(&*step.sound, "step.stone");
        assert!(tables.individual("random.click").is_some());
        let mut scaled = tables.clone();
        scaled.interactive_defaults.volume = FloatRange { min: 0.5, max: 0.5 };
        let quiet = scaled.interactive("player", "step", "stone").unwrap();
        assert!((quiet.volume.min - 0.15).abs() < 1e-6);
    }

    #[test]
    fn legacy_block_materials_resolve_canonical_names_without_custom_fallbacks() {
        let tables = SoundEventTables::from_json(
            &json!({}),
            &json!({"grass": "grass", "dirt": "gravel", "chain": "metal"}),
        );
        assert_eq!(tables.material_of("minecraft:grass_block"), Some("grass"));
        assert_eq!(tables.material_of("grass_block"), Some("grass"));
        assert_eq!(tables.material_of("minecraft:grass"), Some("grass"));
        assert_eq!(tables.material_of("minecraft:dirt"), Some("gravel"));
        assert_eq!(tables.material_of("minecraft:iron_chain"), Some("metal"));
        assert_eq!(tables.material_of("example:grass_block"), None);
        assert_eq!(tables.material_of("minecraft:unknown_block"), None);
    }

    #[test]
    fn exact_material_and_server_overrides_win_over_legacy_aliases() {
        let mut tables = SoundEventTables::from_json(
            &json!({}),
            &json!({"grass": "grass", "grass_block": "modern_grass"}),
        );
        assert_eq!(
            tables.material_of("minecraft:grass_block"),
            Some("modern_grass")
        );
        tables.merge(SoundEventTables::from_json(
            &json!({}),
            &json!({"minecraft:grass_block": ""}),
        ));
        assert_eq!(tables.material_of("minecraft:grass_block"), Some(""));

        let legacy_only = SoundEventTables::from_json(&json!({}), &json!({"grass": "grass"}));
        let mut overridden = legacy_only;
        overridden.merge(SoundEventTables::from_json(
            &json!({}),
            &json!({"grass": "custom_grass"}),
        ));
        assert_eq!(
            overridden.material_of("minecraft:grass_block"),
            Some("custom_grass")
        );
    }

    #[test]
    fn merge_replaces_named_entries() {
        let mut base = tables();
        let later = SoundEventTables::from_json(
            &json!({"individual_event_sounds": {"events": {"random.click": "custom.click"}}}),
            &json!({}),
        );
        base.merge(later);
        assert_eq!(
            &*base.individual("random.click").unwrap().sound,
            "custom.click"
        );
        assert!(base.block("stone", "break").is_some());
    }
}

/// Legacy door sound, which picks opening or closing at random.
const DOOR_EVENT: i32 = 1003;

/// Sound-only level events: `(id, individual event name, sound definition fallback)`.
const LEVEL_EVENT_SOUNDS: &[(i32, &str, &str)] = &[
    (1000, "block.click", "random.click"),
    (1001, "block.click.fail", "random.click"),
    (1002, "launch", "random.bow"),
    (1004, "fizz", "random.fizz"),
    (1005, "", "random.fuse"),
    (1007, "", "mob.ghast.charge"),
    (1008, "", "mob.ghast.fireball"),
    (1009, "", "mob.blaze.shoot"),
    (1010, "", "mob.zombie.wood"),
    (1012, "", "mob.zombie.woodbreak"),
    (1016, "unfect", "mob.zombie.unfect"),
    (1017, "remedy", "mob.zombie.remedy"),
    (1018, "", "mob.endermen.portal"),
    (1020, "", "random.anvil_break"),
    (1021, "", "random.anvil_use"),
    (1022, "", "random.anvil_land"),
    (1030, "", "random.pop"),
    (1032, "", "mob.endermen.portal"),
    (1040, "", "block.itemframe.add_item"),
    (1041, "", "block.itemframe.break"),
    (1042, "", "block.itemframe.place"),
    (1043, "", "block.itemframe.remove_item"),
    (1044, "", "block.itemframe.rotate_item"),
    (1051, "", "random.orb"),
    (1052, "random.totem", "random.totem"),
    (1060, "", "mob.armor_stand.break"),
    (1061, "", "mob.armor_stand.hit"),
    (1062, "", "mob.armor_stand.land"),
    (1063, "", "mob.armor_stand.place"),
];

/// Shared native/browser selection for sound-range LevelEvent packets.
pub fn level_event_sound_route(
    tables: &SoundEventTables,
    event_id: i32,
    roll: f32,
) -> Option<SoundRoute> {
    if event_id == DOOR_EVENT {
        return Some(SoundRoute {
            sound: if roll >= 0.5 {
                "random.door_close"
            } else {
                "random.door_open"
            }
            .into(),
            volume: FloatRange::ONE,
            pitch: FloatRange::ONE,
        });
    }
    let &(_, individual, fallback) = LEVEL_EVENT_SOUNDS
        .iter()
        .find(|(id, _, _)| *id == event_id)?;
    let lookup = if individual.is_empty() { RouteLookup::Absent } else { tables.individual_lookup(individual) };
    match lookup {
        RouteLookup::Route(route) => Some(route),
        RouteLookup::Silent => None,
        RouteLookup::Absent => Some(SoundRoute {
            sound: fallback.into(), volume: FloatRange::ONE, pitch: FloatRange::ONE,
        }),
    }
}
