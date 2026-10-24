//! Two-ID, one-section test fixture. These IDs are not a production block registry.

use sim::{Aabb, CollisionRegistry, PaletteWorld, Vec3};
use world::{BlockUpdate, ChunkStore, RawBlockIds, SubChunkKey};

pub const KEY: SubChunkKey = SubChunkKey::new(0, 0, 0, 0);
pub const AIR: u32 = 0;
pub const SOLID: u32 = 1;
pub const SPAWN: Vec3 = Vec3::new(8., 1., 4.);
/// Test camera offset and reach, not accepted retail gameplay constants.
pub const EYE_HEIGHT: f64 = 1.5;
pub const REACH: f64 = 5.;

pub struct Fixture {
    pub store: ChunkStore,
    pub registry: CollisionRegistry,
    pub air: u32,
    pub solid: u32,
}

impl Fixture {
    pub fn new() -> Self {
        Self::with_ids(AIR, SOLID)
    }

    /// Uses caller-resolved cube identities; geometry and collision remain a test fixture.
    pub fn with_ids(air: u32, solid: u32) -> Self {
        assert_ne!(air, solid);
        let bytes = encode(air, solid, |x, y, z| {
            y == 0 || (y < 3 && (4..12).contains(&x) && (10..12).contains(&z))
        });
        let mut store = ChunkStore::new();
        store
            .apply_sub_chunk(KEY, &bytes, &RawBlockIds { air })
            .unwrap();
        // This synthetic column is known air outside the one encoded section.
        // Marking only that section leaves below-floor liquid/contact queries unknown.
        store.mark_chunk_loaded(KEY.chunk()).unwrap();
        let mut registry = CollisionRegistry::new();
        registry.set_air_runtime_id(air);
        registry.register(air, []).unwrap();
        registry
            .register(solid, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
            .unwrap();
        Self {
            store,
            registry,
            air,
            solid,
        }
    }

    /// Applies fixture-only creative edits after the shared selection ray succeeds.
    /// Production interaction admission/authority is still owned by gameplay/core.
    pub fn edit(&mut self, eye: Vec3, direction: Vec3, place: bool, feet: Vec3) -> bool {
        let view = PaletteWorld::new(
            &self.store,
            &self.registry,
            KEY.dimension,
            Default::default(),
        );
        let Ok(Some(hit)) = view.block_interaction_ray_current(eye, direction, REACH) else {
            return false;
        };
        let mut pos = hit.block_pos;
        if place {
            let axis = usize::from(hit.face / 2);
            // Bedrock face pairs are Y, Z, X; derive the axis from that semantic face.
            let axis = match axis {
                0 => 1,
                1 => 2,
                2 => 0,
                _ => return false,
            };
            pos[axis] += if hit.face & 1 == 0 { -1 } else { 1 };
            if !view.is_air(pos).unwrap_or(false) {
                return false;
            }
            let origin = Vec3::new(pos[0].into(), pos[1].into(), pos[2].into());
            if Aabb::new(origin, origin + Vec3::ONE).intersects(Aabb::player_at(feet)) {
                return false;
            }
        }
        // The fixture owns one section. Never pretend its boundary is a game-world limit.
        if !pos.iter().all(|n| (0..16).contains(n)) {
            return false;
        }
        self.store
            .update_block(
                KEY,
                BlockUpdate {
                    x: pos[0] as u8,
                    y: pos[1] as u8,
                    z: pos[2] as u8,
                    layer: 0,
                    runtime_id: if place { self.solid } else { self.air },
                },
                self.air,
            )
            .unwrap()
            .is_some()
    }

    /// A Bedrock runtime-palette subchunk checkpoint, not a Minecraft world save.
    pub fn checkpoint(&self) -> Vec<u8> {
        let sub = self.store.sub_chunk_ref(KEY).unwrap();
        encode(self.air, self.solid, |x, y, z| {
            sub.runtime_id(0, x, y, z) == Some(self.solid)
        })
    }

    pub fn restore(&mut self, bytes: &[u8]) {
        self.store
            .apply_sub_chunk(KEY, bytes, &RawBlockIds { air: self.air })
            .unwrap();
    }
}

fn encode(air: u32, block: u32, mut solid: impl FnMut(u8, u8, u8) -> bool) -> Vec<u8> {
    let mut words = [0_u32; 128];
    for x in 0..16 {
        for z in 0..16 {
            for y in 0..16 {
                if solid(x, y, z) {
                    let i = usize::from(x) * 256 + usize::from(z) * 16 + usize::from(y);
                    words[i / 32] |= 1 << (i % 32);
                }
            }
        }
    }
    let mut bytes = vec![9, 1, 0, 3];
    for word in words {
        bytes.extend(word.to_le_bytes());
    }
    // Network palettes use signed zigzag varints for count and runtime identities.
    for value in [2, air, block] {
        let mut encoded = value << 1;
        loop {
            let next = (encoded & 0x7f) as u8;
            encoded >>= 7;
            bytes.push(next | if encoded != 0 { 0x80 } else { 0 });
            if encoded == 0 {
                break;
            }
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nontrivial_palette_ids_survive_checkpoint_and_edits() {
        let mut fixture = Fixture::with_ids(127, 4097);
        let mut player = sim::PlayerState::new(SPAWN);
        for _ in 0..20 {
            sim::Simulator::default()
                .tick(
                    &mut player,
                    sim::MovementInput::default(),
                    &PaletteWorld::for_movement(&fixture.store, &fixture.registry, KEY.dimension),
                )
                .unwrap();
        }
        assert!(player.on_ground);
        assert_eq!(
            fixture
                .store
                .sub_chunk_ref(KEY)
                .unwrap()
                .runtime_id(0, 8, 0, 8),
            Some(4097)
        );
        assert!(fixture.edit(
            Vec3::new(8.5, 2.5, 8.5),
            Vec3::new(0., 0., 1.),
            false,
            SPAWN
        ));
        let mut restored = Fixture::with_ids(fixture.air, fixture.solid);
        restored.restore(&fixture.checkpoint());
        assert!(
            fixture
                .store
                .sub_chunk_ref(KEY)
                .unwrap()
                .blocks_match(restored.store.sub_chunk_ref(KEY).unwrap())
        );
    }
    #[test]
    fn movement_samples_known_air_around_the_floor() {
        let fixture = Fixture::new();
        let mut player = sim::PlayerState::new(SPAWN);
        let view = PaletteWorld::for_movement(&fixture.store, &fixture.registry, KEY.dimension);
        for _ in 0..20 {
            sim::Simulator::default()
                .tick(
                    &mut player,
                    sim::MovementInput {
                        forward: 1.,
                        ..Default::default()
                    },
                    &view,
                )
                .unwrap();
        }
        assert!(player.on_ground);
        assert_eq!(player.position.y, 1.);
        assert!(player.position.z > SPAWN.z + 1.);
    }
    #[test]
    fn selected_block_edits_survive_palette_checkpoint_reload() {
        let mut fixture = Fixture::new();
        let eye = Vec3::new(8.5, 2.5, 8.5);
        assert!(fixture.edit(eye, Vec3::new(0., 0., 1.), false, SPAWN));
        assert_eq!(
            fixture
                .store
                .sub_chunk_ref(KEY)
                .unwrap()
                .runtime_id(0, 8, 2, 10),
            Some(AIR)
        );
        assert!(fixture.edit(eye, Vec3::new(0., -1., 0.), true, SPAWN));
        let saved = fixture.checkpoint();
        let mut reloaded = Fixture::new();
        reloaded.restore(&saved);
        assert!(
            fixture
                .store
                .sub_chunk_ref(KEY)
                .unwrap()
                .blocks_match(reloaded.store.sub_chunk_ref(KEY).unwrap())
        );
    }

    #[test]
    fn placement_cannot_intersect_the_player() {
        let mut fixture = Fixture::new();
        let feet = Vec3::new(8.5, 1., 8.5);
        assert!(!fixture.edit(
            feet + Vec3::new(0., EYE_HEIGHT, 0.),
            Vec3::new(0., -1., 0.),
            true,
            feet
        ));
    }
}
