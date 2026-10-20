//! The per-session rig store actor authority drives through spawns, ticks and frames.
use super::*;
use simulation_clock::identity::{PositionAuthorityGeneration, SessionGeneration};

#[derive(Debug)]
pub struct ActorAnimationStore {
    pub(super) assets: Option<Arc<RuntimeEntityAssets>>,
    pub(super) layout: Arc<VariableLayout>,
    /// The session's server-pack entity catalog, in its own index space; its entities win.
    pub(super) pack: Option<PackCatalog>,
    pub(super) server_compiler: Option<ServerAnimationCompiler>,
    pub(super) rigs: BTreeMap<ActorLifetimeId, ActorRigState>,
    pub(super) skin_preparation: skin::SkinPreparationQueue,
    pub(super) runtime_to_lifetime: HashMap<u64, ActorLifetimeId>,
    pub(super) mount_equipment: HashMap<u64, assets::entity::mount::MountEquipment>,
    /// First actor the world budget skipped last tick, where the next tick starts.
    pub(super) first_starved: Option<ActorLifetimeId>,
    pub(super) local_motion_authority:
        Option<(u64, (SessionGeneration, PositionAuthorityGeneration))>,
    pub(super) completed_tick: u64,
    pub(super) next_reset_generation: u64,
    pub(super) next_rest_reset_generation: u64,
    pub(super) stats: ActorAnimationStats,
    /// Frame samples that failed or ran out of budget and drew the completed tick pair instead.
    pub(super) frame_fallbacks: std::sync::atomic::AtomicU64,
    #[cfg(any(test, feature = "test-support"))]
    pub(super) schedule: schedule::TestSchedule,
}

#[derive(Debug)]
pub(super) struct PackCatalog {
    /// Rig geometry bindings with artwork; a pack entity without any draws as vanilla does.
    pub(super) artwork: std::collections::BTreeSet<u32>,
    pub(super) assets: Arc<RuntimeEntityAssets>,
    pub(super) layout: Arc<VariableLayout>,
}

#[derive(Debug)]
pub(super) enum PoseStep {
    Tick {
        evaluate: bool,
        reset_motion_history: bool,
    },
    Frame {
        initialize_new: bool,
        refresh_local: Option<u64>,
    },
}

impl ActorAnimationStore {
    pub fn diagnostic() -> Self {
        Self::new(None)
    }

    pub fn with_assets(assets: Arc<RuntimeEntityAssets>) -> Self {
        Self::new(Some(assets))
    }

    fn new(assets: Option<Arc<RuntimeEntityAssets>>) -> Self {
        let mut store = Self {
            layout: Arc::new(assets.as_ref().map(VariableLayout::new).unwrap_or_default()),
            assets,
            pack: None,
            server_compiler: None,
            rigs: BTreeMap::new(),
            skin_preparation: skin::SkinPreparationQueue::default(),
            runtime_to_lifetime: HashMap::new(),
            mount_equipment: HashMap::new(),
            first_starved: None,
            local_motion_authority: None,
            completed_tick: 0,
            next_reset_generation: 1,
            next_rest_reset_generation: 1,
            stats: ActorAnimationStats::default(),
            frame_fallbacks: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            schedule: Default::default(),
        };
        store.prewarm_skin_catalog();
        store
    }

    /// Layers a server-pack entity catalog over the vanilla one for actors spawned afterwards.
    pub fn set_pack(&mut self, assets: Option<(Arc<RuntimeEntityAssets>, Vec<u32>)>) {
        self.pack = assets.map(|(assets, artwork)| {
            let mut layout = VariableLayout::new(&assets);
            if let Some(vanilla) = &self.assets {
                layout.plans.bind_attachments(vanilla);
            }
            PackCatalog {
                layout: Arc::new(layout),
                artwork: artwork.into_iter().collect(),
                assets,
            }
        });
    }

    pub fn set_server_animation_compiler(&mut self, compiler: ServerAnimationCompiler) {
        self.server_compiler = Some(compiler);
    }

    pub fn clear(&mut self) {
        self.layout.plans.clear();
        if let Some(pack) = &self.pack {
            pack.layout.plans.clear();
        }
        self.rigs.clear();
        self.skin_preparation = skin::SkinPreparationQueue::default();
        self.prewarm_skin_catalog();
        self.local_motion_authority = None;
        self.runtime_to_lifetime.clear();
        self.mount_equipment.clear();
        self.completed_tick = 0;
        self.bump_generation();
    }

