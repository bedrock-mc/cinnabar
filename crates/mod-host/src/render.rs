//! Transactional render capability: validated passes persist, primitives last one callback.

use super::{State, cinnabar::extension::render as wit};
use anyhow::{Result, bail};
use mod_api::{MAX_PASS_PARAMS, MAX_RENDER_PASSES};
use mod_render::{Pass, Primitives, RenderOutput};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// Render imports per callback; drawing and per-pass updates need more than other capabilities.
const MAX_RENDER_CALLS: u32 = 32;
/// Process-wide, so a reloaded instance never repeats a predecessor's identifiers.
static REVISION: AtomicU64 = AtomicU64::new(1);

fn next_revision() -> u64 {
    REVISION.fetch_add(1, Ordering::Relaxed)
}

pub(super) struct RenderState {
    committed: RenderOutput,
    generation: u64,
    staged_passes: Option<Vec<Pass>>,
    staged_primitives: Primitives,
    calls: u32,
    compiles_left: u32,
}

impl RenderState {
    pub fn new() -> Self {
        Self {
            committed: RenderOutput::default(),
            generation: next_revision(),
            staged_passes: None,
            staged_primitives: Primitives::default(),
            calls: 0,
            compiles_left: MAX_RENDER_PASSES as u32,
        }
    }

    pub fn output(&self) -> &RenderOutput {
        &self.committed
    }

    /// Changes whenever the committed output does, including across reloads.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn begin_frame(&mut self) {
        self.calls = 0;
        self.compiles_left = mod_api::MAX_PASS_COMPILES_PER_FRAME;
        self.staged_passes = None;
        self.staged_primitives = Primitives::default();
    }

    pub fn commit(&mut self) {
        let primitives = std::mem::take(&mut self.staged_primitives);
        let mut changed = false;
        if let Some(mut passes) = self.staged_passes.take() {
            passes.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.name.cmp(&b.name)));
            changed |= passes != self.committed.passes;
            self.committed.passes = passes;
        }
        // Unchanged content keeps its identity, so the renderer rebuilds and uploads nothing.
        if primitives != *self.committed.primitives {
            self.committed.primitives = Arc::new(primitives);
            changed = true;
        }
        if changed {
            self.generation = next_revision();
        }
    }

    pub fn revoke(&mut self) {
        self.staged_passes = None;
        self.staged_primitives = Primitives::default();
        if self.committed != RenderOutput::default() {
            self.committed = RenderOutput::default();
            self.generation = next_revision();
        }
    }

    fn budget(&mut self) -> Result<()> {
        self.calls += 1;
        if self.calls > MAX_RENDER_CALLS {
            bail!("render import budget exhausted");
        }
        Ok(())
    }

    fn staged(&mut self) -> &mut Vec<Pass> {
        self.staged_passes
            .get_or_insert_with(|| self.committed.passes.clone())
    }
}

impl State {
    fn render_allowed(&mut self) -> Result<Result<(), String>> {
        self.render.budget()?;
        Ok(if self.grants.render {
            Ok(())
        } else {
            Err("render capability denied".into())
        })
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
        if let Err(denied) = self.render_allowed()? {
            return Ok(Err(denied));
        }
        if !mod_render::pass_name_valid(&spec.name) {
            return Ok(Err(
                "pass names are 1–32 lowercase letters, digits, - or _".into()
            ));
        }
        if spec.depth && !self.grants.render_depth {
            return Ok(Err("render depth capability denied".into()));
        }
        let passes = self.render.staged();
        let existing = passes.iter().position(|pass| pass.name == spec.name);
        if let Some(index) = existing
            && *passes[index].source == spec.source
            && passes[index].depth == spec.depth
        {
            passes[index].order = spec.order;
            return Ok(Ok(()));
        }
        if existing.is_none() && passes.len() >= MAX_RENDER_PASSES {
            return Ok(Err("render pass budget exhausted".into()));
        }
        if self.render.compiles_left == 0 {
            return Ok(Err(
                "shader compile budget for this callback is spent".into()
            ));
        }
        self.render.compiles_left -= 1;
        let shader = match mod_render::shader::compose(&spec.source, spec.depth) {
            Ok(shader) => shader,
            Err(error) => return Ok(Err(error)),
        };
        let mut pass = Pass {
            name: spec.name,
            order: spec.order,
            depth: spec.depth,
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

    fn remove_pass(&mut self, name: String) -> Result<Result<(), String>> {
        if let Err(denied) = self.render_allowed()? {
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
        if let Err(denied) = self.render_allowed()? {
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
