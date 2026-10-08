use serde::Deserialize;

// Go encodes nil event data and block state maps as JSON null. They carry no
// entries, so accept the empty collection without loosening other fields.
fn null_is_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Option::<T>::deserialize(deserializer).map(Option::unwrap_or_default)
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Frame {
    pub(super) id: String,
    pub(super) updated_at: String,
    pub(super) arena_id: String,
    pub(super) mode: String,
    #[serde(default)]
    pub(super) round_active: bool,
    #[serde(rename = "players")]
    pub(super) fighters: Vec<Fighter>,
    #[serde(default, deserialize_with = "null_is_default")]
    pub(super) team_wins: Vec<i32>,
    #[serde(default)]
    pub(super) entities: Vec<SceneEntity>,
    #[serde(default)]
    pub(super) events: Vec<SceneEvent>,
    #[serde(default)]
    pub(super) blocks: Vec<SceneBlock>,
    #[serde(default)]
    pub(super) replay_epoch: u64,
    pub(super) replay_playing: Option<bool>,
    pub(super) replay_speed: Option<f32>,
}

impl Frame {
    pub(super) fn visual_speed(&self) -> f32 {
        if self.replay_playing == Some(false) {
            0.0
        } else {
            self.replay_speed.unwrap_or(1.0).clamp(0.25, 2.0)
        }
    }