    pub fn remove_runtime(&mut self, runtime_id: u64) {
        self.mount_equipment.remove(&runtime_id);
        if let Some(lifetime) = self.runtime_to_lifetime.remove(&runtime_id) {
            self.rigs.remove(&lifetime);
        }
    }

    pub fn insert(&mut self, session_id: u64, dimension: i32, actor: &dyn AnimatedActor) {
        self.remove_runtime(actor.runtime_id());
        self.mount_equipment
            .insert(actor.runtime_id(), self.resolve_mount_equipment(actor));
        let Some(assets) = self.assets.clone() else {
            return;
        };
        let lifetime = ActorLifetimeId {
            session_id,
            dimension,
            runtime_id: actor.runtime_id(),
            spawn_revision: actor.spawn_revision(),
        };
        let from_pack = self.pack.as_ref().and_then(|pack| {
            let mut state = resolve_rig(&pack.assets, &pack.layout, actor, self.completed_tick)?;
            if !pack.artwork.contains(&state.rig.0) {
                return None;
            }
            state.pack = true;
            state.rig = EntityRigId(assets::PACK_RIG_ID_BASE.checked_add(state.rig.0)?);
            Some(state)
        });
        let resolved =
            from_pack.or_else(|| resolve_rig(&assets, &self.layout, actor, self.completed_tick));
        let Some(mut state) = resolved else {
            self.stats.unrigged_spawns = self.stats.unrigged_spawns.saturating_add(1);
            return;
        };
        if let Some((runtime_id, authority)) = self.local_motion_authority
            && runtime_id == actor.runtime_id()
        {
            state
                .java
                .local_body
                .set_authority(Some(authority), &mut state.java.motion);
        }
        state.reset_generation = self.next_reset_generation;
        state.rest_reset_generation = self.take_rest_generation().unwrap_or(0);
        self.bump_generation();
        self.runtime_to_lifetime
            .insert(actor.runtime_id(), lifetime);
        self.rigs.insert(lifetime, state);
    }

    pub fn mark_reset(&mut self, runtime_id: u64) {
        let Some(lifetime) = self.runtime_to_lifetime.get(&runtime_id) else {
            return;
        };
        if let Some(state) = self.rigs.get_mut(lifetime) {
            state.reset_pending = true;
            state.rest_reset_pending = true;
            state.java.reset_cape();
        }
    }

    /// Reanchors displacement and cape history without restarting animation clocks or controllers.
    pub fn reanchor_position(&mut self, runtime_id: u64, position: [f32; 3]) {
        if !position.iter().all(|value| value.is_finite()) {
            return;
        }
        let Some(state) = self
            .runtime_to_lifetime
            .get(&runtime_id)
            .and_then(|id| self.rigs.get_mut(id))
        else {
            return;
        };
        if let Some(input) = state.history.back_mut() {
            state.java.reanchor_position(input.position, position);
            input.position = position;
        }
    }

    /// Drops Java's equip progress to zero at the actor's next tick.
    pub fn reset_java_equip(&mut self, runtime_id: u64) {
        let Some(lifetime) = self.runtime_to_lifetime.get(&runtime_id) else {
            return;
        };
        if let Some(state) = self.rigs.get_mut(lifetime) {
            state.java.reset_equip();
        }
    }

    /// Applies Java's limb boost on every hurt event, independently of the hurt countdown.
    pub fn hurt_java_limbs(&mut self, runtime_id: u64) {
        let Some(lifetime) = self.runtime_to_lifetime.get(&runtime_id) else {
            return;
        };
        if let Some(state) = self.rigs.get_mut(lifetime) {
            state.java.hurt();
        }
    }

    /// Restarts the Bedrock swing behind `variable.attack_time` and the Java swing, each
    /// unless still in the first half of its own current length.
    pub fn start_swing(&mut self, runtime_id: u64, bedrock_ticks: i32, java_ticks: i32) {
        let Some(lifetime) = self.runtime_to_lifetime.get(&runtime_id) else {
            return;
        };
        if let Some(state) = self.rigs.get_mut(lifetime) {
            state.local_swing = None;
            state.java.motion.local_swing_alpha = None;
            state.motion.set_local_swing(None);
            state.motion.start_swing(bedrock_ticks);
            state.java.start_swing(java_ticks);
        }
    }

