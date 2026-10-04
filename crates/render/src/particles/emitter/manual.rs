//! Explicit emission from native cached manual emitters.

use super::{Emitter, Outputs, Rate};

impl Emitter {
    /// Adds particles at this independent origin without moving earlier world-space particles.
    /// `LevelRendererPlayer::addBiomeTintedParticleEffect`
    /// keeps one emitter per colour and calls its manual emission method for every origin.
    pub(in crate::particles) fn emit_cached_manual(
        &mut self,
        position: [f32; 3],
        output: &mut Outputs,
        live_budget: usize,
    ) {
        let def = std::sync::Arc::clone(&self.def);
        let Rate::Manual { max } = &def.emitter.rate else {
            return;
        };
        let capacity = self.eval(max).max(0.0) as usize;
        let room = capacity.saturating_sub(self.particles.len());
        self.pos = position;
        self.manual_pending = 0;
        if room > 0 && live_budget > 0 {
            self.spawn_particle(output);
        }
    }
}
