//! Static neutral empty-hand adapter. Missing animation observers are capability
//! gaps, not evidence that the player is in a source-qualified idle state.
use crate::camera::FlyCamera;
use bevy::{
    camera::{Camera, Hdr, RenderTarget},
    ecs::system::SystemParam,
    prelude::*,
    window::WindowRef,
};
use client_ui::ui_runtime::UiRuntime;
use render::{
    ViewmodelCompletionGate, ViewmodelGeometry, ViewmodelMode, ViewmodelScene, ViewmodelSkin,
    ViewmodelToken,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// World and gameplay facts sampled by the host at the viewmodel preparation boundary.
pub struct ViewmodelWorld<'a> {
    pub stream: Option<&'a chunk_pipeline::WorldStream>,
    pub entity_assets: Option<&'a Arc<assets::RuntimeEntityAssets>>,
    pub runtime_assets: &'a Arc<assets::RuntimeAssets>,
    pub block_registry_hash: [u8; 32],
    pub renders_game: bool,
    pub movement: Option<ViewmodelAuthority>,
}

/// The local input authority identity; presentation never mutates the movement ticker.
#[derive(Clone, Copy)]
pub struct ViewmodelAuthority {
    pub session: u64,
    pub epoch: u64,
    pub authorized: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandFallback {
    Hidden,
    Ownership,
    ItemsUnknownOrHeld,
    Skin,
    Geometry,
    View,
    KnownActive,
}
#[derive(Default, Debug)]
pub struct HandStats {
    pub mode: Option<ViewmodelMode>,
    pub fallback: Option<HandFallback>,
    pub neutral_eligible_frames: u64,
    // Main-world fallback requests; not rendered/presented frame counters.
    pub cpu_fallback_requested_frames: u64,
    pub skin_validations: u64,
    pub gpu_rejections: u64,
    // Exact claims of this mode, never inferred values of missing observations.
    pub animation_parity_unavailable: bool,
    pub lighting_parity_unavailable: bool,
    pub avatar_model_identity_unavailable: bool,
}
#[derive(Default, Resource)]
pub struct HandAdapter {
    raw_skin: Option<Arc<[u8]>>,
    skin: Option<ViewmodelSkin>,
    skin_identity: [u8; 32],
    revision: u64,
    revision_exhausted: bool,
    cube: Option<CubeCache>,
    cube_observation: [i128; 4],
    cube_reason: u8,
    owner: Option<HandOwner>,
    // Retain the stream/UI association across Empty and failed owner changes.
    // Stream IDs are globally allocated with checked monotonic progression.
    local_stream_session: Option<(u64, u64)>,
    pub stats: HandStats,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HandOwner {
    Actor(client_world::ActorLifetimeId),
    Local {
        session: u64,
        actor_session: u64,
        dimension: i32,
        runtime: u64,
        epoch: u64,
    },
}
fn plain_cube_extra(extra: &[u8]) -> bool {
    // Nonshield plain item: no compound, no placement or breaking restrictions.
    // The zero-byte representation remains a supported legacy observation.
    extra.is_empty() || extra == [0; 10]
}
fn extra_shape_flags(extra: &[u8]) -> i128 {
    i128::from(extra.is_empty())
        | (i128::from(plain_cube_extra(extra)) << 1)
        | (i128::from(extra.len() == 10) << 2)
        | (i128::from(extra.get(..2) == Some(&[0; 2])) << 3)
        | (i128::from(extra.get(2..6) == Some(&[0; 4])) << 4)
        | (i128::from(extra.get(6..10) == Some(&[0; 4])) << 5)
        | ((extra.len() as i128) << 8)
}
fn stack_block_visual_id(
    stream: &chunk_pipeline::WorldStream,
    assets: &assets::RuntimeAssets,
    wire_id: i32,
) -> Option<u32> {
    let internal = stream.resolve_block_network_id(u32::from_ne_bytes(wire_id.to_ne_bytes()));
    match stream.network_id_mode() {
        assets::NetworkIdMode::Sequential => Some(internal),
        assets::NetworkIdMode::Hashed => assets.sequential_id_for_hash(internal),
    }
}
#[cfg(test)]
mod local_owner_tests {
    use super::*;
    #[test]
    fn held_block_geometry_uses_the_current_session_palette_without_changing_the_descriptor() {
        let assets = assets::RuntimeAssets::diagnostic();
        let mut stack = protocol::NetworkItemStack::empty();
        stack.block_runtime_id = 1;
        for internal in [7, 6] {
            let mut stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
                local_player_unique_id: 1,
                local_player_runtime_id: 1,
                dimension: 0,
                player_position: [0.0; 3],
                world_spawn_position: [0; 3],
                air_network_id: 0,
                block_network_ids_are_hashes: false,
            });
            stream.set_sequential_id_remap(assets::SequentialIdRemap::from_palette(
                vec![0, internal],
                internal + 1,
            ));
            assert_eq!(
                stack_block_visual_id(&stream, &assets, stack.block_runtime_id),
                Some(internal)
            );
            assert_eq!(stack.block_runtime_id, 1);
        }
    }

