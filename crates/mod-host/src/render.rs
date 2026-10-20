//! Transactional render capability: validated passes persist, primitives last one callback.

use super::cinnabar::world_draw::drawing::{DrawError, ErrorCode};
use super::{State, cinnabar::extension::render as wit};
use anyhow::{Result, bail};
use mod_api::{MAX_PASS_PARAMS, MAX_RENDER_CALLS, MAX_RENDER_PASSES};
use mod_render::{Pass, Primitives, RenderOutput};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// Process-wide, so a reloaded instance never repeats a predecessor's identifiers.
static REVISION: AtomicU64 = AtomicU64::new(1);

/// Allocates a process-unique identity across reloads and owners.
fn next_revision() -> u64 {
    REVISION.fetch_add(1, Ordering::Relaxed)
}

/// Preserves the admission category while legacy callers continue receiving plain detail.
fn admission(code: ErrorCode, detail: impl Into<String>) -> DrawError {
    DrawError {
        code,
        detail: detail.into(),
    }
}

pub(super) struct RenderState {
    pub(super) masks: mod_render::masks::MaskStore,
    committed: RenderOutput,
    pub(super) retained: mod_render::retained::Store,
    generation: u64,
    staged_passes: Option<Vec<Pass>>,
    staged_primitives: Primitives,
    calls: usize,
    mask_calls: u32,
    compiles_left: u32,
    native_ribbons: Vec<mod_render::Ribbon>,
    native_generation: u64,
    combined: Option<RenderOutput>,
}

impl RenderState {
    pub fn new() -> Self {
        Self {
            masks: mod_render::masks::MaskStore::new(next_revision()),
            committed: RenderOutput::default(),
            retained: Default::default(),
            generation: next_revision(),
            staged_passes: None,
            staged_primitives: Primitives::default(),
            calls: 0,
            mask_calls: 0,
            compiles_left: MAX_RENDER_PASSES as u32,
            native_ribbons: Vec::new(),
            native_generation: 0,
            combined: None,
        }
    }

    pub fn output(&self) -> &RenderOutput {
        self.combined.as_ref().unwrap_or(&self.committed)
    }

    /// Changes whenever the committed output does, including across reloads.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Starts mask and geometry transactions and restores their import allowances.
    pub fn begin_callback(&mut self) {
        self.masks.begin();
        self.retained.begin();
        self.calls = 0;
        self.mask_calls = 0;
    }

    /// Discards earlier immediate output and restores the frame's shader compile allowance.
    pub fn begin_frame(&mut self) {
        self.begin_callback();
        self.compiles_left = mod_api::MAX_PASS_COMPILES_PER_FRAME;
        self.staged_passes = None;
        self.staged_primitives = Primitives::default();
    }

