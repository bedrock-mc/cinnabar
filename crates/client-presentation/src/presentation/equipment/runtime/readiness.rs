//! Keeps exact hand eligibility independent from committed attachable animation state.
use super::*;

impl EquipmentRuntime {
    /// Visits the startup and optional pack layers without allocating a layer list.
    fn attachable_layers(&mut self) -> impl Iterator<Item = &mut client_world::AttachablesRuntime> {
        std::iter::once(&mut self.attachables)
            .chain(self.pack.as_mut().map(|pack| &mut pack.attachables))
    }

    /// Evaluates startup and pack hand models provisionally for the current readiness query.
    pub fn begin_hand_readiness(&mut self) {
        for layer in self.attachable_layers() {
            layer.begin_preview();
        }
    }

    /// Retains computed hand models while returning ordinary equipment to committed evaluation.
    pub fn end_hand_readiness(&mut self) {
        for layer in self.attachable_layers() {
            layer.end_preview();
        }
    }

    /// Adopts a reused hand or discards its provisional models after final source selection.
    pub fn finish_hand_readiness(&mut self, reused: bool) {
        for layer in self.attachable_layers() {
            layer.finish_preview(reused);
        }
    }
}
