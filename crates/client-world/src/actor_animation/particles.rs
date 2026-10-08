use super::{ActorAnimationStore, ActorLifetimeId};

/// A controller state that completed authoritative animation evaluation for one actor lifetime.
#[derive(Clone, Copy, Debug)]
pub struct ActorParticleController<'a> {
    pub actor: ActorLifetimeId,
    pub entity: &'a str,
    pub controller: &'a str,
    pub state: &'a str,
    pub controller_index: u32,
    pub state_index: u32,
    pub entered_tick: u64,
    pub reset_generation: u64,
}

impl ActorAnimationStore {
    pub(crate) fn particle_controllers(&self) -> impl Iterator<Item = ActorParticleController<'_>> {
        self.rigs.iter().flat_map(move |(&actor, rig)| {
            let assets = if rig.pack {
                self.pack.as_ref().map(|pack| pack.assets.as_ref())
            } else {
                self.assets.as_deref()
            };
            rig.controllers.iter().filter_map(move |runtime| {
                if !rig.initialized || rig.reset_pending || !runtime.active {
                    return None;
                }
                let assets = assets?;
                let binding = assets.rig_bindings().get(rig.rig_binding)?;
                let entity = assets.symbols().get(binding.entity_symbol as usize)?;
                let controller = assets.controllers().get(runtime.controller)?;
                let symbol = assets.symbols().get(controller.symbol as usize)?;
                let state = assets
                    .controller_states()
                    .get(controller.first_state as usize + usize::from(runtime.state))?;
                let name = assets.molang_symbols().get(state.name as usize)?;
                Some(ActorParticleController {
                    actor,
                    entity: &entity.identifier,
                    controller: &symbol.identifier,
                    state: &name.identifier,
                    controller_index: runtime.controller as u32,
                    state_index: controller.first_state + u32::from(runtime.state),
                    entered_tick: runtime.entered_tick,
                    reset_generation: rig.reset_generation,
                })
            })
        })
    }
}
