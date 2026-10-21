//! Component adapter for owner-scoped actor masks.

use super::{
    State,
    cinnabar::world_draw::{drawing as types, masks as wit},
};
use mod_api::drawing::{DrawingHandle, MaskDepth};
use mod_render::masks::MaskError;

impl crate::ModHost {
    /// Supplies trusted admitted actors before callbacks; callers must use the render frame's
    /// session, spawn generation and combined body/equipment vertex charge.
    pub fn publish_mask_targets(&mut self, targets: &[(mod_api::drawing::MaskEntity, u64)]) {
        self.instance.publish_mask_targets(targets);
    }
}

impl super::Instance {
    /// Replaces candidate tokens without disclosing positions or model data to the component.
    pub(super) fn publish_mask_targets(&mut self, targets: &[(mod_api::drawing::MaskEntity, u64)]) {
        self.store.data_mut().render.masks.publish_targets(targets);
    }
}

/// Converts the component token without dropping any owner or generation bits.
fn handle(value: types::Handle) -> DrawingHandle {
    DrawingHandle {
        owner: value.owner,
        generation: value.generation,
        slot: value.slot,
    }
}

/// Returns a domain token through the component ABI.
fn token(value: DrawingHandle) -> types::Handle {
    types::Handle {
        owner: value.owner,
        generation: value.generation,
        slot: value.slot,
    }
}

/// Keeps typed failures separate from traps and callback-budget exhaustion.
fn error(value: MaskError) -> types::DrawError {
    let (code, detail) = match value {
        MaskError::Invalid => (types::ErrorCode::Invalid, "invalid mask appearance"),
        MaskError::Denied => (types::ErrorCode::Denied, "entity mask authority denied"),
        MaskError::StaleHandle => (
            types::ErrorCode::StaleHandle,
            "unknown owner or actor incarnation",
        ),
        MaskError::BudgetExceeded => (
            types::ErrorCode::BudgetExceeded,
            "entity mask selection budget exceeded",
        ),
    };
    types::DrawError {
        code,
        detail: detail.into(),
    }
}

impl wit::Host for State {
    fn entity(
        &mut self,
        runtime_id: u64,
    ) -> anyhow::Result<Result<types::Handle, types::DrawError>> {
        self.render.mask_budget()?;
        Ok(self
            .render
            .masks
            .entity(runtime_id)
            .map(token)
            .map_err(error))
    }

    fn create(
        &mut self,
        depth: wit::DepthPolicy,
    ) -> anyhow::Result<Result<types::Handle, types::DrawError>> {
        self.render.mask_budget()?;
        let depth = match depth {
            wit::DepthPolicy::Test => MaskDepth::Test,
            wit::DepthPolicy::Always => MaskDepth::Always,
        };
        Ok(self.render.masks.create(depth).map(token).map_err(error))
    }

    fn select(
        &mut self,
        mask: types::Handle,
        entities: Vec<types::Handle>,
    ) -> anyhow::Result<Result<(), types::DrawError>> {
        self.render.mask_budget()?;
        if entities.len() > mod_api::drawing::MAX_MASK_ENTITIES {
            return Ok(Err(error(MaskError::BudgetExceeded)));
        }
        Ok(self
            .render
            .masks
            .select(
                handle(mask),
                &entities.into_iter().map(handle).collect::<Vec<_>>(),
            )
            .map_err(error))
    }

    fn remove(&mut self, mask: types::Handle) -> anyhow::Result<Result<(), types::DrawError>> {
        self.render.mask_budget()?;
        if self.render.mask_in_use(handle(mask)) {
            return Ok(Err(types::DrawError {
                code: types::ErrorCode::InUse,
                detail: "remove effects that reference this mask first".into(),
            }));
        }
        Ok(self.render.masks.remove(handle(mask)).map_err(error))
    }

    fn set_style(
        &mut self,
        mask: types::Handle,
        appearance: Option<wit::Style>,
    ) -> anyhow::Result<Result<(), types::DrawError>> {
        self.render.mask_budget()?;
        let rgba = |value: wit::Rgba| [value.r, value.g, value.b, value.a];
        let style = appearance.map(|value| mod_api::drawing::MaskStyle {
            fill: None,
            outline: rgba(value.outline),
            outline_radius: value.outline_radius,
            glow: rgba(value.glow),
            glow_radius: value.glow_radius,
        });
        Ok(self
            .render
            .masks
            .set_style(handle(mask), style)
            .map_err(error))
    }

