//! The shared presentation imports, linked to either a personal or a bundle-owned state.

use super::*;
use wasmtime::component::{HasSelf, Linker};

impl State {
    /// Links the canonical imports directly to the same host implementation in both contexts.
    pub(crate) fn link_presentation<T: 'static>(
        linker: &mut Linker<T>,
        get: fn(&mut T) -> &mut State,
    ) -> Result<()> {
        cinnabar::world_draw::drawing::add_to_linker::<_, HasSelf<_>>(linker, get)?;
        cinnabar::world_draw::masks::add_to_linker::<_, HasSelf<_>>(linker, get)?;
        cinnabar::world_draw::assets::add_to_linker::<_, HasSelf<_>>(linker, get)?;
        cinnabar::extension::render::add_to_linker::<_, HasSelf<_>>(linker, get)?;
        cinnabar::extension::environment::add_to_linker::<_, HasSelf<_>>(linker, get)?;
        Ok(())
    }

    /// Invalidates all presentation handles before the guest rebuilds for another world.
    pub(crate) fn reset_presentation(&mut self) {
        self.render.revoke();
        if let Some(session) = self.drawing_policy {
            self.render
                .retained
                .bind_session(session.connection, session.dimension);
            self.render.masks.bind(session);
        }
        self.time_override = None;
        self.weather_override = None;
        self.fullbright = false;
    }

    /// Installs trusted native actor references before calling any guest import.
    pub(crate) fn presentation_context(
        &mut self,
        context: &crate::presentation::Context,
    ) -> Result<()> {
        let session = mod_api::drawing::DrawingSession {
            connection: context.connection,
            dimension: context.dimension,
            local: false,
            permissions: self.grants.drawing_permissions(),
        };
        if self.drawing_policy != Some(session) {
            if !self
                .render
                .retained
                .attach_session(context.connection, context.dimension)
            {
                self.render
                    .retained
                    .bind_session(context.connection, context.dimension);
            }
            self.render.masks.bind(session);
            self.drawing_policy = Some(session);
        }
        let removed = self
            .render
            .retained
            .snapshot()
            .entities
            .values()
            .copied()
            .filter(|key| !context.live.contains(key))
            .collect::<Vec<_>>();
        self.render
            .update_entities(context.candidates.clone(), &removed)
            .map_err(|error| anyhow::anyhow!("drawing entities: {}", error.detail))?;
        self.render.masks.publish_targets(&context.targets);
        Ok(())
    }

    /// Begins a presentation transaction without altering the last committed snapshot.
    pub(crate) fn begin_presentation(&mut self) {
        self.render.begin_frame();
        self.environment_writes = 0;
        self.pending_weather = None;
        self.pending_time = None;
        self.pending_fullbright = None;
    }

    /// Publishes geometry and environment together after a successful bundle callback.
    pub(crate) fn commit_presentation(&mut self) {
        self.render.commit();
        if let Some(value) = self.pending_weather.take() {
            self.weather_override = value;
        }
        if let Some(value) = self.pending_time.take() {
            self.time_override = value;
        }
        if let Some(value) = self.pending_fullbright.take() {
            self.fullbright = value;
        }
    }

    /// Includes environment changes in the shared committed presentation identity.
    pub(crate) fn presentation_key(&self) -> (u64, Option<u32>, Option<[f32; 2]>) {
        (
            self.render.generation(),
            self.time_override,
            self.weather_override.map(|weather| match weather {
                cinnabar::extension::environment::Weather::Clear => [0., 0.],
                cinnabar::extension::environment::Weather::Rain => [1., 0.],
                cinnabar::extension::environment::Weather::Thunder => [1., 1.],
            }),
        )
    }

    /// Copies portable inputs only; native shader modules and cached bounds remain local.
    pub(crate) fn presentation_output(&self) -> crate::presentation::Output {
        let output = self.render.output();
        crate::presentation::Output {
            drawing: mod_render::retained::transfer::Snapshot::from_resources(
                self.render.retained.snapshot(),
            ),
            primitives: (*output.primitives).clone(),
            passes: output
                .passes
                .iter()
                .map(|pass| crate::presentation::Pass {
                    name: pass.name.clone(),
                    order: pass.order,
                    source: pass.source.to_string(),
                    enabled: pass.enabled,
                    params: pass.params,
                    masks: pass.masks.clone(),
                })
                .collect(),
            masks: output.masks.to_vec(),
            time: self.time_override,
            weather: self.presentation_key().2,
        }
    }
}