    /// Installs authoritative local swing samples and requests evaluation only when they change.
    pub fn sync_local_swing(&mut self, runtime_id: u64, progress: LocalSwingProgress) -> bool {
        let Some(state) = self
            .runtime_to_lifetime
            .get(&runtime_id)
            .and_then(|id| self.rigs.get_mut(id))
        else {
            return false;
        };
        let changed = state
            .local_swing
            .is_none_or(|old| old.bedrock != progress.bedrock || old.java != progress.java);
        state.local_swing = Some(progress);
        state.motion.set_local_swing(Some(progress.bedrock[1]));
        state.java.motion.swing = progress.java;
        state.java.motion.local_swing_alpha = progress.frame_alpha;
        if state.java.local_body.active() {
            state.java.motion.body_frame_alpha = progress.frame_alpha;
        }
        if let Some(input) = state.history.back_mut() {
            input.attack_time = progress.bedrock[1];
        }
        changed
    }

    /// Advances tick state; only the frame's final tick evaluates visual controllers and poses.
    pub fn advance_tick<A: AnimatedActor>(
        &mut self,
        actors: &HashMap<u64, A>,
        view: Option<&ActorAnimationView>,
        exempt: Option<u64>,
        evaluate: bool,
        reset_motion_history: bool,
        context: impl Fn(&A) -> ActorTickContext,
    ) {
        self.evaluate_tick(
            actors,
            view,
            exempt,
            PoseStep::Tick {
                evaluate,
                reset_motion_history,
            },
            context,
        );
    }

    /// Changes the local draw context without advancing motion or clip time.
    #[cfg(test)]
    pub fn refresh_local_view<A: AnimatedActor>(
        &mut self,
        actors: &HashMap<u64, A>,
        runtime_id: u64,
        context: impl Fn(&A) -> ActorTickContext,
    ) {
        self.evaluate_tick(
            actors,
            None,
            Some(runtime_id),
            PoseStep::Frame {
                initialize_new: false,
                refresh_local: Some(runtime_id),
            },
            context,
        );
    }

    /// Prepares newly bound rigs and an optional changed local view without advancing simulation.
    pub fn prepare_frame<A: AnimatedActor>(
        &mut self,
        actors: &HashMap<u64, A>,
        view: Option<&ActorAnimationView>,
        exempt: Option<u64>,
        refresh_local: bool,
        context: impl Fn(&A) -> ActorTickContext,
    ) {
        self.evaluate_tick(
            actors,
            view,
            exempt,
            PoseStep::Frame {
                initialize_new: true,
                refresh_local: exempt.filter(|_| refresh_local),
            },
            context,
        );
    }

    /// Frame samples that fell back to the completed tick pair since the store was created.
    pub fn frame_fallbacks(&self) -> u64 {
        self.frame_fallbacks
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn get(&self, runtime_id: u64) -> Option<ActorRigSnapshot<'_>> {
        let lifetime = *self.runtime_to_lifetime.get(&runtime_id)?;
        self.snapshot(lifetime, self.rigs.get(&lifetime)?)
    }

    /// The local actor's full-body pose while the main evaluation drives first-person hands.
    pub fn ui_pose(&self, runtime_id: u64) -> Option<&[BoneTransform]> {
        let state = self.rigs.get(self.runtime_to_lifetime.get(&runtime_id)?)?;
        match state.ui_pose.as_deref() {
            Some([]) => None,
            Some(pose) => Some(pose),
            None => Some(&state.current),
        }
    }