    fn register_effect(
        &mut self,
        spec: wit::EffectSpec,
    ) -> anyhow::Result<Result<(), types::DrawError>> {
        self.render.mask_budget()?;
        if spec.masks.len() > mod_api::drawing::MAX_MASKS {
            return Ok(Err(error(MaskError::BudgetExceeded)));
        }
        let masks: Vec<_> = spec.masks.into_iter().map(handle).collect();
        if let Err(failure) = self.render.masks.bindings(&masks) {
            return Ok(Err(error(failure)));
        }
        if !self.grants.render_post {
            return Ok(Err(error(MaskError::Denied)));
        }
        let spec = super::cinnabar::extension::render::PassSpec {
            name: spec.name,
            source: spec.source,
            order: spec.order,
            depth: false,
        };
        self.register_render_pass(spec, masks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wit::Host as _;

    #[test]
    fn mask_imports_are_denied_before_session_binding_and_have_a_callback_budget() {
        let mut state = State::new(
            crate::ModGrants::default(),
            String::new(),
            Default::default(),
        );
        assert_eq!(
            state
                .create(wit::DepthPolicy::Test)
                .unwrap()
                .unwrap_err()
                .code,
            types::ErrorCode::Denied
        );
        state.render.begin_callback();
        for _ in 0..mod_api::drawing::MAX_MASK_CALLS {
            assert_eq!(
                state.entity(1).unwrap().unwrap_err().code,
                types::ErrorCode::Denied
            );
        }
        assert!(state.entity(1).is_err());
        state.render.begin_callback();
        assert!(state.entity(1).is_ok());
    }

    #[test]
    fn successful_mask_transactions_publish_once_and_revoke_immediately() {
        let mut state = State::new(
            crate::ModGrants::default(),
            String::new(),
            Default::default(),
        );
        state.render.masks.bind(mod_api::drawing::DrawingSession {
            connection: 7,
            permissions: mod_api::drawing::DrawingPermissions::ALL,
            ..Default::default()
        });
        let mask = state.create(wit::DepthPolicy::Test).unwrap().unwrap();
        assert!(state.render.output().masks.is_empty());
        state.render.commit_retained();
        let generation = state.render.generation();
        assert_eq!(state.render.output().masks.len(), 1);
        state.render.begin_callback();
        state.render.commit_retained();
        assert_eq!(state.render.generation(), generation);
        state.render.revoke();
        assert!(state.render.output().masks.is_empty());
        assert_eq!(
            state.remove(mask).unwrap().unwrap_err().code,
            types::ErrorCode::StaleHandle
        );
    }

    #[test]
    fn post_effects_require_separate_authority_and_hold_only_declared_masks() {
        let mut state = State::new(
            crate::ModGrants::default(),
            String::new(),
            Default::default(),
        );
        state.render.masks.bind(mod_api::drawing::DrawingSession {
            connection: 7,
            permissions: mod_api::drawing::DrawingPermissions::ALL,
            ..Default::default()
        });
        let mask = state.create(wit::DepthPolicy::Test).unwrap().unwrap();
        let spec = wit::EffectSpec { name: "selection".into(), order: 0, source: "fn effect(uv: vec2<f32>) -> vec3<f32> { return scene(uv) + vec3<f32>(mask(0u, uv)); }".into(), masks: vec![mask] };
        assert_eq!(
            state
                .register_effect(spec.clone())
                .unwrap()
                .unwrap_err()
                .code,
            types::ErrorCode::Denied
        );
        state.grants.render_post = true;
        state.render.commit_retained();
        state.render.begin_frame();
        state.register_effect(spec.clone()).unwrap().unwrap();
        assert_eq!(
            state
                .register_effect(wit::EffectSpec {
                    name: "exhausted".into(),
                    ..spec
                })
                .unwrap()
                .unwrap_err()
                .code,
            types::ErrorCode::BudgetExceeded
        );
        state.render.commit_retained();
        assert_eq!(state.render.output().passes.len(), 1);
        assert_eq!(
            state.remove(mask).unwrap().unwrap_err().code,
            types::ErrorCode::InUse
        );
        use super::super::cinnabar::extension::render::Host as _;
        state.remove_pass("selection".into()).unwrap().unwrap();
        state.remove(mask).unwrap().unwrap();
        state.render.commit_retained();
        assert!(state.render.output().masks.is_empty());
        assert!(state.render.output().passes.is_empty());
    }
}