    #[test]
    fn plain_frame_requires_complete_empty_compound_and_restriction_shape() {
        assert!(plain_cube_extra(&[]));
        assert!(plain_cube_extra(&[0; 10]));
        for length in [1, 2, 9, 11] {
            assert!(!plain_cube_extra(&vec![0; length]));
        }
        for offset in 0..10 {
            let mut extra = [0; 10];
            extra[offset] = 1;
            assert!(!plain_cube_extra(&extra));
        }
        assert!(!plain_cube_extra(&[255, 255, 1, 10, 0, 0, 0]));
    }
    #[test]
    fn owner_domains_and_incarnations_change_revision_but_pose_does_not() {
        let mut adapter = HandAdapter::default();
        let local = HandOwner::Local {
            session: 1,
            actor_session: 1,
            dimension: 0,
            runtime: 1,
            epoch: 1,
        };
        adapter.select_owner(local).unwrap();
        let revision = adapter.revision;
        for _ in 0..10 {
            adapter.select_owner(local).unwrap();
            assert_eq!(adapter.revision, revision);
        }
        adapter
            .select_owner(HandOwner::Actor(client_world::ActorLifetimeId {
                session_id: 1,
                dimension: 0,
                runtime_id: 1,
                spawn_revision: 1,
            }))
            .unwrap();
        assert!(adapter.revision > revision);
        assert!(
            adapter
                .select_owner(HandOwner::Local {
                    session: 2,
                    actor_session: 1,
                    dimension: 0,
                    runtime: 1,
                    epoch: 1,
                })
                .is_none()
        );
        adapter.select_owner(local).unwrap();
        let revision = adapter.revision;
        adapter
            .select_owner(HandOwner::Local {
                session: 1,
                actor_session: 1,
                dimension: 0,
                runtime: 1,
                epoch: 2,
            })
            .unwrap();
        assert!(adapter.revision > revision);
        assert!(adapter.select_owner(local).is_none());
        adapter.revision = u64::MAX;
        assert!(
            adapter
                .select_owner(HandOwner::Actor(client_world::ActorLifetimeId {
                    session_id: 1,
                    dimension: 0,
                    runtime_id: 1,
                    spawn_revision: 2
                }))
                .is_none()
        );
        assert!(adapter.revision_exhausted);
    }
}
struct CubeCache {
    stack: assets::ItemStackIdentity,
    visual: assets::BlockVisualId,
    identifier: Option<Arc<str>>,
    slot: u8,
    world: Arc<assets::RuntimeAssets>,
    entities: Arc<assets::RuntimeEntityAssets>,
    geometry: ViewmodelGeometry,
    pixels: ViewmodelSkin,
}
impl HandAdapter {
    fn select_owner(&mut self, owner: HandOwner) -> Option<()> {
        if let HandOwner::Local {
            session,
            actor_session,
            ..
        } = owner
            && self
                .local_stream_session
                .is_some_and(|(previous_session, previous_stream)| {
                    actor_session < previous_stream
                        || (session != previous_session && actor_session == previous_stream)
                })
        {
            return None;
        }
        if let (
            Some(HandOwner::Local {
                session: old_session,
                actor_session: old_stream,
                epoch: old_epoch,
                ..
            }),
            HandOwner::Local {
                session,
                actor_session,
                epoch,
                ..
            },
        ) = (self.owner, owner)
            && old_session == session
            && old_stream == actor_session
            && epoch < old_epoch
        {
            return None;
        }
        if self.owner != Some(owner) {
            self.advance_revision()?;
            self.owner = Some(owner);
        }
        if let HandOwner::Local {
            session,
            actor_session,
            ..
        } = owner
        {
            self.local_stream_session = Some((session, actor_session));
        }
        Some(())
    }
    fn advance_revision(&mut self) -> Option<()> {
        match self.revision.checked_add(1) {
            Some(next) if !self.revision_exhausted => {
                self.revision = next;
                Some(())
            }
            _ => {
                self.revision_exhausted = true;
                self.cube = None;
                self.skin = None;
                None
            }
        }
    }
    fn cube(
        &mut self,
        stack: &protocol::NetworkItemStack,
        slot: u8,
        world: &ViewmodelWorld<'_>,
    ) -> Option<(ViewmodelGeometry, ViewmodelSkin)> {
        let diagnostic = ViewmodelCompletionGate::observation_enabled();
        if diagnostic {
            self.cube_reason = 1;
            self.cube_observation = [0; 4];
        }
        let stream = world.stream?;
        let entities = world.entity_assets?;
        let canonical = stream.authority().canonical_item_stack(stack)?;
        if diagnostic {
            self.cube_reason = 2;
        }
        let assets::ItemVisualRoute::BlockItem(visual) = canonical.visual else {
            return None;
        };
        if diagnostic {
            self.cube_observation = [1, i128::from(visual.0), 0, 0];
            self.cube_reason = 3;
        }
        if !plain_cube_extra(&stack.extra_data)
            || entities.source_manifest_sha256()
                != world.runtime_assets.provenance().source_manifest_sha256
            || entities.block_visual_count() as usize != world.runtime_assets.visual_count()
        {
            if diagnostic {
                self.cube_reason = if !plain_cube_extra(&stack.extra_data) {
                    3
                } else if entities.source_manifest_sha256()
                    != world.runtime_assets.provenance().source_manifest_sha256
                {
                    8
                } else {
                    9
                };
            }
            return None;
        }
        if stack.block_runtime_id != 0 {
            let sequential =
                stack_block_visual_id(stream, world.runtime_assets, stack.block_runtime_id);
            if diagnostic {
                self.cube_observation[2] = sequential.map_or(-1, i128::from);
                self.cube_reason = 4;
            }
            if sequential != Some(visual.0) {
                return None;
            }
        }
        if self.cube.as_ref().is_none_or(|old| {
            old.stack != canonical.identity
                || old.visual != visual
                || old.identifier != canonical.identifier
                || old.slot != slot
                || !Arc::ptr_eq(&old.world, world.runtime_assets)
                || !Arc::ptr_eq(&old.entities, entities)
        }) {
            let registry: [u8; 32] = world.block_registry_hash;
            if diagnostic {
                self.cube_reason = 5;
            }
            if world.runtime_assets.provenance().block_registry_sha256 != registry {
                return None;
            }
            if diagnostic {
                self.cube_reason = 6;
            }
            let (geometry, pixels) = ViewmodelGeometry::opaque_cube(world.runtime_assets, visual)?;
            if diagnostic {
                self.cube_reason = 7;
            }
            self.advance_revision()?;
            self.cube = Some(CubeCache {
                stack: canonical.identity,
                visual,
                identifier: canonical.identifier,
                slot,
                world: Arc::clone(world.runtime_assets),
                entities: Arc::clone(entities),
                geometry,
                pixels,
            });
        }
        let cached = self.cube.as_ref()?;
        if diagnostic {
            self.cube_reason = 0;
        }
        Some((cached.geometry.clone(), cached.pixels.clone()))
    }
    fn skin(&mut self, raw: &protocol::StandardSkin) -> Option<ViewmodelSkin> {
        if raw.width != 64
            || raw.height != 64
            || raw.rgba8.len() != 64 * 64 * 4
            || self.revision_exhausted
        {
            return None;
        }
        if self
            .raw_skin
            .as_ref()
            .is_none_or(|old| !Arc::ptr_eq(old, raw.rgba8.pixels()))
        {
            self.raw_skin = Some(Arc::clone(raw.rgba8.pixels()));
            self.skin_identity = Sha256::digest(&raw.rgba8).into();
            self.skin = ViewmodelSkin::new(Arc::clone(raw.rgba8.pixels()), self.skin_identity);
            self.stats.skin_validations = self.stats.skin_validations.saturating_add(1);
            if let Some(next) = self.revision.checked_add(1) {
                self.revision = next;
            } else {
                self.revision_exhausted = true;
                self.skin = None;
            }
        }
        self.skin.clone()
    }
}