    /// Publishes masks, geometry and effects together after a successful callback.
    pub fn commit_retained(&mut self) {
        let masks = self.masks.commit();
        let mut changed = masks != self.committed.masks;
        self.committed.masks = masks;
        self.retained.commit();
        changed |= self.publish_retained();
        if let Some(mut passes) = self.staged_passes.take() {
            passes.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.name.cmp(&b.name)));
            changed |= passes != self.committed.passes;
            self.committed.passes = passes;
        }
        if changed {
            self.generation = next_revision();
            self.refresh_native_lines();
        }
    }

    /// Publishes immediate primitives and retained changes after a successful frame.
    pub fn commit(&mut self) {
        let primitives = std::mem::take(&mut self.staged_primitives);
        let mut changed = false;
        self.commit_retained();
        // Unchanged content keeps its identity, so the renderer rebuilds and uploads nothing.
        if primitives != *self.committed.primitives {
            self.committed.primitives = Arc::new(primitives);
            changed = true;
        }
        if changed {
            self.generation = next_revision();
            self.refresh_native_lines();
        }
    }

    /// Publishes confirmed target removals before another callback can render stale anchors.
    pub(super) fn update_entities(
        &mut self,
        candidates: Vec<mod_render::retained::EntityKey>,
        despawned: &[mod_render::retained::EntityKey],
    ) -> Result<(), mod_render::retained::DrawError> {
        self.retained.reconcile_entities(candidates, despawned)?;
        if self.publish_retained() {
            self.generation = next_revision();
            self.refresh_native_lines();
        }
        Ok(())
    }

    /// Copies only the immutable snapshot identity into renderer output.
    fn publish_retained(&mut self) -> bool {
        let snapshot = self.retained.snapshot();
        if snapshot.geometry.is_empty()
            && snapshot.instances.is_empty()
            && snapshot.materials.is_empty()
        {
            let changed = !self.committed.retained.is_empty();
            self.committed.retained.clear();
            changed
        } else if matches!(self.committed.retained.as_slice(), [previous] if Arc::ptr_eq(previous, snapshot))
        {
            false
        } else {
            self.committed.retained.clear();
            self.committed.retained.push(Arc::clone(snapshot));
            true
        }
    }

    pub fn revoke(&mut self) {
        self.native_ribbons.clear();
        let native = self.combined.take().is_some();
        self.masks.revoke();
        self.retained.revoke();
        self.staged_passes = None;
        self.staged_primitives = Primitives::default();
        if native || self.committed != RenderOutput::default() {
            self.committed = RenderOutput::default();
            self.generation = next_revision();
            self.refresh_native_lines();
        }
    }

    /// Merges validated native outlines within the guest's existing shared primitive limits.
    pub(super) fn native_lines(&mut self, ribbons: Vec<mod_render::Ribbon>) {
        if ribbons == self.native_ribbons && self.native_generation == self.generation {
            return;
        }
        if ribbons != self.native_ribbons {
            self.native_ribbons = ribbons;
            self.generation = next_revision();
        }
        self.refresh_native_lines();
    }

    /// Rebuilds the overlay from the current retained and immediate renderer output.
    fn refresh_native_lines(&mut self) {
        if self.native_ribbons.is_empty() {
            self.combined = None;
        } else {
            let mut primitives = (*self.committed.primitives).clone();
            for ribbon in &self.native_ribbons {
                let _ = primitives.append_checked(Primitives {
                    ribbons: vec![ribbon.clone()],
                    ..Default::default()
                });
            }
            let mut combined = self.committed.clone();
            combined.primitives = Arc::new(primitives);
            self.combined = Some(combined);
        }
        self.native_generation = self.generation;
    }

    pub(super) fn budget(&mut self) -> Result<()> {
        self.calls += 1;
        if self.calls > MAX_RENDER_CALLS {
            bail!("render import budget exhausted");
        }
        Ok(())
    }

    /// Charges mask imports separately from legacy primitives and pass mutations.
    pub(super) fn mask_budget(&mut self) -> Result<()> {
        self.mask_calls += 1;
        if self.mask_calls > mod_api::drawing::MAX_MASK_CALLS {
            bail!("mask import budget exhausted");
        }
        Ok(())
    }

    /// Retained post effects hold their declared mask resources until removed or replaced.
    pub(super) fn mask_in_use(&self, handle: mod_api::drawing::DrawingHandle) -> bool {
        self.staged_passes
            .as_ref()
            .unwrap_or(&self.committed.passes)
            .iter()
            .any(|pass| pass.masks.contains(&handle))
    }

    fn staged(&mut self) -> &mut Vec<Pass> {
        self.staged_passes
            .get_or_insert_with(|| self.committed.passes.clone())
    }
}

impl State {
    /// Charges an import before testing primitive authority.
    fn render_allowed(&mut self) -> Result<Result<(), String>> {
        self.render.budget()?;
        Ok(if self.grants.render || self.grants.world_draw {
            Ok(())
        } else {
            Err("render capability denied".into())
        })
    }
}

impl State {
    /// Legacy post effects require explicit consent under the current world fence.
    fn post_allowed(&mut self) -> Result<Result<(), String>> {
        self.render.budget()?;
        Ok(
            if (self.grants.render || self.grants.render_post)
                && self
                    .drawing_policy
                    .is_none_or(|policy| policy.permissions.render_post)
            {
                Ok(())
            } else {
                Err("post effect capability denied".into())
            },
        )
    }
}

