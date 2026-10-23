//! Component admission and replacement under the current drawing session authority.
use super::*;
use wasmtime::{
    Engine,
    component::{Component, HasSelf, Linker},
};

impl Instance {
    /// Revokes previously published drawing and installs the current admission ceiling.
    pub(crate) fn bind_drawing_policy(
        &mut self,
        grants: ModGrants,
        session: mod_api::drawing::DrawingSession,
    ) {
        let state = self.store.data_mut();
        state.render.revoke();
        state.block_highlights.revoke();
        state.grants = grants;
        state.drawing_policy = Some(session);
        state
            .render
            .retained
            .bind_session(session.connection, session.dimension);
        let mut effective = session;
        effective.permissions = state.grants.drawing_permissions();
        state.render.masks.bind(effective);
    }

    /// Reads the last committed actor references for native liveness reconciliation.
    pub(crate) fn drawing_entity_keys(
        &self,
    ) -> impl Iterator<Item = mod_render::retained::EntityKey> + '_ {
        self.store
            .data()
            .render
            .retained
            .snapshot()
            .entities
            .values()
            .copied()
    }

    /// Reconciles native actor lifetime changes outside guest callback transactions.
    pub(crate) fn update_drawing_entities(
        &mut self,
        candidates: Vec<mod_render::retained::EntityKey>,
        despawned: &[mod_render::retained::EntityKey],
    ) -> Result<(), mod_render::retained::DrawError> {
        self.store
            .data_mut()
            .render
            .update_entities(candidates, despawned)
    }

    /// Returns current effective grants rather than the retained loader consent.
    pub(crate) fn grants(&self) -> &ModGrants {
        &self.store.data().grants
    }

    /// Compiles a component and checks its imports and callbacks without running guest code.
    pub(crate) fn check(engine: &Engine, bytes: &[u8]) -> Result<()> {
        let component = Component::new(engine, bytes)?;
        let mut linker = Linker::new(engine);
        FilteredPlayerMod::add_to_linker::<_, HasSelf<_>>(&mut linker, |state: &mut State| state)?;
        exports::Exports::check(&linker.instantiate_pre(&component)?)
    }

    /// Initializes a candidate store without changing the published instance. An `extension`
    /// component's instantiation and `init` share `FRAME_FUEL`; a `player-mod` component's
    /// `init`, which may read the whole session, gets `LOAD_FUEL`.
    #[cfg(test)]
    pub(crate) fn new(
        engine: &Engine,
        bytes: &[u8],
        grants: ModGrants,
        settings: String,
        declared: Declared,
        panel_bundle: Option<Arc<ui::mod_panel::PanelBundle>>,
    ) -> Result<Self> {
        Self::new_with_drawing_policy(
            engine,
            bytes,
            grants,
            settings,
            declared,
            panel_bundle,
            None,
        )
    }

    /// Installs session limits before instantiation and init can call any import.
    pub(crate) fn new_with_drawing_policy(
        engine: &Engine,
        bytes: &[u8],
        grants: ModGrants,
        settings: String,
        declared: Declared,
        panel_bundle: Option<Arc<ui::mod_panel::PanelBundle>>,
        policy: Option<mod_api::drawing::DrawingSession>,
    ) -> Result<Self> {
        let grants = policy.map_or_else(
            || grants.clone(),
            |session| grants.for_drawing_session(session),
        );
        let component = Component::new(engine, bytes)?;
        let mut linker = Linker::new(engine);
        FilteredPlayerMod::add_to_linker::<_, HasSelf<_>>(&mut linker, |state: &mut State| state)?;
        let mut state = State::new(grants, settings, declared);
        state.drawing_policy = policy;
        if let Some(mut session) = policy {
            state
                .render
                .retained
                .bind_session(session.connection, session.dimension);
            session.permissions = state.grants.drawing_permissions();
            state.render.masks.bind(session);
        }
        if let Some(bundle) = panel_bundle.filter(|_| state.grants.controls) {
            state.controls.panel = Some(bundle.panel.clone());
            state.controls.bundle = Some(bundle);
        }
        let pre = linker.instantiate_pre(&component)?;
        exports::Exports::check(&pre)?;
        let mut store = Store::new(engine, state);
        store.limiter(|state| &mut state.limits);
        let initial = FRAME_FUEL;
        crate::execution::arm(&mut store, initial, true)?;
        let instance = pre.instantiate(&mut store)?;
        let (init, exports) = exports::Exports::find(&mut store, &instance)?;
        let instantiated = initial - store.get_fuel().unwrap_or(0).min(initial);
        let mut armed = initial;
        if exports.loads_session() {
            armed = LOAD_FUEL;
            crate::execution::arm(&mut store, armed, true)?;
        }
        init.call(&mut store, ())?;
        init.post_return(&mut store)?;
        commit(&mut store);
        let last_fuel = armed - store.get_fuel().unwrap_or(0).min(armed)
            + if exports.loads_session() {
                instantiated
            } else {
                0
            };
        Ok(Self {
            store,
            exports,
            status: InstanceStatus::Running,
            last_fuel,
            callback_budget: None,
            filter_identity: super::action_filter::next_identity(),
        })
    }

    /// Initializes a replacement through the bounded loading lane with its full guest limits.
    pub(crate) fn replacement(
        &self,
        engine: &Engine,
        bytes: &[u8],
        grants: ModGrants,
        settings: String,
        declared: Declared,
        panel_bundle: Option<Arc<ui::mod_panel::PanelBundle>>,
    ) -> Result<Self> {
        let budget = self.callback_budget.clone();
        if let Some(budget) = &budget {
            anyhow::ensure!(
                budget.reserve(LOAD_FUEL).is_some(),
                "loading callback deferred"
            );
        }
        let mut candidate = Self::new_with_drawing_policy(
            engine,
            bytes,
            grants,
            settings,
            declared,
            panel_bundle,
            self.store.data().drawing_policy,
        )?;
        candidate.callback_budget = budget;
        candidate.store.data_mut().exclusive = self.store.data().exclusive.clone();
        Ok(candidate)
    }

    /// Defers an on-frame hot reload when earlier callbacks have consumed the allowance.
    pub(crate) fn can_reload(&self) -> bool {
        self.callback_budget
            .as_ref()
            .is_none_or(|budget| budget.can_load())
    }
}