    pub(super) fn parse(input: &str) -> Result<Self, String> {
        if input.len() > 1024 * 1024 {
            return Err("spectator frame exceeds 1 MiB".into());
        }
        let frame: Self = serde_json::from_str(input).map_err(|error| error.to_string())?;
        if frame.replay_speed.is_some_and(|speed| !speed.is_finite()) {
            return Err("spectator replay speed is invalid".into());
        }
        if frame.entities.len() > 256 || frame.events.len() > 512 || frame.blocks.len() > 8192 {
            return Err("spectator world state exceeds admission limits".into());
        }
        for entity in &frame.entities {
            if entity.id.len() > 128
                || entity.kind.len() > 128
                || !finite_position(&entity.position)
                || !entity.yaw.is_finite()
                || !entity.pitch.is_finite()
            {
                return Err("spectator entity exceeds admission limits".into());
            }
            if let Some(item) = &entity.item {
                item.validate()?;
            }
        }
        for event in &frame.events {
            if event.id.len() > 128
                || event.kind.len() > 64
                || event.name.len() > 128
                || event.updated_at.len() > 64
                || !finite_position(&event.position)
                || event.data.len() > 64
                || event
                    .data
                    .iter()
                    .any(|(key, value)| key.len() > 128 || !value.is_finite())
            {
                return Err("spectator event exceeds admission limits".into());
            }
        }
        for block in &frame.blocks {
            if block.name.len() > 128
                || block.states.len() > 64
                || block
                    .position
                    .iter()
                    .any(|value| !(-1_000_000..=1_000_000).contains(value))
            {
                return Err("spectator block exceeds admission limits".into());
            }
        }
        if frame.fighters.len() > 32
            || frame.id.len() > 128
            || frame.arena_id.len() > 128
            || frame.updated_at.len() > 64
            || frame.mode.len() > 64
            || frame.team_wins.len() > 16
        {
            return Err("spectator frame exceeds fighter or identity limits".into());
        }
        for fighter in &frame.fighters {
            if fighter.id.len() > 128
                || fighter.name.len() > 128
                || !fighter
                    .position
                    .iter()
                    .all(|value| value.is_finite() && value.abs() <= 1_000_000.0)
                || ![
                    fighter.yaw,
                    fighter.pitch,
                    fighter.health,
                    fighter.max_health,
                ]
                .iter()
                .all(|value| value.is_finite())
            {
                return Err("spectator fighter has an invalid identity or coordinate".into());
            }
            if let Some(equipment) = &fighter.equipment {
                for item in equipment
                    .armour
                    .iter()
                    .chain([&equipment.main_hand, &equipment.off_hand])
                    .flatten()
                {
                    item.validate()?;
                }
            }
            if let Some(pov) = &fighter.pov {
                pov.validate()?;
            }
        }
        let identities = frame
            .fighters
            .iter()
            .map(|fighter| &fighter.id)
            .collect::<std::collections::BTreeSet<_>>();
        if identities.len() != frame.fighters.len() {
            return Err("spectator frame repeats a fighter identity".into());
        }
        Ok(frame)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Fighter {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) position: [f32; 3],
    pub(super) yaw: f32,
    pub(super) pitch: f32,
    pub(super) health: f32,
    pub(super) max_health: f32,
    pub(super) dead: bool,
    pub(super) equipment: Option<Equipment>,
    #[serde(default)]
    pub(super) sneaking: bool,
    #[serde(default)]
    pub(super) sprinting: bool,
    #[serde(default)]
    pub(super) using_item: bool,
    #[serde(default)]
    pub(super) on_ground: bool,
    #[serde(default)]
    pub(super) swimming: bool,
    pub(super) swing_at: Option<String>,
    pub(super) swing_id: Option<String>,
    pub(super) hurt_at: Option<String>,
    pub(super) hurt_id: Option<String>,
    pub(super) pov: Option<Pov>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Equipment {
    pub(super) main_hand: Option<Item>,
    pub(super) off_hand: Option<Item>,
    pub(super) armour: [Option<Item>; 4],
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Item {
    pub(super) name: String,
    pub(super) meta: i32,
    pub(super) count: i32,
    pub(super) enchanted: bool,
    pub(super) durability: i32,
    pub(super) max_durability: i32,
    pub(super) color: Option<String>,
    pub(super) block: Option<ItemBlock>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct ItemBlock {
    pub(super) name: String,
    #[serde(default, deserialize_with = "null_is_default")]
    pub(super) states: serde_json::Map<String, serde_json::Value>,
}
impl Item {
    fn validate(&self) -> Result<(), String> {
        if self.name.len() > 128
            || self.count < 0
            || self.count > 1024
            || self.color.as_ref().is_some_and(|value| value.len() > 32)
        {
            return Err("spectator item exceeds admission limits".into());
        }
        if self.block.as_ref().is_some_and(|block| {
            block.name.len() > 128
                || block.states.len() > 64
                || block.states.iter().any(|(key, value)| {
                    key.len() > 64
                        || !(value.is_boolean() || value.is_string() || value.as_i64().is_some())
                })
        }) {
            return Err("invalid item block state".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Pov {
    pub(super) hotbar: [Option<Item>; 9],
    pub(super) selected_slot: usize,
    pub(super) eye_height: f32,
    pub(super) food: i32,
    pub(super) absorption: f32,
    pub(super) armour_points: f32,
    pub(super) experience_level: i32,
    pub(super) experience_progress: f32,
    pub(super) air_ticks: Option<i32>,
    pub(super) max_air_ticks: Option<i32>,
    #[serde(default, deserialize_with = "null_is_default")]
    pub(super) effects: Vec<Effect>,
    pub(super) hud: Hud,
}

impl Pov {
    fn validate(&self) -> Result<(), String> {
        if self.selected_slot >= self.hotbar.len()
            || self.effects.len() > 64
            || ![
                self.eye_height,
                self.absorption,
                self.armour_points,
                self.experience_progress,
            ]
            .iter()
            .all(|value| value.is_finite())
            || !(0.0..=4.0).contains(&self.eye_height)
        {
            return Err("spectator POV has invalid statistics or a selected slot".into());
        }
        for item in self.hotbar.iter().flatten() {
            item.validate()?;
        }
        if self.hud.scoreboard.as_ref().is_some_and(|value| {
            value.title.len() > 1024
                || value.lines.len() > 15
                || value.lines.iter().any(|line| line.len() > 1024)
        }) || [&self.hud.popup, &self.hud.action_bar]
            .into_iter()
            .flatten()
            .any(|value| value.text.len() > 1024 || value.updated_at.len() > 64)
            || self.hud.title.as_ref().is_some_and(|value| {
                value.text.len() > 1024
                    || value.subtitle.len() > 1024
                    || value.updated_at.len() > 64
            })
        {
            return Err("spectator HUD exceeds bounded text limits".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Effect {
    pub(super) id: i32,
    pub(super) level: i32,
    pub(super) duration_ticks: i32,
    pub(super) infinite: bool,
    pub(super) particles_hidden: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Hud {
    pub(super) scoreboard: Option<Scoreboard>,
    pub(super) popup: Option<HudText>,
    pub(super) title: Option<HudTitle>,
    pub(super) action_bar: Option<HudText>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct Scoreboard {
    pub(super) title: String,
    pub(super) lines: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct HudText {
    pub(super) text: String,
    pub(super) updated_at: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct HudTitle {
    pub(super) text: String,
    pub(super) subtitle: String,
    pub(super) fade_in_ticks: i32,
    pub(super) stay_ticks: i32,
    pub(super) fade_out_ticks: i32,
    pub(super) updated_at: String,
}

fn finite_position(position: &[f32; 3]) -> bool {
    position
        .iter()
        .all(|value| value.is_finite() && value.abs() <= 1_000_000.0)
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SceneEntity {
    pub(super) id: String,
    pub(super) kind: String,
    pub(super) position: [f32; 3],
    pub(super) yaw: f32,
    pub(super) pitch: f32,
    pub(super) item: Option<Item>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SceneEvent {
    pub(super) id: String,
    pub(super) kind: String,
    pub(super) position: [f32; 3],
    #[serde(default)]
    pub(super) name: String,
    #[serde(default, deserialize_with = "null_is_default")]
    pub(super) data: std::collections::BTreeMap<String, f64>,
    pub(super) updated_at: String,
    #[serde(default)]
    pub(super) block_name: String,
    #[serde(default)]
    pub(super) item_name: String,
    #[serde(default)]
    pub(super) item_aux: u32,
    #[serde(default, deserialize_with = "null_is_default")]
    pub(super) block_states: serde_json::Map<String, serde_json::Value>,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(super) struct SceneBlock {
    pub(super) position: [i32; 3],
    pub(super) name: String,
    #[serde(default, deserialize_with = "null_is_default")]
    pub(super) states: serde_json::Map<String, serde_json::Value>,
}

impl SceneEntity {
    pub(super) fn observation(&self) -> Fighter {
        Fighter {
            id: self.id.clone(),
            name: self.kind.clone(),
            position: self.position,
            yaw: self.yaw,
            pitch: self.pitch,
            health: 1.0,
            max_health: 1.0,
            dead: false,
            equipment: None,
            sneaking: false,
            sprinting: false,
            using_item: false,
            on_ground: false,
            swimming: false,
            swing_at: None,
            swing_id: None,
            hurt_at: None,
            hurt_id: None,
            pov: None,
        }
    }
}