impl State {
    /// Admits either a legacy pass or an explicitly owner-bound masked pass.
    pub(super) fn register_render_pass(
        &mut self,
        spec: wit::PassSpec,
        masks: Vec<mod_api::drawing::DrawingHandle>,
    ) -> Result<Result<(), DrawError>> {
        if let Err(denied) = self.post_allowed()? {
            return Ok(Err(admission(ErrorCode::Denied, denied)));
        }
        if !mod_render::pass_name_valid(&spec.name) {
            return Ok(Err(admission(
                ErrorCode::Invalid,
                "pass names are 1–32 lowercase letters, digits, - or _",
            )));
        }
        if spec.depth && !self.grants.render_depth {
            return Ok(Err(admission(
                ErrorCode::Denied,
                "render depth capability denied",
            )));
        }
        let passes = self.render.staged();
        let existing = passes.iter().position(|pass| pass.name == spec.name);
        if let Some(index) = existing
            && *passes[index].source == spec.source
            && passes[index].depth == spec.depth
            && passes[index].masks == masks
        {
            passes[index].order = spec.order;
            return Ok(Ok(()));
        }
        if existing.is_none() && passes.len() >= MAX_RENDER_PASSES {
            return Ok(Err(admission(
                ErrorCode::BudgetExceeded,
                "render pass budget exhausted",
            )));
        }
        if self.render.compiles_left == 0 {
            return Ok(Err(admission(
                ErrorCode::BudgetExceeded,
                "shader compile budget for this callback is spent",
            )));
        }
        self.render.compiles_left -= 1;
        let shader = match mod_render::shader::compose_masked(&spec.source, spec.depth, masks.len())
        {
            Ok(shader) => shader,
            Err(error) => return Ok(Err(admission(ErrorCode::Invalid, error))),
        };
        let mut pass = Pass {
            name: spec.name,
            order: spec.order,
            depth: spec.depth,
            masks,
            source: spec.source.into(),
            shader: shader.into(),
            revision: next_revision(),
            enabled: true,
            params: [0.0; MAX_PASS_PARAMS],
        };
        let passes = self.render.staged();
        match existing {
            Some(index) => {
                pass.enabled = passes[index].enabled;
                pass.params = passes[index].params;
                passes[index] = pass;
            }
            None => passes.push(pass),
        }
        Ok(Ok(()))
    }
}

impl wit::Host for State {
    fn set_block_highlights(
        &mut self,
        spec: Option<wit::BlockHighlightSpec>,
    ) -> Result<Result<(), String>> {
        super::block_highlights::set(self, spec)
    }

    fn register_pass(&mut self, spec: wit::PassSpec) -> Result<Result<(), String>> {
        Ok(self
            .register_render_pass(spec, Vec::new())?
            .map_err(|error| error.detail))
    }

    fn remove_pass(&mut self, name: String) -> Result<Result<(), String>> {
        if let Err(denied) = self.post_allowed()? {
            return Ok(Err(denied));
        }
        let passes = self.render.staged();
        let before = passes.len();
        passes.retain(|pass| pass.name != name);
        Ok(if passes.len() < before {
            Ok(())
        } else {
            Err("unknown render pass".into())
        })
    }

    fn update_pass(
        &mut self,
        name: String,
        enabled: bool,
        params: Vec<f32>,
    ) -> Result<Result<(), String>> {
        if let Err(denied) = self.post_allowed()? {
            return Ok(Err(denied));
        }
        if params.len() > MAX_PASS_PARAMS || !params.iter().all(|value| value.is_finite()) {
            return Ok(Err("pass params must be at most 16 finite values".into()));
        }
        let mut values = [0.0; MAX_PASS_PARAMS];
        values[..params.len()].copy_from_slice(&params);
        let current = self
            .render
            .staged_passes
            .as_ref()
            .unwrap_or(&self.render.committed.passes)
            .iter()
            .find(|pass| pass.name == name);
        match current {
            None => return Ok(Err("unknown render pass".into())),
            // Per-frame updates are usually unchanged; those must not copy the pass list.
            Some(pass) if pass.enabled == enabled && pass.params == values => return Ok(Ok(())),
            Some(_) => {}
        }
        if let Some(pass) = self
            .render
            .staged()
            .iter_mut()
            .find(|pass| pass.name == name)
        {
            pass.enabled = enabled;
            pass.params = values;
        }
        Ok(Ok(()))
    }

