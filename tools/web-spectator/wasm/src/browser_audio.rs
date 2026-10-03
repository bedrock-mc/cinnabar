//! Browser sound selection uses the same pack routing tables as the native client.
use assets::{RouteLookup, SoundEventTables};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct SoundRoutes {
    tables: SoundEventTables,
}

#[wasm_bindgen]
impl SoundRoutes {
    #[wasm_bindgen(constructor)]
    pub fn new(sounds: &str, blocks: &str) -> Result<SoundRoutes, String> {
        if sounds.len() > 8 * 1024 * 1024 || blocks.len() > 8 * 1024 * 1024 {
            return Err("sound routing exceeds browser limits".into());
        }
        let sounds = serde_json::from_str(sounds).map_err(|error| error.to_string())?;
        let blocks = serde_json::from_str(blocks).map_err(|error| error.to_string())?;
        Ok(Self {
            tables: SoundEventTables::from_json(&sounds, &blocks),
        })
    }
    /// An explicit silent entity route never falls back to a block/default route.
    pub fn resolve(
        &self,
        event: &str,
        entity: &str,
        block: &str,
        random_unit: f32,
    ) -> Result<String, String> {
        if [event, entity, block].iter().any(|value| value.len() > 128) || !random_unit.is_finite()
        {
            return Err("sound request exceeds browser limits".into());
        }
        let mut route = None;
        if !entity.is_empty() {
            match self.tables.entity_lookup(entity, event, None) {
                RouteLookup::Route(found) => route = Some(found),
                RouteLookup::Silent => return Ok("null".into()),
                RouteLookup::Absent => {}
            }
        }
        if route.is_none() {
            route = self
                .tables
                .material_of(block)
                .and_then(|material| {
                    self.tables.block(material, event).or_else(|| {
                        self.tables.interactive(
                            if entity.is_empty() { "player" } else { entity },
                            event,
                            material,
                        )
                    })
                })
                .or_else(|| self.tables.individual(event).cloned());
        }
        match route {
            Some(route) => serde_json::to_string(&serde_json::json!({"sound": route.sound, "volume": route.volume.sample(random_unit), "pitch": route.pitch.sample(random_unit)})).map_err(|error| error.to_string()),
            None => Ok("null".into()),
        }
    }
    /// Native and browser use one sound-range packet mapping.
    pub fn resolve_level(&self, event_id: i32, random_unit: f32) -> Result<String, String> {
        if !random_unit.is_finite() {
            return Err("invalid sound random unit".into());
        }
        match assets::level_event_sound_route(&self.tables, event_id, random_unit) {
            Some(route) => serde_json::to_string(&serde_json::json!({"sound": route.sound, "volume": route.volume.sample(random_unit), "pitch": route.pitch.sample(random_unit)})).map_err(|error| error.to_string()),
            None => Ok("null".into()),
        }
    }
}