type ViewmodelCameras<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Camera,
        &'static RenderTarget,
        &'static Msaa,
        Option<&'static Hdr>,
    ),
    With<FlyCamera>,
>;

#[derive(SystemParam)]
pub struct ViewmodelPublish<'w, 's> {
    scene: Option<ResMut<'w, ViewmodelScene>>,
    gate: Option<Res<'w, ViewmodelCompletionGate>>,
    adapter: Option<ResMut<'w, HandAdapter>>,
    geometry: Option<Res<'w, ViewmodelGeometry>>,
    local_visibility: Option<Res<'w, crate::local_player::LocalAvatarVisibilityCarrier>>,
    cameras: ViewmodelCameras<'w, 's>,
}
impl ViewmodelPublish<'_, '_> {
    pub fn bind_cpu_fallback(
        &mut self,
        input: &render_model::UiRenderInput,
        empty: Option<ui::IconRef>,
        held: Option<ui::IconRef>,
    ) {
        let cube = self
            .scene
            .as_ref()
            .is_some_and(|scene| scene.is_opaque_cube());
        let icon = if cube { held.or(empty) } else { empty };
        if let (Some(scene), Some(gate), Some(icon)) = (&mut self.scene, &self.gate, icon) {
            if cube {
                scene.bind_cube_cpu_fallback(input, u32::from(icon.page), icon.uv, gate);
            } else {
                scene.bind_cpu_fallback(input, u32::from(icon.page), icon.uv, gate);
            }
        } else {
            self.clear();
        }
        if ViewmodelCompletionGate::observation_enabled() {
            let mut values = [0; 32];
            values[0] = i128::from(cube);
            values[1] = i128::from(icon.is_some());
            if let Some(icon) = icon {
                values[2] = i128::from(icon.page);
                values[4..8].copy_from_slice(&icon.uv.map(i128::from));
            }
            values[3] = self
                .scene
                .as_ref()
                .map_or(0, |scene| i128::from(scene.is_opaque_cube()));
            ViewmodelCompletionGate::observe_binding(if values[3] != 0 { 1 } else { 2 }, values);
        }
    }
    pub fn clear(&mut self) {
        if let (Some(scene), Some(gate)) = (&mut self.scene, &self.gate) {
            scene.clear(gate);
        }
        if let Some(adapter) = &mut self.adapter {
            adapter.stats.mode = None;
            adapter.stats.fallback = Some(HandFallback::View);
            adapter.stats.animation_parity_unavailable = false;
            adapter.stats.lighting_parity_unavailable = false;
            adapter.stats.avatar_model_identity_unavailable = false;
        }
    }

