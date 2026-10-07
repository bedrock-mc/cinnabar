use chunk_pipeline::WorldStream;
use particles::{ParticleSystem, SpawnRequest, actor::ActorEffectKey};

#[cfg(test)]
mod tests;

#[derive(Debug)]
pub(super) enum ActorParticleCommand {
    Start(ActorEffectKey, Box<SpawnRequest>),
    Stop(u64),
}

pub(super) fn queue_actor_particles(
    stream: &WorldStream,
    system: &mut ParticleSystem,
    queue: &mut Vec<ActorParticleCommand>,
) {
    system.actor_emitters.begin_frame();
    for state in stream.authority().actor_particle_controllers() {
        let Some(actor) = stream.authority().actor(state.actor.runtime_id) else {
            continue;
        };
        for (index, (effect, bound)) in system
            .actor_bindings
            .effects_for(state.entity, state.controller, state.state)
            .enumerate()
        {
            if !system.has_effect(effect) {
                continue;
            }
            let key = ActorEffectKey {
                session_id: state.actor.session_id,
                dimension: state.actor.dimension,
                runtime_id: state.actor.runtime_id,
                spawn_revision: state.actor.spawn_revision,
                reset_generation: state.reset_generation,
                controller: state.controller_index,
                state: state.state_index,
                entered_tick: state.entered_tick,
                effect_index: index as u16,
            };
            if system.actor_emitters.admit(key) {
                queue.push(ActorParticleCommand::Start(
                    key,
                    Box::new(SpawnRequest {
                        effect: effect.to_owned(),
                        position: actor.position,
                        bound: bound.then_some((actor.runtime_id, [0.0; 3])),
                        seed: actor.runtime_id ^ actor.spawn_revision.rotate_left(32),
                        ..SpawnRequest::default()
                    }),
                ));
            }
        }
    }
    system
        .actor_emitters
        .finish_frame(|emitter| queue.push(ActorParticleCommand::Stop(emitter)));
}

pub(super) fn route_actor_particles(
    system: &mut ParticleSystem,
    queue: &mut Vec<ActorParticleCommand>,
) {
    for command in queue.drain(..) {
        match command {
            ActorParticleCommand::Start(key, request) => {
                let id = system.spawn(&request);
                system.actor_emitters.started(key, id);
            }
            ActorParticleCommand::Stop(emitter) => system.stop(emitter),
        }
    }
}
