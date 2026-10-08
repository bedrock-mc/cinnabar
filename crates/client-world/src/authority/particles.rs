use assets::NetworkIdMode;
use protocol::ParticleEvent;

use super::WorldAuthority;

impl WorldAuthority {
    /// Keeps terrain event ids in the same palette as decoded blocks.
    pub fn remap_particle_block_ids(&self, event: &mut ParticleEvent) {
        if self.network_id_mode != NetworkIdMode::Sequential || self.id_remap.is_identity() {
            return;
        }
        let ParticleEvent::Level(level) = event else {
            return;
        };
        let data = level.data as u32;
        level.data = match level.event_id {
            2001 | 2021 | 0x4013 => self.id_remap.to_internal(data) as i32,
            2014 => {
                let id = self.id_remap.to_internal(data & 0x00ff_ffff);
                ((data & 0xff00_0000) | (id & 0x00ff_ffff)) as i32
            }
            _ => level.data,
        };
    }
}
