//! Recorded triggers use Cinnabar's compiled definitions and particle GPU plugin.
use std::collections::BTreeSet;

use bevy::platform::time::Instant;
use particles::{LevelParticle, ParticleSystem, ParticleView, classify_level_event, named_request};
use render::ParticleGpuFrame;

use crate::browser_model::{Frame, SceneEvent};

pub(super) struct BrowserEffects {
    system: ParticleSystem,
    icons: assets::RuntimeIconCatalog,
    seen: BTreeSet<String>,
    epoch: Option<u64>,
    last_update: Instant,
}
impl BrowserEffects {
    pub(super) fn new(bytes: &[u8], icon_bytes: &[u8]) -> Result<Self, String> {
        let assets =
            assets::RuntimeParticleAssets::decode(bytes).map_err(|error| error.to_string())?;
        Ok(Self {
            system: ParticleSystem::from_assets(&assets),
            icons: assets::RuntimeIconCatalog::decode(icon_bytes)
                .map_err(|error| error.to_string())?,
            seen: BTreeSet::new(),
            epoch: None,
            last_update: Instant::now(),
        })
    }
    fn spawn(
        &mut self,
        event: &SceneEvent,
        terrain: Option<&crate::terrain_runtime::TerrainScene>,
        assets: &crate::TerrainAssets,
    ) {
        let request = match event.kind.as_str() {
            "particle" => {
                let mut request = named_request(&event.name, event.position, None);
                request.variables = event
                    .data
                    .iter()
                    .map(|(name, value)| {
                        (
                            name.trim_start_matches("variable.").to_ascii_lowercase(),
                            *value as f32,
                        )
                    })
                    .collect();
                Some(request)
            }
            "level" => match classify_level_event(
                event.data.get("eventId").copied().unwrap_or(0.0) as i32,
                event.data.get("extraData").copied().unwrap_or(0.0) as i32,
            ) {
                Some(LevelParticle::Named {
                    effect,
                    spell_color,
                }) => Some(named_request(effect, event.position, spell_color)),
                Some(LevelParticle::ItemIcon { .. }) if !event.item_name.is_empty() => {
                    particles::tiles::item_tile(&self.icons, &event.item_name, event.item_aux).map(
                        |tile| {
                            particles::item_icon_request(
                                event.position,
                                tile,
                                particles::ITEM_ICON_PARTICLES as f32,
                            )
                        },
                    )
                }
                Some(LevelParticle::FixedItemIcon { identifier, count }) => {
                    particles::tiles::item_tile(&self.icons, identifier, 0).map(|tile| {
                        particles::item_icon_request(event.position, tile, count as f32)
                    })
                }
                Some(
                    kind @ (LevelParticle::BlockBreak { .. }
                    | LevelParticle::Terrain { .. }
                    | LevelParticle::BlockCrack { .. }),
                ) => {
                    let block = event.position.map(|value| value.floor() as i32);
                    let id = match kind {
                        LevelParticle::BlockCrack { .. } => {
                            terrain.map(|scene| scene.block_id(block, assets.air))
                        }
                        _ if !event.block_name.is_empty() => crate::canonical::palette_ids(
                            &assets.canonical,
                            &[crate::model::PaletteEntry {
                                name: event.block_name.clone(),
                                states: event.block_states.clone(),
                            }],
                        )
                        .ok()
                        .and_then(|ids| ids.first().copied()),
                        _ => None,
                    };
                    id.and_then(|id| {
                        particles::tiles::terrain_tile(
                            &assets.runtime,
                            assets::NetworkIdMode::Sequential,
                            id,
                        )
                    })
                    .map(|(tile, _flags)| match kind {
                        LevelParticle::BlockBreak { .. } => particles::block_break_request(
                            particles::BLOCK_BREAK_EFFECT,
                            block,
                            tile,
                            [1.0; 4],
                        ),
                        LevelParticle::BlockCrack { face, .. } => particles::block_crack_request(
                            particles::BLOCK_BREAK_EFFECT,
                            block,
                            face,
                            tile,
                            [1.0; 4],
                        ),
                        _ => particles::terrain_request(
                            particles::BLOCK_BREAK_EFFECT,
                            block,
                            tile,
                            [1.0; 4],
                        ),
                    })
                }
                _ => None,
            },
            _ => None,
        };
        if let Some(mut request) = request {
            // Replaying the same event uses the same native emitter random seed.
            request.seed = event.id.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
            });
            self.system.spawn(&request);
        }
    }
    pub(super) fn update(
        &mut self,
        frame: &Frame,
        output: &mut ParticleGpuFrame,
        view: &ParticleView,
        wall_millis: u64,
        terrain: Option<&crate::terrain_runtime::TerrainScene>,
        assets: &crate::TerrainAssets,
    ) {
        let particle_world = terrain.map(|scene| scene.particle_world(assets));
        let empty = particles::EmptyWorld;
        let world: &dyn particles::ParticleWorld = particle_world
            .as_ref()
            .map_or(&empty as &dyn particles::ParticleWorld, |world| world);
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_update).as_secs_f32().min(0.25);
        self.last_update = now;
        self.system.set_camera(view.position);
        let reset = self.epoch != Some(frame.replay_epoch);
        if reset {
            self.system.clear();
            self.seen.clear();
            self.epoch = Some(frame.replay_epoch);
        }
        let mut events = frame
            .events
            .iter()
            .filter_map(|event| {
                let timestamp = js_sys::Date::parse(&event.updated_at);
                (timestamp.is_finite()
                    && timestamp <= wall_millis as f64
                    && wall_millis as f64 - timestamp <= 5000.0)
                    .then_some((timestamp, event))
            })
            .collect::<Vec<_>>();
        events.sort_by(|(left, _), (right, _)| left.total_cmp(right));
        let mut cursor = events
            .first()
            .map_or(wall_millis as f64, |(timestamp, _)| *timestamp);
        for (timestamp, event) in events {
            if self.seen.insert(event.id.clone()) {
                if reset {
                    self.advance((timestamp - cursor) as f32 / 1000.0, world);
                    cursor = timestamp;
                }
                self.spawn(event, terrain, assets);
            }
        }
        if reset {
            self.advance((wall_millis as f64 - cursor) as f32 / 1000.0, world);
        }
        self.seen
            .retain(|id| frame.events.iter().any(|event| event.id == *id));
        let dt = elapsed * frame.visual_speed();
        render::update_particle_frame(&mut self.system, output, dt, view, world);
    }
    fn advance(&mut self, seconds: f32, world: &dyn particles::ParticleWorld) {
        let mut remaining = seconds.clamp(0.0, 5.0);
        while remaining > 0.0 {
            let step = remaining.min(0.25);
            self.system.tick(step, world);
            remaining -= step;
        }
    }
}