    pub fn snapshots(&self) -> impl Iterator<Item = ActorRigSnapshot<'_>> {
        self.rigs
            .iter()
            .filter_map(|(&lifetime, state)| self.snapshot(lifetime, state))
    }

    pub const fn stats(&self) -> ActorAnimationStats {
        self.stats
    }

    fn snapshot<'a>(
        &'a self,
        actor: ActorLifetimeId,
        state: &'a ActorRigState,
    ) -> Option<ActorRigSnapshot<'a>> {
        if state.previous.len() != state.current.len() {
            return None;
        }
        Some(ActorRigSnapshot {
            placement_frame: state.placement_frame,
            actor,
            rig: state.rig,
            previous: &state.previous,
            current: &state.current,
            rest: &state.rest,
            rest_completed_tick: if state.rest_reset_pending {
                0
            } else {
                state.rest_completed_tick
            },
            rest_reset_generation: state.rest_reset_generation,
            completed_tick: state.completed_tick,
            reset_generation: state.reset_generation,
            pose_evaluated: state.initialized,
            fallback: state.fallback,
            scale: state.scale.map_or_else(
                || {
                    if state.pack {
                        self.pack.as_ref().map(|pack| &pack.assets)
                    } else {
                        self.assets.as_ref()
                    }
                    .and_then(|assets| assets.rig_bindings().get(state.rig_binding))
                    .map_or(1.0, |rig| rig.scale.get())
                },
                |scale| scale[0],
            ),
            axis_scale: state
                .scale
                .map_or([1.0; 3], |scale| [scale[1], scale[2], scale[3]]),
            held_item_scale: state.held_item_scale,
            previous_body_yaw: state.motion.previous_body_yaw,
            body_yaw: state.motion.body_yaw,
            render: &state.render,
            attachments: Some(
                state
                    .skin_skeleton()
                    .map_or(&state.attachment_bindings, |skeleton| {
                        &skeleton.prepared.attachments
                    }),
            ),
            bone_names: state.posed_bone_names(),
            skin_geometry: state
                .skin_skeleton()
                .map(|skeleton| &skeleton.prepared.geometry),
            skin_mesh: state
                .skin_skeleton()
                .and_then(|skeleton| skeleton.prepared.mesh.as_ref()),
            skin_layers: &state.skin_layers,
            hand: state.hand_phases(),
            main_hand_equipped: state.equipped_main.0.as_deref(),
            item_animation: state.hand_phases().map(ItemAnimationState::from),
            off_hand_animation: state.off_hand_animation,
            animation_variables: ActorAnimationVariables::new(
                if state.pack {
                    self.pack.as_ref().map(|pack| pack.assets.as_ref())
                } else {
                    self.assets.as_deref()
                },
                &state.variables,
                state.completed_tick.saturating_sub(state.lifetime_epoch),
                state.completed_tick.saturating_sub(state.animation_epoch),
            )
            .with_input(state.history.back().copied())
            .with_item_context(Some(&state.query_context), state.complete_spear_variables),
            java: state.java.motion,
            java_equipped: state.java.equipped(),
        })
    }

    pub(super) fn bump_generation(&mut self) {
        self.next_reset_generation = self.next_reset_generation.saturating_add(1);
    }

    pub(super) fn take_rest_generation(&mut self) -> Option<u64> {
        let next = self.next_rest_reset_generation.checked_add(1)?;
        let generation = self.next_rest_reset_generation;
        self.next_rest_reset_generation = next;
        Some(generation)
    }
}

/// The rig's authored scale times its largest per-axis scale, bounding its culling box.
pub(super) fn model_scale(state: &ActorRigState, assets: &RuntimeEntityAssets) -> f32 {
    let scale = state.scale.map_or_else(
        || {
            assets
                .rig_bindings()
                .get(state.rig_binding)
                .map_or(1.0, |rig| rig.scale.get())
        },
        |scale| scale[0],
    );
    let axes = state
        .scale
        .map_or([1.0; 3], |scale| [scale[1], scale[2], scale[3]]);
    scale
        * axes
            .iter()
            .fold(1.0_f32, |largest, axis| largest.max(axis.abs()))
}

fn resolve_rig(
    assets: &RuntimeEntityAssets,
    layout: &VariableLayout,
    actor: &dyn AnimatedActor,
    completed_tick: u64,
) -> Option<ActorRigState> {
    let identifier = match actor.kind() {
        ActorKind::Player { .. } => assets::facts::VanillaEntity::Player.identifier(),
        ActorKind::Entity { identifier } => identifier,
    };
    let entity_symbol = assets
        .symbol_candidates(EntityAssetKind::Entity, identifier)
        .first()?;
    let entity_symbol_index = assets
        .symbols()
        .iter()
        .position(|symbol| std::ptr::eq(symbol, entity_symbol))?;
    let rig_binding = assets
        .rig_bindings()
        .iter()
        .position(|rig| rig.entity_symbol as usize == entity_symbol_index)?;
    resolve_binding(assets, layout, actor, completed_tick, rig_binding)
}
