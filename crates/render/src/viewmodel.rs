//! Neutral empty-hand foundation. Animation and environmental lighting are not
//! supplied by this mode; it is deliberately not an idle-animation parity claim.
use bevy::{prelude::*, render::extract_resource::ExtractResource};
use std::sync::{Arc, Mutex};
mod cube;
mod geometry;
#[cfg(test)]
mod tests;
mod witness;

pub(super) const VIEWMODEL_TEXTURE_SIDE: u32 = 64;
pub(super) const VIEWMODEL_TEXTURE_BYTES: usize =
    (VIEWMODEL_TEXTURE_SIDE * VIEWMODEL_TEXTURE_SIDE * 4) as usize;

pub const MAX_VIEWMODEL_DEPTH_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ViewmodelMode {
    EmptyHandNeutralStaticFallback,
    OpaqueCubeNeutralStaticFallback,
    /// The local player's own first-person rig drawn near-camera; retires the static fallbacks.
    AnimatedRig,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ViewmodelToken {
    pub session: u64,
    pub actor_session: u64,
    pub dimension: i32,
    pub runtime: u64,
    pub spawn: u64,
    pub owner: Entity,
    pub viewport: [u32; 2],
    pub samples: u32,
    pub hdr: bool,
    pub skin: [u8; 32],
    pub geometry: [u8; 32],
    pub revision: u64,
}

impl ViewmodelToken {
    fn valid(self) -> bool {
        self.session != 0
            && self.actor_session != 0
            && self.runtime != 0
            && self.spawn != 0
            && self.revision != 0
            && self.skin != [0; 32]
            && self.geometry != [0; 32]
            && viewmodel_depth_bytes(self.viewport, self.samples).is_some()
    }
}

pub fn viewmodel_depth_bytes(size: [u32; 2], samples: u32) -> Option<u64> {
    if size.contains(&0) || !matches!(samples, 1 | 2 | 4 | 8) {
        return None;
    }
    u64::from(size[0])
        .checked_mul(u64::from(size[1]))?
        .checked_mul(u64::from(samples))?
        .checked_mul(4)
        .filter(|bytes| *bytes <= MAX_VIEWMODEL_DEPTH_BYTES)
}

#[derive(Clone, Debug)]
pub struct ViewmodelSkin {
    pub(crate) rgba8: Arc<[u8]>,
    pub(crate) identity: [u8; 32],
}
impl ViewmodelSkin {
    pub fn identity(&self) -> [u8; 32] {
        self.identity
    }
    pub fn new(rgba8: Arc<[u8]>, identity: [u8; 32]) -> Option<Self> {
        (identity != [0; 32]
            && rgba8.len() == VIEWMODEL_TEXTURE_BYTES
            && rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| matches!(pixel[3], 0 | 255)))
        .then_some(Self { rgba8, identity })
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct HandVertex {
    position: [f32; 3],
    uv: [f32; 2],
}
const _: () = assert!(size_of::<HandVertex>() == 20);

#[derive(Clone, Debug, Resource)]
pub struct ViewmodelGeometry {
    pub(crate) vertices: Arc<[HandVertex]>,
    pub(crate) identity: [u8; 32],
    allowed_rigs: Arc<[u32]>,
    cube_origin: bool,
}
impl ViewmodelGeometry {
    pub fn from_runtime(
        assets: &assets::RuntimeEntityAssets,
        artwork: &crate::actor::ActorArtworkPages,
    ) -> Option<Self> {
        let candidates = assets.geometry_candidates("geometry.humanoid.custom");
        let [geometry] = candidates else {
            return None;
        };
        let mut result = geometry::validated_geometry(geometry, artwork.entity_identity)?;
        result.allowed_rigs = assets
            .rig_geometries()
            .iter()
            .enumerate()
            .filter_map(|(index, binding)| {
                let candidate = assets.geometries().get(binding.geometry as usize)?;
                (candidate == geometry).then_some(index as u32)
            })
            .collect::<Vec<_>>()
            .into();
        (!result.allowed_rigs.is_empty()).then_some(result)
    }
    pub fn accepts_rig(&self, rig: u32) -> bool {
        self.allowed_rigs.contains(&rig)
    }
}

#[derive(Clone, Debug)]
pub(super) struct HandFrame {
    pub token: ViewmodelToken,
    pub skin: ViewmodelSkin,
    pub geometry: ViewmodelGeometry,
    pub fallback: Option<(u64, u32, u32)>,
}

#[derive(Clone, Default, Debug, Resource, ExtractResource)]
pub struct ViewmodelScene {
    pub(super) frame: Option<HandFrame>,
}
impl ViewmodelScene {
    pub fn clear(&mut self, gate: &ViewmodelCompletionGate) {
        gate.select(None);
        self.frame = None;
    }
    pub fn publish(
        &mut self,
        token: ViewmodelToken,
        skin: &ViewmodelSkin,
        geometry: &ViewmodelGeometry,
        gate: &ViewmodelCompletionGate,
    ) -> bool {
        if !token.valid() || token.skin != skin.identity || token.geometry != geometry.identity {
            self.clear(gate);
            return false;
        }
        if self.frame.as_ref().is_some_and(|old| {
            old.token.skin == token.skin
                && !Arc::ptr_eq(&old.skin.rgba8, &skin.rgba8)
                && old.skin.rgba8 != skin.rgba8
        }) {
            self.clear(gate);
            return false;
        }
        gate.select(Some(token));
        if self.frame.as_ref().is_none_or(|frame| frame.token != token) {
            self.frame = Some(HandFrame {
                token,
                skin: skin.clone(),
                geometry: geometry.clone(),
                fallback: None,
            });
        }
        self.frame.as_mut().unwrap().fallback = None;
        true
    }
    /// Bind only a unique canonical CPU quad produced for the dedicated hand
    /// icon. Ambiguous or clipped/rewritten geometry leaves GPU admission off.
    pub fn bind_cpu_fallback(
        &mut self,
        input: &render_model::UiRenderInput,
        page: u32,
        uv: [u16; 4],
        gate: &ViewmodelCompletionGate,
    ) -> bool {
        self.bind_fallback(input, page, uv, gate, false)
    }
    pub fn is_opaque_cube(&self) -> bool {
        self.frame
            .as_ref()
            .is_some_and(|frame| frame.geometry.cube_origin)
    }
    /// Bind the original rotated held-item quad only for validated cube geometry.
    pub fn bind_cube_cpu_fallback(
        &mut self,
        input: &render_model::UiRenderInput,
        page: u32,
        uv: [u16; 4],
        gate: &ViewmodelCompletionGate,
    ) -> bool {
        if !self.is_opaque_cube() {
            self.clear(gate);
            return false;
        }
        self.bind_fallback(input, page, uv, gate, true)
    }
    fn bind_fallback(
        &mut self,
        input: &render_model::UiRenderInput,
        page: u32,
        uv: [u16; 4],
        gate: &ViewmodelCompletionGate,
        rotated: bool,
    ) -> bool {
        if self.frame.is_none() {
            return false;
        }
        if input.validate().is_err()
            || input.viewport_size != self.frame.as_ref().unwrap().token.viewport
        {
            self.clear(gate);
            return false;
        }
        let mut found = None;
        let expected = [
            [uv[0], uv[1]],
            [uv[2], uv[1]],
            [uv[2], uv[3]],
            [uv[0], uv[3]],
        ];
        for batch in input
            .batches
            .iter()
            .filter(|b| b.texture_page == page && b.blend_mode == render_model::UI_BLEND_ALPHA)
        {
            if batch.first_index % 3 != 0 || batch.index_count % 3 != 0 {
                self.clear(gate);
                return false;
            }
            let Some(end) = batch.first_index.checked_add(batch.index_count) else {
                self.clear(gate);
                return false;
            };
            let Some(indices) = input.indices.get(batch.first_index as usize..end as usize) else {
                self.clear(gate);
                return false;
            };
            for (quad, values) in indices.as_chunks::<6>().0.iter().enumerate() {
                if values[0] != values[3] || values[2] != values[4] {
                    continue;
                }
                let corners = [values[0], values[1], values[2], values[5]];
                let [Some(a), Some(b), Some(c), Some(d)] =
                    corners.map(|i| input.vertices.get(i as usize))
                else {
                    continue;
                };
                let axis_aligned = a.position[0] < b.position[0]
                    && a.position[1] < d.position[1]
                    && a.position[1] == b.position[1]
                    && b.position[0] == c.position[0]
                    && c.position[1] == d.position[1]
                    && d.position[0] == a.position[0];
                let rectangle = if rotated {
                    oriented_rectangle([a.position, b.position, c.position, d.position])
                        && quad_intersects_scissor(
                            [a.position, b.position, c.position, d.position],
                            batch.scissor,
                        )
                } else {
                    axis_aligned
                };
                let distinct = corners
                    .iter()
                    .enumerate()
                    .all(|(i, value)| !corners[..i].contains(value));
                let valid = rectangle
                    && distinct
                    && corners.into_iter().zip(expected).all(|(index, uv)| {
                        input.vertices.get(index as usize).is_some_and(|v| {
                            v.uv == uv.map(f32::from)
                                && v.color == [255; 4]
                                && v.style_flags == 0
                                && v.position.iter().all(|v| v.is_finite())
                        })
                    });
                if valid {
                    if found.is_some() {
                        self.clear(gate);
                        return false;
                    }
                    found = Some((input.revision, batch.first_index + quad as u32 * 6, page));
                }
            }
        }
        if input.revision == 0 || found.is_none() {
            self.clear(gate);
            return false;
        }
        self.frame.as_mut().unwrap().fallback = found;
        true
    }
    pub fn geometry_identity(geometry: &ViewmodelGeometry) -> [u8; 32] {
        geometry.identity
    }
}

fn oriented_rectangle([a, b, c, d]: [[f32; 2]; 4]) -> bool {
    if [a, b, c, d]
        .into_iter()
        .flatten()
        .any(|value| !value.is_finite())
    {
        return false;
    }
    let u = Vec2::from(b) - Vec2::from(a);
    let v = Vec2::from(d) - Vec2::from(a);
    let lengths = u.length_squared() * v.length_squared();
    if !lengths.is_finite() || lengths <= 0. || u.perp_dot(v) <= 0. {
        return false;
    }
    let tolerance = 0.0001;
    let closure = Vec2::from(c) - (Vec2::from(a) + u + v);
    u.dot(v).abs() <= tolerance * lengths.sqrt()
        && closure.length_squared()
            <= tolerance * tolerance * u.length_squared().max(v.length_squared())
}

fn quad_intersects_scissor(points: [[f32; 2]; 4], scissor: render_model::UiScissor) -> bool {
    let min = points
        .into_iter()
        .map(Vec2::from)
        .fold(Vec2::splat(f32::INFINITY), Vec2::min);
    let max = points
        .into_iter()
        .map(Vec2::from)
        .fold(Vec2::splat(f32::NEG_INFINITY), Vec2::max);
    min.x < (scissor.x + scissor.width) as f32
        && max.x > scissor.x as f32
        && min.y < (scissor.y + scissor.height) as f32
        && max.y > scissor.y as f32
}

#[derive(Default, Debug)]
struct CompletionState {
    epoch: u64,
    selected: Option<ViewmodelToken>,
    completed: bool,
    pending: bool,
    exhausted: bool,
    rejections: u64,
    rejected: bool,
}
#[derive(Clone, Default, Debug, Resource)]
pub struct ViewmodelCompletionGate(Arc<Mutex<CompletionState>>);
#[derive(Clone, Copy, Debug)]
pub(super) struct CompletionReservation {
    epoch: u64,
    token: ViewmodelToken,
}
impl ViewmodelCompletionGate {
    /// Enables bounded diagnostics only for an explicitly matching loopback marker.
    pub fn configure_observation(address: Option<&str>) {
        witness::configure(address);
    }
    pub fn retire_observation() {
        witness::retire();
    }
    pub fn observation_enabled() -> bool {
        witness::enabled()
    }
    /// Numeric diagnostic input, never admission or completion authority.
    pub fn observe_main(reason: u8, values: [i128; 32]) {
        witness::record(0, reason, values);
    }
    pub fn observe_binding(reason: u8, values: [i128; 32]) {
        witness::record(1, reason, values);
    }
    pub(crate) fn observe_stage(stage: usize, reason: u8, token: Option<ViewmodelToken>) {
        if !witness::enabled() {
            return;
        }
        let mut values = [0; 32];
        if let Some(token) = token {
            values[..9].copy_from_slice(&[
                i128::from(token.session),
                i128::from(token.actor_session),
                i128::from(token.dimension),
                i128::from(token.runtime),
                i128::from(token.spawn),
                i128::from(token.revision),
                i128::from(token.viewport[0]),
                i128::from(token.viewport[1]),
                i128::from(token.samples),
            ]);
        }
        witness::record(stage, reason, values);
    }
    pub fn select(&self, token: Option<ViewmodelToken>) {
        let mut state = self.0.lock().expect("hand completion lock");
        if state.selected != token {
            let next = state.epoch.checked_add(1);
            state.exhausted |= next.is_none();
            state.epoch = next.unwrap_or(state.epoch);
            state.selected = if state.exhausted { None } else { token };
            state.completed = false;
            state.pending = false;
            state.rejected = false;
        }
    }
    pub fn completed(&self, token: ViewmodelToken) -> bool {
        let state = self.0.lock().expect("hand completion lock");
        !state.exhausted && state.selected == Some(token) && state.completed
    }
    pub fn rejection_count(&self) -> u64 {
        self.0.lock().expect("hand completion lock").rejections
    }
    pub(super) fn rejected(&self, token: ViewmodelToken) -> bool {
        let state = self.0.lock().expect("hand completion lock");
        state.selected == Some(token) && state.rejected
    }
    pub(super) fn reject(&self, token: ViewmodelToken) {
        let mut state = self.0.lock().expect("hand completion lock");
        if state.selected == Some(token) {
            let next = state.epoch.checked_add(1);
            state.exhausted |= next.is_none();
            state.epoch = next.unwrap_or(state.epoch);
            state.completed = false;
            state.pending = false;
            state.rejected = true;
            state.rejections = state.rejections.saturating_add(1);
        }
    }
    pub(super) fn reserve(&self, token: ViewmodelToken) -> Option<CompletionReservation> {
        let mut state = self.0.lock().expect("hand completion lock");
        if state.exhausted || state.selected != Some(token) || state.pending || state.completed {
            return None;
        }
        state.pending = true;
        Some(CompletionReservation {
            epoch: state.epoch,
            token,
        })
    }
    pub(super) fn complete(&self, reservation: CompletionReservation) -> bool {
        let mut state = self.0.lock().expect("hand completion lock");
        if state.exhausted
            || state.epoch != reservation.epoch
            || state.selected != Some(reservation.token)
            || !state.pending
        {
            Self::observe_stage(5, 2, Some(reservation.token));
            return false;
        }
        state.pending = false;
        state.completed = true;
        state.rejected = false;
        Self::observe_stage(5, 1, Some(reservation.token));
        true
    }
}

pub(super) fn hand_projection(size: [u32; 2]) -> Mat4 {
    Mat4::perspective_infinite_reverse_rh(
        70.0_f32.to_radians(),
        size[0] as f32 / size[1] as f32,
        render_api::CAMERA_NEAR_PLANE_BLOCKS,
    )
}
