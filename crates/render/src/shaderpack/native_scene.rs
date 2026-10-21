//! Keeps native scene layers until their pack replacements and view targets are ready.

use super::*;

/// Whether native layers need the scene-copy path until the geometry host can compose them.
pub(super) fn needs_native_scene(
    atmosphere: Option<&render_scene::AtmosphereFrame>,
    particles: Option<&render_scene::ParticleGpuFrame>,
    weather: Option<&render_scene::PrecipitationScene>,
    overlays: Option<&render_scene::ScreenOverlayScene>,
    nametags: Option<&render_scene::NametagSceneResource>,
    covered: impl Fn(Effect) -> bool,
) -> bool {
    atmosphere.is_some_and(|frame| {
        frame.sky_kind() == render_scene::SkyKind::Overworld
            && [Effect::Sun, Effect::Moon, Effect::Stars, Effect::Clouds]
                .into_iter()
                .any(|effect| !covered(effect))
    }) || particles.is_some_and(|frame| {
        (!frame.opaque.is_empty() && !covered(Effect::Particles))
            || (!frame.blend.is_empty() && !covered(Effect::ParticlesTranslucent))
            || (!frame.add.is_empty() && !covered(Effect::ParticlesAdditive))
    }) || weather.is_some_and(|scene| !scene.layers.is_empty() && !covered(Effect::Weather))
        || overlays.is_some_and(|scene| !scene.layers.is_empty())
        || nametags.is_some_and(|scene| !scene.records.is_empty())
}

/// Native layers whose pixels cannot yet be composed over replacement terrain.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct NativeSceneLayers<'w> {
    actors: NativeActorFrames<'w>,
    pipelines: Option<Res<'w, ActorSkinPipelines>>,
    cache: Option<Res<'w, PipelineCache>>,
    atmosphere: Option<Res<'w, render_scene::AtmosphereFrame>>,
    particles: Option<Res<'w, render_scene::ParticleGpuFrame>>,
    weather: Option<Res<'w, render_scene::PrecipitationScene>>,
    overlays: Option<Res<'w, render_scene::ScreenOverlayScene>>,
    nametags: Option<Res<'w, render_scene::NametagSceneResource>>,
    viewmodel: Option<Res<'w, render_scene::ViewmodelScene>>,
}

impl NativeSceneLayers<'_> {
    /// Requires actor kernels and compatible native frames before replacing world queues.
    fn world_ready(&self) -> bool {
        self.pipelines
            .as_deref()
            .zip(self.cache.as_deref())
            .and_then(|(pipelines, cache)| pipelines.kernels(cache))
            .is_some()
            && actors::compatibility::frame_supported(
                self.actors.0.as_deref(),
                self.actors.1.as_deref(),
                self.actors.2.as_deref().map(|frame| &frame.0),
            )
    }

    /// Keeps static viewmodels and hands without a ready actor replacement on the native path.
    pub(super) fn first_person_native(&self) -> bool {
        (self
            .actors
            .1
            .as_ref()
            .is_some_and(|scene| scene.is_active())
            && !self.world_ready())
            || self
                .viewmodel
                .as_ref()
                .is_some_and(|scene| scene.frame.is_some())
    }

    /// Whether this frame needs the native scene colour to preserve its layers.
    pub(super) fn needed(
        &self,
        pack: &PackGpu,
        textures: Option<&ShaderpackEffectTextures>,
    ) -> bool {
        !self.world_ready()
            || self.first_person_native()
            || needs_native_scene(
                self.atmosphere.as_deref(),
                self.particles.as_deref(),
                self.weather.as_deref(),
                self.overlays.as_deref(),
                self.nametags.as_deref(),
                |effect| {
                    !pack.effects.draws(effect)
                        || (pack
                            .draws
                            .iter()
                            .any(|draw| draw.kind == DrawKind::Effect(effect))
                            && textures.is_some_and(|textures| textures.views(effect).is_some()))
                },
            )
    }
}

/// Uses one replacement decision for view uniforms and the native queue handoff.
pub(super) fn replacement_ready(
    pack: &Arc<PackGpu>,
    cameras: impl Iterator<Item = Entity>,
    geometry: Option<&ShaderpackGeometry>,
    states: &HashMap<Entity, ViewState>,
    native_scene: bool,
) -> bool {
    let mut cameras = cameras.peekable();
    !native_scene
        && geometry.is_some_and(|geometry| {
            geometry.supported() && geometry.origin().is_some() && geometry.all_sections_fit()
        })
        && super::super::draws::complete_world(|kind| {
            pack.draws.iter().any(|draw| draw.kind == kind)
        })
        && cameras.peek().is_some()
        && cameras.all(|entity| {
            states.get(&entity).is_some_and(|state| {
                Arc::ptr_eq(&state.pack, pack) && state.geometry_targets_ready()
            })
        })
}