    fn draw_lines(&mut self, lines: Vec<wit::Line>) -> Result<Result<(), String>> {
        if let Err(denied) = self.render_allowed()? {
            return Ok(Err(denied));
        }
        if lines.len() > mod_api::MAX_RENDER_RIBBONS {
            return Ok(Err("line count exceeds shared ribbon budget".into()));
        }
        let primitives = Primitives {
            ribbons: lines
                .into_iter()
                .map(|line| mod_render::Ribbon {
                    taper: false,
                    pattern: match line.pattern {
                        wit::LinePattern::Solid => mod_render::LinePattern::Solid,
                        wit::LinePattern::Dashed => mod_render::LinePattern::Dashed,
                        wit::LinePattern::Dotted => mod_render::LinePattern::Dotted,
                    },
                    points: line.points.into_iter().map(|p| [p.x, p.y, p.z]).collect(),
                    width: line.width,
                    color: [line.color.r, line.color.g, line.color.b, line.color.a],
                })
                .collect(),
            ..Default::default()
        };
        Ok(self.render.staged_primitives.append_checked(primitives))
    }

    fn draw(&mut self, primitives: wit::Primitives) -> Result<Result<(), String>> {
        if let Err(denied) = self.render_allowed()? {
            return Ok(Err(denied));
        }
        Ok(self
            .render
            .staged_primitives
            .append_checked(convert(primitives)))
    }
}

fn convert(primitives: wit::Primitives) -> Primitives {
    let point = |v: wit::Vector3| [v.x, v.y, v.z];
    let color = |c: wit::Rgba| [c.r, c.g, c.b, c.a];
    Primitives {
        decals: primitives
            .decals
            .into_iter()
            .map(|decal| mod_render::Decal {
                center: point(decal.center),
                radius: decal.radius,
                color: color(decal.color),
                progress: decal.progress,
                style: match decal.style {
                    wit::DecalStyle::Telegraph => mod_render::DecalStyle::Telegraph,
                    wit::DecalStyle::Disc => mod_render::DecalStyle::Disc,
                    wit::DecalStyle::Crater => mod_render::DecalStyle::Crater,
                    wit::DecalStyle::Shockwave => mod_render::DecalStyle::Shockwave,
                    wit::DecalStyle::Dust => mod_render::DecalStyle::Dust,
                },
            })
            .collect(),
        ribbons: primitives
            .ribbons
            .into_iter()
            .map(|ribbon| mod_render::Ribbon {
                taper: true,
                pattern: mod_render::LinePattern::Solid,
                points: ribbon.points.into_iter().map(point).collect(),
                width: ribbon.width,
                color: color(ribbon.color),
            })
            .collect(),
        beams: primitives
            .beams
            .into_iter()
            .map(|beam| mod_render::Beam {
                start: point(beam.start),
                end: point(beam.end),
                width: beam.width,
                color: color(beam.color),
                intensity: beam.intensity,
            })
            .collect(),
        billboards: primitives
            .billboards
            .into_iter()
            .map(|billboard| mod_render::Billboard {
                position: point(billboard.position),
                width: billboard.width,
                height: billboard.height,
                color: color(billboard.color),
                pattern: match billboard.pattern {
                    wit::BillboardPattern::Solid => mod_render::BillboardPattern::Solid,
                    wit::BillboardPattern::SoftDisc => mod_render::BillboardPattern::SoftDisc,
                    wit::BillboardPattern::Ring => mod_render::BillboardPattern::Ring,
                    wit::BillboardPattern::Spark => mod_render::BillboardPattern::Spark,
                    wit::BillboardPattern::Silhouette => mod_render::BillboardPattern::Silhouette,
                    wit::BillboardPattern::Sphere => mod_render::BillboardPattern::Sphere,
                    wit::BillboardPattern::Aura => mod_render::BillboardPattern::Aura,
                },
                upright: billboard.upright,
            })
            .collect(),
    }
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
