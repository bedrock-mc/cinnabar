//! Numeric presentation routes compiled from the native event rules.

use super::ParticleEffect;
use super::events_generated::{LEGACY_PARTICLES, LEVEL_PARTICLES, LEVEL_SOUNDS, NOTE_INSTRUMENTS};

/// Looks up a legacy particle effect without matching an identifier during simulation.
pub fn legacy_particle_effect(id: i32) -> Option<ParticleEffect> {
    LEGACY_PARTICLES
        .binary_search_by_key(&id, |row| row.0)
        .ok()
        .map(|index| LEGACY_PARTICLES[index].1)
}

/// Looks up an ordinary named level particle; parameterized events retain their decoder.
pub fn level_particle_effect(id: i32) -> Option<ParticleEffect> {
    LEVEL_PARTICLES
        .binary_search_by_key(&id, |row| row.0)
        .ok()
        .map(|index| LEVEL_PARTICLES[index].1)
}

/// Returns the authored event name and fallback definition for a sound-only level event.
pub fn level_event_sound(id: i32) -> Option<(&'static str, &'static str)> {
    LEVEL_SOUNDS
        .binary_search_by_key(&id, |row| row.0)
        .ok()
        .map(|index| {
            let (_, event, fallback) = LEVEL_SOUNDS[index];
            (event, fallback)
        })
}

/// Resolves the packed instrument selector without a second instrument inventory.
pub fn note_instrument(index: usize) -> Option<&'static str> {
    NOTE_INSTRUMENTS.get(index).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol_ids::LevelEvent;

    #[test]
    fn facts_migration_qualified_particle_identifiers_round_trip() {
        for &effect in ParticleEffect::ALL {
            assert_eq!(effect.identifier(), format!("minecraft:{}", effect.name()));
            assert_eq!(ParticleEffect::from_name(effect.identifier()), Some(effect));
        }
    }

    #[test]
    fn generated_routes_preserve_wire_selectors_and_unknown_refusals() {
        assert_eq!(
            legacy_particle_effect(1),
            Some(ParticleEffect::BasicBubbleParticle)
        );
        assert_eq!(legacy_particle_effect(0), None);
        assert_eq!(legacy_particle_effect(i32::MAX), None);
        assert_eq!(
            level_particle_effect(LevelEvent::ParticleCropGrowth.wire_value()),
            Some(ParticleEffect::CropGrowthEmitter)
        );
        assert_eq!(
            level_event_sound(LevelEvent::SoundClick.wire_value()),
            Some(("block.click", "random.click"))
        );
        assert_eq!(note_instrument(0), Some("note.harp"));
        assert_eq!(note_instrument(NOTE_INSTRUMENTS.len()), None);
        for &(id, _) in LEVEL_PARTICLES {
            assert!(LevelEvent::from_wire(id).is_some());
        }
        for table in [LEGACY_PARTICLES, LEVEL_PARTICLES] {
            assert!(table.windows(2).all(|pair| pair[0].0 < pair[1].0));
        }
    }
}
