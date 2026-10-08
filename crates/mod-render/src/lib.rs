//! Validated render output of a personal mod: sandboxed post passes and world primitives.

pub mod geometry;
pub mod shader;

use mod_api::{
    MAX_PASS_PARAMS, MAX_PRIMITIVE_COORDINATE, MAX_PRIMITIVE_EXTENT_BLOCKS, MAX_RENDER_BEAMS,
    MAX_RENDER_BILLBOARDS, MAX_RENDER_DECALS, MAX_RENDER_PASSES, MAX_RENDER_RIBBONS,
    MAX_RIBBON_POINTS,
};
use std::sync::Arc;

pub type Vec3 = [f32; 3];
/// Linear colour; alpha scales coverage.
pub type Rgba = [f32; 4];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecalStyle {
    Telegraph,
    Disc,
    Crater,
    Shockwave,
    Dust,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BillboardPattern {
    Solid,
    SoftDisc,
    Ring,
    Spark,
    Silhouette,
    Sphere,
    Aura,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Decal {
    pub center: Vec3,
    pub radius: f32,
    pub color: Rgba,
    pub progress: f32,
    pub style: DecalStyle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ribbon {
    pub points: Vec<Vec3>,
    pub width: f32,
    pub color: Rgba,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Beam {
    pub start: Vec3,
    pub end: Vec3,
    pub width: f32,
    pub color: Rgba,
    pub intensity: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Billboard {
    pub position: Vec3,
    pub width: f32,
    pub height: f32,
    pub color: Rgba,
    pub pattern: BillboardPattern,
    pub upright: bool,
}

/// One callback's world-space primitives, bounded by the `mod_api` budgets.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Primitives {
    pub decals: Vec<Decal>,
    pub ribbons: Vec<Ribbon>,
    pub beams: Vec<Beam>,
    pub billboards: Vec<Billboard>,
}

impl Primitives {
    pub fn is_empty(&self) -> bool {
        self.decals.is_empty()
            && self.ribbons.is_empty()
            && self.beams.is_empty()
            && self.billboards.is_empty()
    }

    /// Appends `other` whole, or rejects it whole when any value or budget is invalid.
    pub fn append_checked(&mut self, other: Primitives) -> Result<(), String> {
        if self.decals.len() + other.decals.len() > MAX_RENDER_DECALS
            || self.ribbons.len() + other.ribbons.len() > MAX_RENDER_RIBBONS
            || self.beams.len() + other.beams.len() > MAX_RENDER_BEAMS
            || self.billboards.len() + other.billboards.len() > MAX_RENDER_BILLBOARDS
        {
            return Err("render primitive budget exceeded".into());
        }
        let valid = other.decals.iter().all(|decal| {
            point(decal.center)
                && extent(decal.radius)
                && color(decal.color)
                && decal.progress.is_finite()
        }) && other.ribbons.iter().all(|ribbon| {
            (2..=MAX_RIBBON_POINTS).contains(&ribbon.points.len())
                && ribbon.points.iter().all(|&p| point(p))
                && extent(ribbon.width)
                && color(ribbon.color)
        }) && other.beams.iter().all(|beam| {
            point(beam.start)
                && point(beam.end)
                && extent(beam.width)
                && color(beam.color)
                && (0.0..=16.0).contains(&beam.intensity)
        }) && other.billboards.iter().all(|billboard| {
            point(billboard.position)
                && extent(billboard.width)
                && extent(billboard.height)
                && color(billboard.color)
        });
        if !valid {
            return Err(
                "render primitives need finite, bounded positions, sizes and colours".into(),
            );
        }
        self.decals.extend(other.decals);
        self.ribbons.extend(other.ribbons);
        self.beams.extend(other.beams);
        self.billboards.extend(other.billboards);
        Ok(())
    }
}

fn point(p: Vec3) -> bool {
    p.iter()
        .all(|v| v.is_finite() && v.abs() <= MAX_PRIMITIVE_COORDINATE)
}

fn extent(value: f32) -> bool {
    value.is_finite() && value > 0.0 && value <= MAX_PRIMITIVE_EXTENT_BLOCKS
}

fn color(rgba: Rgba) -> bool {
    rgba.iter()
        .all(|c| c.is_finite() && (0.0..=16.0).contains(c))
        && rgba[3] <= 1.0
}

/// A validated post pass; `shader` is the complete host-composed WGSL module.
#[derive(Clone, Debug, PartialEq)]
pub struct Pass {
    pub name: String,
    pub order: i32,
    pub depth: bool,
    /// The guest's own source, compared to skip recompiling an unchanged registration.
    pub source: Arc<str>,
    pub shader: Arc<str>,
    /// Unique per accepted source, so renderers can key compiled pipelines by it.
    pub revision: u64,
    pub enabled: bool,
    pub params: [f32; MAX_PASS_PARAMS],
}

/// Everything a mod currently renders; passes are sorted in execution order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderOutput {
    pub passes: Vec<Pass>,
    pub primitives: Arc<Primitives>,
}

/// Combines several mods' output in load order within the single-mod budgets: earlier mods
/// keep a contested pass name and fill each primitive budget first. Merged primitives keep
/// their identity while every source is unchanged, so pass-only changes rebuild no geometry.
#[derive(Debug, Default)]
pub struct RenderMerge {
    sources: Vec<Arc<Primitives>>,
    merged: Arc<Primitives>,
}

impl RenderMerge {
    pub fn merge<'a>(
        &mut self,
        outputs: impl IntoIterator<Item = &'a RenderOutput>,
    ) -> RenderOutput {
        let outputs: Vec<&RenderOutput> = outputs.into_iter().collect();
        let mut passes: Vec<Pass> = Vec::new();
        for pass in outputs.iter().flat_map(|output| &output.passes) {
            if passes.len() < MAX_RENDER_PASSES && passes.iter().all(|kept| kept.name != pass.name)
            {
                passes.push(pass.clone());
            }
        }
        passes.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.name.cmp(&b.name)));
        let mut drawing = outputs
            .iter()
            .filter(|output| !output.primitives.is_empty());
        // A lone drawing mod's set already fits the budgets; sharing it skips a geometry rebuild.
        let primitives = match (drawing.next(), drawing.next()) {
            (None, _) => outputs
                .first()
                .map(|output| Arc::clone(&output.primitives))
                .unwrap_or_default(),
            (Some(only), None) => Arc::clone(&only.primitives),
            _ => self.merge_primitives(&outputs),
        };
        RenderOutput { passes, primitives }
    }

    fn merge_primitives(&mut self, outputs: &[&RenderOutput]) -> Arc<Primitives> {
        let same_sources = self.sources.len() == outputs.len()
            && self
                .sources
                .iter()
                .zip(outputs)
                .all(|(source, output)| Arc::ptr_eq(source, &output.primitives));
        if same_sources {
            return Arc::clone(&self.merged);
        }
        self.sources = outputs
            .iter()
            .map(|output| Arc::clone(&output.primitives))
            .collect();
        let mut merged = Primitives::default();
        for other in outputs.iter().map(|output| &output.primitives) {
            fill(&mut merged.decals, &other.decals, MAX_RENDER_DECALS);
            fill(&mut merged.ribbons, &other.ribbons, MAX_RENDER_RIBBONS);
            fill(&mut merged.beams, &other.beams, MAX_RENDER_BEAMS);
            fill(
                &mut merged.billboards,
                &other.billboards,
                MAX_RENDER_BILLBOARDS,
            );
        }
        if merged != *self.merged {
            self.merged = Arc::new(merged);
        }
        Arc::clone(&self.merged)
    }
}

fn fill<T: Clone>(into: &mut Vec<T>, from: &[T], cap: usize) {
    let room = cap.saturating_sub(into.len());
    into.extend(from.iter().take(room).cloned());
}

/// Valid pass names are short lowercase identifiers.
pub fn pass_name_valid(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= mod_api::MAX_PASS_NAME_BYTES
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

#[cfg(test)]
mod tests;
