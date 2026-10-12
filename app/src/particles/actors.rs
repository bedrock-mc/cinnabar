use std::{borrow::Cow, collections::HashMap};

use actor_animation::{ActorRenderFrame, ActorRigSnapshot, RenderTextureLayer};
use chunk_pipeline::WorldStream;
use particles::{ParticleSystem, SpawnRequest, actor::ActorEffectKey};

#[cfg(test)]
mod tests;

#[derive(Debug)]
pub(super) enum ActorParticleCommand {
    Start(ActorEffectKey, Box<SpawnRequest>),
    Stop(u64),
}

/// Lazily shares each actor's sampled rig and layers across spawning and attachment updates.
pub(super) struct ActorParticleFrame<'a> {
    stream: &'a WorldStream,
    alpha: f32,
    render: ActorRenderFrame<'a>,
    samples: HashMap<u64, ActorParticleSample<'a>>,
}

struct ActorParticleSample<'a> {
    rig: Option<ActorRigSnapshot<'a>>,
    layers: Cow<'a, [RenderTextureLayer]>,
}

impl<'a> ActorParticleFrame<'a> {
    /// Starts an empty sampling cache for one particle frame at the supplied interpolation.
    pub(super) fn new(stream: &'a WorldStream, alpha: f32) -> Self {
        Self {
            stream,
            alpha,
            render: stream.authority().actor_render_frame(alpha),
            samples: HashMap::new(),
        }
    }

    /// Evaluates an actor's placement inputs once, only when a particle needs them.
    fn sample(&mut self, runtime_id: u64) -> &ActorParticleSample<'a> {
        self.samples.entry(runtime_id).or_insert_with(|| {
            let rig = self
                .stream
                .authority()
                .actor_rig(runtime_id)
                .map(|rig| self.render.sample_rig_scale(rig));
            let layers = self.render.layers(runtime_id).unwrap_or_default();
            ActorParticleSample { rig, layers }
        })
    }
}

/// Queues each pending state-entry occurrence only after its placement becomes available.
pub(super) fn queue_actor_particles(
    frame: &mut ActorParticleFrame<'_>,
    system: &mut ParticleSystem,
    queue: &mut Vec<ActorParticleCommand>,
) {
    let stream = frame.stream;
    let alpha = frame.alpha;
    system.actor_emitters.begin_frame();
    for state in stream.authority().actor_particle_controllers() {
        let Some(actor) = stream.authority().actor(state.actor.runtime_id) else {
            continue;
        };
        for (index, effect) in system
            .actor_bindings
            .effects_for(state.entity, state.controller, state.state)
            .enumerate()
        {
            if !system.has_effect(effect.identifier) {
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
                let sample = frame.sample(actor.runtime_id);
                let Some(transform) =
                    client_presentation::presentation::actors::actor_emitter_transform(
                        actor,
                        sample.rig.as_ref(),
                        &sample.layers,
                        effect.locator,
                        alpha,
                    )
                else {
                    continue;
                };
                let query_names = effect
                    .initialization
                    .into_iter()
                    .flat_map(|script| script.query_names())
                    .map(Into::into)
                    .collect();
                let attachment = particles::actor::ActorAttachment {
                    runtime_id: actor.runtime_id,
                    spawn_revision: actor.spawn_revision,
                    locator: effect.locator.map(Into::into),
                    queries: query_names,
                    entered_tick: state.entered_tick,
                    world_offset: [0.0; 3],
                };
                let queries = if effect.bound {
                    actor_queries(actor, sample.rig.as_ref(), &attachment, alpha)
                } else {
                    particles::Queries::default()
                };
                queue.push(ActorParticleCommand::Start(
                    key,
                    Box::new(SpawnRequest {
                        effect: effect.identifier.to_owned(),
                        position: transform.position,
                        basis: effect.bound.then_some(transform.basis),
                        initialization: effect.initialization.cloned(),
                        queries,
                        actor_attachment: effect.bound.then_some(attachment),
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

/// Applies queued starts and stops and records each spawn attempt.
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

/// Provides emitter-local numeric query inputs from the retained actor context.
fn actor_queries(
    actor: &client_world::ActorSnapshot,
    rig: Option<&actor_animation::ActorRigSnapshot<'_>>,
    attachment: &particles::actor::ActorAttachment,
    alpha: f32,
) -> particles::Queries {
    let context = rig.map(|rig| rig.particle_queries(actor, alpha, attachment.entered_tick));
    let query = |name| context.as_ref().map_or(0.0, |context| context.query(name));
    particles::Queries {
        is_in_water: query("is_in_water"),
        is_in_water_or_rain: query("is_in_water_or_rain"),
        is_baby: query("is_baby"),
        ground_speed: query("ground_speed"),
        frame_alpha: alpha,
        spell_color: actor.spell_color(),
        actor: (!attachment.queries.is_empty()).then(|| {
            std::sync::Arc::new(
                attachment
                    .queries
                    .iter()
                    .map(|name| (name.clone(), query(name.as_ref())))
                    .collect(),
            )
        }),
    }
}

/// Updates bound controller emitters from sampled actor poses, guarding actor replacement.
pub(super) fn follow_actor_particles(
    system: &mut ParticleSystem,
    frame: &mut ActorParticleFrame<'_>,
) {
    use particles::actor::ActorAttachmentUpdate;
    let stream = frame.stream;
    let alpha = frame.alpha;
    system.update_actor_attachments(|attachment| {
        let Some(actor) = stream.authority().actor(attachment.runtime_id) else {
            return ActorAttachmentUpdate::Removed;
        };
        if actor.spawn_revision != attachment.spawn_revision {
            return ActorAttachmentUpdate::Removed;
        }
        let sample = frame.sample(actor.runtime_id);
        let Some(mut transform) =
            client_presentation::presentation::actors::actor_emitter_transform(
                actor,
                sample.rig.as_ref(),
                &sample.layers,
                attachment.locator.as_deref(),
                alpha,
            )
        else {
            return ActorAttachmentUpdate::Pending;
        };
        transform.position =
            std::array::from_fn(|axis| transform.position[axis] + attachment.world_offset[axis]);
        ActorAttachmentUpdate::Placed(
            transform,
            actor_queries(actor, sample.rig.as_ref(), attachment, alpha),
        )
    });
}