    /// The near-camera animated rig owns the hand this frame. Retire the static empty-hand scene
    /// so it never double-draws, and record the mode. No CPU fallback quad is bound.
    pub fn use_animated_rig(&mut self) {
        if let (Some(scene), Some(gate)) = (&mut self.scene, &self.gate) {
            scene.clear(gate);
        }
        if let Some(adapter) = &mut self.adapter {
            adapter.cube = None;
            adapter.stats.mode = Some(ViewmodelMode::AnimatedRig);
            adapter.stats.fallback = None;
            adapter.stats.animation_parity_unavailable = false;
            adapter.stats.lighting_parity_unavailable = false;
            adapter.stats.avatar_model_identity_unavailable = false;
        }
    }
    pub fn observe(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        world: &ViewmodelWorld<'_>,
        first_person: bool,
        hidden: bool,
        viewport: [u32; 2],
    ) -> bool {
        // UI-only and headless clients retain their existing CPU path without
        // installing the optional GPU hand resources.
        if self.adapter.is_none() || self.scene.is_none() || self.gate.is_none() {
            self.clear();
            self.record_observation(
                player_runtime,
                runtime,
                world,
                first_person,
                hidden,
                viewport,
                false,
            );
            return false;
        }
        let adapter = self.adapter.as_deref_mut().unwrap();
        adapter.stats.mode = None;
        adapter.stats.fallback = None;
        adapter.stats.animation_parity_unavailable = false;
        adapter.stats.lighting_parity_unavailable = false;
        adapter.stats.avatar_model_identity_unavailable = false;
        adapter.stats.gpu_rejections = self.gate.as_deref().unwrap().rejection_count();
        if ViewmodelCompletionGate::observation_enabled() {
            adapter.cube_reason = 255;
            adapter.cube_observation = [0; 4];
        }
        let result = self.publish(
            player_runtime,
            runtime,
            world,
            first_person,
            hidden,
            viewport,
        );
        let adapter = self.adapter.as_deref_mut().unwrap();
        let gate = self.gate.as_deref().unwrap();
        let completed = match result {
            Ok(token) => {
                adapter.stats.mode = Some(if adapter.cube.is_some() {
                    ViewmodelMode::OpaqueCubeNeutralStaticFallback
                } else {
                    ViewmodelMode::EmptyHandNeutralStaticFallback
                });
                adapter.stats.animation_parity_unavailable = true;
                adapter.stats.lighting_parity_unavailable = true;
                adapter.stats.avatar_model_identity_unavailable = true;
                adapter.stats.neutral_eligible_frames =
                    adapter.stats.neutral_eligible_frames.saturating_add(1);
                let completed = gate.completed(token);
                if !completed {
                    adapter.stats.cpu_fallback_requested_frames = adapter
                        .stats
                        .cpu_fallback_requested_frames
                        .saturating_add(1);
                }
                completed
            }
            Err(reason) => {
                self.scene.as_deref_mut().unwrap().clear(gate);
                adapter.cube = None;
                adapter.stats.fallback = Some(reason);
                adapter.stats.cpu_fallback_requested_frames = adapter
                    .stats
                    .cpu_fallback_requested_frames
                    .saturating_add(1);
                false
            }
        };
        self.record_observation(
            player_runtime,
            runtime,
            world,
            first_person,
            hidden,
            viewport,
            completed,
        );
        completed
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "Player authority is borrowed separately from UI state."
    )]
    fn record_observation(
        &self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        world: &ViewmodelWorld<'_>,
        first_person: bool,
        hidden: bool,
        viewport: [u32; 2],
        completed: bool,
    ) {
        if !ViewmodelCompletionGate::observation_enabled() {
            return;
        }
        let (reason, values) = self.diagnostic_snapshot(
            player_runtime,
            runtime,
            world,
            first_person,
            hidden,
            viewport,
            completed,
        );
        ViewmodelCompletionGate::observe_main(reason, values);
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "Player authority is borrowed separately from UI state."
    )]
    pub fn diagnostic_snapshot(
        &self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        world: &ViewmodelWorld<'_>,
        first_person: bool,
        hidden: bool,
        viewport: [u32; 2],
        completed: bool,
    ) -> (u8, [i128; 32]) {
        let mut values = [0; 32];
        values[0] = i128::from(runtime.session_id());
        values[20] = i128::from(
            runtime
                .hud()
                .health()
                .is_some_and(|health| health.current() == 0),
        ) | (i128::from(
            runtime
                .gameplay_hud()
                .air_ticks()
                .is_some_and(|(air, max)| air < max),
        ) << 1);
        values[14] = runtime
            .gameplay_hud()
            .offhand_is_empty()
            .map_or(0, |empty| if empty { 2 } else { 1 });
        if let Some(stream) = &world.stream {
            values[1] = i128::from(stream.authority().actor_session_id());
            values[2] = i128::from(stream.current_dimension());
            values[3] = i128::from(stream.local_player_runtime_id());
            if let Some(actor) = stream.authority().actor(stream.local_player_runtime_id()) {
                values[4] = 1;
                values[5] = i128::from(actor.spawn_revision);
                values[20] |= i128::from(stream.authority().actor_health_by_unique(actor.unique_id).is_some_and(|(health, _)| health <= 0.))
                    | (i128::from(matches!(actor.metadata.get(&0), Some(protocol::ActorMetadataValue::Flags(flags)) if flags & ((1 << 4) | (1 << 5)) != 0)) << 2)
                    | (i128::from(actor.metadata.get(&38).is_some_and(|value| !matches!(value, protocol::ActorMetadataValue::Float(scale) if *scale == 1.0))) << 3);
            }
            values[15] = match stream.network_id_mode() {
                assets::NetworkIdMode::Sequential => 1,
                assets::NetworkIdMode::Hashed => 2,
            };
            values[24] = i128::from(
                stream
                    .authority()
                    .actor_rig(stream.local_player_runtime_id())
                    .is_some(),
            );
        }
        if let Some(selected) = player_runtime.selected_stack_snapshot() {
            values[7] = i128::from(selected.slot);
            match selected.state {
                inventory::inventory_ledger::PlayerInventorySlot::Unknown => {}
                inventory::inventory_ledger::PlayerInventorySlot::Empty => values[6] = 1,
                inventory::inventory_ledger::PlayerInventorySlot::Present(stack) => {
                    values[6] = 2;
                    values[8..14].copy_from_slice(&[
                        i128::from(stack.network_id),
                        i128::from(stack.count),
                        i128::from(stack.metadata),
                        i128::from(stack.block_runtime_id),
                        i128::from(stack.stack_network_id),
                        extra_shape_flags(&stack.extra_data),
                    ]);
                }
            }
        }
        let mut reason = 1;
        if let Some(adapter) = &self.adapter {
            values[16..20].copy_from_slice(&[
                adapter.cube_observation[0],
                adapter.cube_observation[1],
                adapter.cube_observation[2],
                i128::from(adapter.cube_reason),
            ]);
            reason = match adapter.stats.fallback {
                None => 0,
                Some(HandFallback::Hidden) => 2,
                Some(HandFallback::Ownership) => 3,
                Some(HandFallback::ItemsUnknownOrHeld) => 4,
                Some(HandFallback::Skin) => 5,
                Some(HandFallback::Geometry) => 6,
                Some(HandFallback::View) => 7,
                Some(HandFallback::KnownActive) => 8,
            };
        }
        values[21] = self
            .scene
            .as_ref()
            .map_or(0, |scene| i128::from(scene.is_opaque_cube()));
        values[22] = i128::from(completed);
        values[23] = i128::from(self.geometry.is_some());
        if let Ok((_, camera, target, _, _)) = self.cameras.single() {
            values[25] = 1
                | (i128::from(camera.is_active) << 1)
                | (i128::from(camera.viewport.is_none()) << 2)
                | (i128::from(matches!(target, RenderTarget::Window(WindowRef::Primary))) << 3);
            values[26] =
                i128::from(camera.physical_viewport_size() == Some(UVec2::from_array(viewport)));
        }
        values[27..32].copy_from_slice(&[
            i128::from(hidden),
            i128::from(first_person),
            i128::from(runtime.ui_focused(player_runtime)),
            i128::from(viewport[0]),
            i128::from(viewport[1]),
        ]);
        (reason, values)
    }
    fn publish(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        world: &ViewmodelWorld<'_>,
        first_person: bool,
        hidden: bool,
        viewport: [u32; 2],
    ) -> Result<ViewmodelToken, HandFallback> {
        if hidden
            || !world.renders_game
            || !first_person
            || runtime.ui_focused(player_runtime)
            || player_runtime
                .facts
                .player_game_mode()
                .is_some_and(|mode| !mode.shows_hotbar())
        {
            return Err(HandFallback::Hidden);
        }
        let stream = world.stream.ok_or(HandFallback::Ownership)?;
        let actor = stream.authority().actor(stream.local_player_runtime_id());
        if runtime.session_id() == 0
            || runtime.local_runtime_id(player_runtime) != Some(stream.local_player_runtime_id())
        {
            return Err(HandFallback::Ownership);
        }
        if actor
            .and_then(|actor| stream.authority().actor_health_by_unique(actor.unique_id))
            .is_some_and(|(health, _)| health <= 0.)
            || runtime
                .hud()
                .health()
                .is_some_and(|health| health.current() == 0)
            || runtime
                .gameplay_hud()
                .air_ticks()
                .is_some_and(|(air, max)| air < max)
            || matches!(actor.and_then(|actor| actor.metadata.get(&0)), Some(protocol::ActorMetadataValue::Flags(flags))
                if flags & ((1 << 4) | (1 << 5)) != 0)
        {
            return Err(HandFallback::KnownActive);
        }
        // The retained scale field is a wire observation, not a fresh clock or
        // an inferred animation state. Reject a known nonordinary scale.
        if actor
            .and_then(|actor| actor.metadata.get(&38))
            .is_some_and(|value| {
                !matches!(value,
            protocol::ActorMetadataValue::Float(scale) if *scale == 1.0)
            })
        {
            return Err(HandFallback::Geometry);
        }
        let selected = player_runtime
            .selected_stack_snapshot()
            .ok_or(HandFallback::ItemsUnknownOrHeld)?;
        if runtime.gameplay_hud().offhand_is_empty() != Some(true) {
            return Err(HandFallback::ItemsUnknownOrHeld);
        }
        let (owner, camera, target, msaa, hdr) =
            self.cameras.single().map_err(|_| HandFallback::View)?;
        if !camera.is_active
            || camera.viewport.is_some()
            || !matches!(target, RenderTarget::Window(WindowRef::Primary))
            || camera.physical_viewport_size() != Some(UVec2::from_array(viewport))
        {
            return Err(HandFallback::View);
        }
        let local_owner = if matches!(
            selected.state,
            inventory::inventory_ledger::PlayerInventorySlot::Present(_)
        ) {
            let movement = world.movement.ok_or(HandFallback::Ownership)?;
            let visibility = self
                .local_visibility
                .as_deref()
                .and_then(|carrier| carrier.snapshot())
                .ok_or(HandFallback::Ownership)?;
            let (session, epoch) = (movement.session, movement.epoch);
            if !movement.authorized
                || session != runtime.session_id()
                || visibility.session_generation() != session
                || visibility.runtime_id() != stream.local_player_runtime_id()
                || epoch == 0
                || epoch == u64::MAX
            {
                return Err(HandFallback::Ownership);
            }
            Some(HandOwner::Local {
                session,
                actor_session: stream.authority().actor_session_id(),
                dimension: stream.current_dimension(),
                runtime: visibility.runtime_id(),
                epoch,
            })
        } else {
            None
        };
        let adapter = self.adapter.as_deref_mut().unwrap();
        let owner_identity =
            local_owner.unwrap_or(HandOwner::Actor(client_world::ActorLifetimeId {
                session_id: stream.authority().actor_session_id(),
                dimension: stream.current_dimension(),
                runtime_id: actor.map_or(0, |actor| actor.runtime_id),
                spawn_revision: actor.map_or(0, |actor| actor.spawn_revision),
            }));
        adapter
            .select_owner(owner_identity)
            .ok_or(HandFallback::Geometry)?;
        let (geometry, skin, lifetime) = match selected.state {
            inventory::inventory_ledger::PlayerInventorySlot::Empty => {
                let actor = actor.ok_or(HandFallback::Ownership)?;
                let rig = stream
                    .authority()
                    .actor_rig(actor.runtime_id)
                    .ok_or(HandFallback::Ownership)?;
                if rig.actor.session_id != stream.authority().actor_session_id()
                    || rig.actor.spawn_revision != actor.spawn_revision
                    || rig.actor.runtime_id != actor.runtime_id
                {
                    return Err(HandFallback::Ownership);
                }
                let geometry = self.geometry.as_ref().ok_or(HandFallback::Geometry)?;
                if !geometry.accepts_rig(rig.rig.0) {
                    return Err(HandFallback::Geometry);
                }
                if adapter.cube.take().is_some() {
                    adapter.advance_revision().ok_or(HandFallback::Geometry)?;
                }
                let profile = stream
                    .authority()
                    .actor_player_profile(actor.runtime_id)
                    .ok_or(HandFallback::Skin)?;
                let protocol::PlayerSkin::Standard(raw) = &profile.skin else {
                    return Err(HandFallback::Skin);
                };
                (
                    (**geometry).clone(),
                    adapter.skin(raw).ok_or(HandFallback::Skin)?,
                    rig.actor,
                )
            }
            inventory::inventory_ledger::PlayerInventorySlot::Present(stack) => {
                let Some(HandOwner::Local {
                    actor_session,
                    dimension,
                    runtime,
                    epoch,
                    ..
                }) = local_owner
                else {
                    return Err(HandFallback::Ownership);
                };
                let (geometry, skin) = adapter
                    .cube(stack, selected.slot, world)
                    .ok_or(HandFallback::ItemsUnknownOrHeld)?;
                (
                    geometry,
                    skin,
                    client_world::ActorLifetimeId {
                        session_id: actor_session,
                        dimension,
                        runtime_id: runtime,
                        // Local position-authority incarnation; owner domain is
                        // separately fenced by the checked adapter revision.
                        spawn_revision: epoch,
                    },
                )
            }
            inventory::inventory_ledger::PlayerInventorySlot::Unknown => {
                return Err(HandFallback::ItemsUnknownOrHeld);
            }
        };
        let token = ViewmodelToken {
            session: runtime.session_id(),
            actor_session: lifetime.session_id,
            dimension: lifetime.dimension,
            runtime: lifetime.runtime_id,
            spawn: lifetime.spawn_revision,
            owner,
            viewport,
            samples: msaa.samples(),
            hdr: hdr.is_some(),
            skin: skin.identity(),
            geometry: ViewmodelScene::geometry_identity(&geometry),
            revision: adapter.revision,
        };
        if !self.scene.as_deref_mut().unwrap().publish(
            token,
            &skin,
            &geometry,
            self.gate.as_deref().unwrap(),
        ) {
            return Err(HandFallback::View);
        }
        Ok(token)
    }
}
