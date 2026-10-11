use assets::RegistryRecord;

/// Compiler-only access to the typed values in a canonical block state.
pub(super) struct State(serde_json::Value);

impl State {
    /// Builds the same typed state identity that actor NBT receives during ingestion.
    pub(super) fn identity(record: &RegistryRecord) -> [u8; 32] {
        use world::block_state_identity::{StateValue, state_identity};
        let state = Self::read(record).expect("validated canonical state");
        let values = state.0.as_object().expect("canonical state object");
        let entries: std::collections::BTreeMap<_, _> = values
            .iter()
            .map(|(key, field)| {
                let value = match field["type"].as_str().expect("typed canonical value") {
                    "byte" => StateValue::Byte(field["value"].as_i64().unwrap() as i8),
                    "int" => StateValue::Int(field["value"].as_i64().unwrap() as i32),
                    "string" => StateValue::String(field["value"].as_str().unwrap()),
                    _ => panic!("unsupported canonical state type"),
                };
                (key.as_str(), value)
            })
            .collect();
        state_identity(&record.name, entries)
    }
    /// Decodes state JSON once for the family being compiled.
    pub(super) fn read(record: &RegistryRecord) -> Option<Self> {
        serde_json::from_str(&record.canonical_state).ok().map(Self)
    }

    /// Reads an integer state value.
    pub(super) fn number(&self, name: &str) -> Option<i64> {
        self.0.get(name)?.get("value")?.as_i64()
    }

    /// Reads a string state value.
    pub(super) fn text(&self, name: &str) -> Option<&str> {
        self.0.get(name)?.get("value")?.as_str()
    }

    /// Reads a byte-backed boolean state value.
    pub(super) fn bit(&self, name: &str) -> Option<bool> {
        self.number(name).map(|value| value != 0)
    }

    /// Resolves the south, west, north, east order used by cardinal state families.
    pub(super) fn direction(&self) -> Option<i64> {
        match self.text("minecraft:cardinal_direction") {
            Some("south") => Some(0),
            Some("west") => Some(1),
            Some("north") => Some(2),
            Some("east") => Some(3),
            Some(_) => None,
            None => self
                .number("direction")
                .filter(|value| (0..4).contains(value)),
        }
    }
}

/// Identifies full slab classes whose stored visual and support properties are a unit cube.
pub(super) fn is_double_slab(record: &RegistryRecord) -> bool {
    record.model_family == assets::ModelFamily::Slab
        && (record.name.ends_with("_double_slab")
            || record.name.ends_with("double_cut_copper_slab"))
}
