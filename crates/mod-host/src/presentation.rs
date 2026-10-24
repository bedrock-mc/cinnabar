//! Typed helper presentation output, admitted again at the application boundary.

use anyhow::{Result, ensure};
use mod_api::drawing::{DrawingHandle, DrawingPermissions};
use mod_render::{RenderOutput, retained::transfer::Snapshot};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

mod transfer;
pub use transfer::Chunk;
pub(crate) use transfer::Pending;

/// Portable post effect inputs. The receiver composes its own sandboxed shader.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pass {
    pub name: String,
    pub order: i32,
    pub source: String,
    pub enabled: bool,
    pub params: [f32; mod_api::MAX_PASS_PARAMS],
    pub masks: Vec<DrawingHandle>,
}

/// Committed drawing and local environment from one successful callback.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Output {
    pub drawing: Snapshot,
    pub primitives: mod_render::Primitives,
    pub passes: Vec<Pass>,
    pub time: Option<u32>,
    pub weather: Option<[f32; 2]>,
    pub masks: Vec<mod_render::masks::Mask>,
}

/// Trusted native frame identities, bounded before helper dispatch.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub epoch: u64,
    pub connection: u64,
    pub dimension: i32,
    pub candidates: Vec<mod_render::retained::EntityKey>,
    /// Current candidates, held references and candidates of one in-flight callback.
    pub live: Vec<mod_render::retained::EntityKey>,
    pub targets: Vec<(mod_api::drawing::MaskEntity, u64)>,
}

impl Context {
    /// Refuses callbacks whose authority or actor identities do not match their world epoch.
    pub fn check(&self, epoch: u64) -> Result<()> {
        ensure!(
            self.connection != 0 && self.epoch == epoch,
            "unbound or stale drawing context"
        );
        ensure!(
            self.candidates.len() <= mod_api::MAX_DRAW_ENTITIES
                && self.live.len() <= mod_api::MAX_DRAW_ENTITIES * 3
                && self.targets.len() <= mod_api::drawing::MAX_MASK_ENTITIES,
            "drawing context budget exceeded"
        );
        ensure!(
            self.candidates.iter().all(|key| self.live.contains(key))
                && self
                    .live
                    .iter()
                    .all(|key| key.session == self.connection && key.dimension == self.dimension)
                && self
                    .targets
                    .iter()
                    .all(|(key, _)| key.connection == self.connection
                        && key.dimension == self.dimension),
            "foreign drawing context actor"
        );
        Ok(())
    }

    /// Identifies the authority shared by callbacks, independently of their current actor lists.
    pub fn fence(&self) -> (u64, u64, i32) {
        (self.connection, self.epoch, self.dimension)
    }
}

/// Maps helper-owned tokens into the application's common renderer namespace.
#[derive(Default)]
pub struct Receiver {
    owners: BTreeMap<u64, u64>,
    assembly: transfer::Assembly,
    output: RenderOutput,
    revision: u64,
    ledger: Option<mod_render::GpuLedger>,
    pub time: Option<u32>,
    pub weather: Option<[f32; 2]>,
    /// Stale held references, snapshot references and mask members discarded at completion.
    pub stale_references: u64,
}

impl Receiver {
    /// Returns the allocation ledger shared by drawing and media for this bundle lifetime.
    pub fn ledger(&mut self, limit: u64) -> &mod_render::GpuLedger {
        self.ledger
            .get_or_insert_with(|| mod_render::GpuLedger::new(limit))
    }

    /// Clears old-world presentation while outstanding storage keeps its bundle ledger.
    pub fn clear(&mut self) {
        self.owners.clear();
        self.assembly = Default::default();
        self.output = Default::default();
        self.time = None;
        self.weather = None;
        self.revision = mod_render::next_owner();
    }

    /// Returns only the latest fully validated output and its local identity.
    pub fn output(&self) -> (&RenderOutput, u64) {
        (&self.output, self.revision)
    }

    /// Rechecks the active drawing and legacy scene after either contribution changes.
    pub fn check_scene_budget(
        &self,
        contributions: &server_experience::runtime::Contributions,
    ) -> Result<()> {
        check_scene_budget(&self.output, contributions)
    }

    /// Assembles a committed scene within its reservation and publishes it atomically.
    /// `valid` queries the current native actor store, independently of drawing visibility.
    pub fn receive(
        &mut self,
        chunk: Chunk,
        permissions: DrawingPermissions,
        context: &Context,
        memory_bytes: u64,
        gpu_bytes: u64,
        valid: impl Fn(mod_render::retained::EntityKey) -> bool,
    ) -> Result<()> {
        if let Some(output) = self.assembly.receive(chunk, context, memory_bytes)? {
            self.apply(output, permissions, context, gpu_bytes, valid)?;
        }
        Ok(())
    }

    /// Rechecks permissions and reservations before publishing any helper output.
    /// `valid` must check the actor incarnation against the current native actor store.
    pub fn apply(
        &mut self,
        input: Output,
        permissions: DrawingPermissions,
        context: &Context,
        gpu_bytes: u64,
        valid: impl Fn(mod_render::retained::EntityKey) -> bool,
    ) -> Result<()> {
        ensure!(
            permissions.world_draw
                || (input.primitives.is_empty() && input.time.is_none() && input.weather.is_none()),
            "world drawing denied"
        );
        ensure!(
            input
                .time
                .is_none_or(|ticks| ticks < mod_api::BEDROCK_DAY_TICKS),
            "invalid visual time"
        );
        ensure!(
            input.weather.is_none_or(|channels| channels
                .into_iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(&v))),
            "invalid weather"
        );
        let mut owners = self.owners.clone();
        let mut output = RenderOutput::default();
        let (resources, mut stale_references) = input
            .drawing
            .admit(
                self.output.retained.first().map(Arc::as_ref),
                permissions,
                context.connection,
                context.dimension,
                &valid,
                |id| {
                    let owner = *owners
                        .entry(id.owner)
                        .or_insert_with(mod_render::next_owner);
                    DrawingHandle { owner, ..id }
                },
            )
            .map_err(|error| anyhow::anyhow!("drawing: {}", error.detail))?;
        let mut primitives = mod_render::Primitives::default();
        primitives
            .append_checked(input.primitives)
            .map_err(anyhow::Error::msg)?;
        output.primitives = if primitives == *self.output.primitives {
            Arc::clone(&self.output.primitives)
        } else {
            Arc::new(primitives)
        };
        ensure!(
            input.passes.len() <= mod_api::MAX_RENDER_PASSES,
            "pass budget exceeded"
        );
        ensure!(
            input.masks.len() <= mod_api::drawing::MAX_MASKS,
            "mask budget exceeded"
        );
        let mut masks = Vec::new();
        for mut mask in input.masks {
            mask.handle.owner = *owners
                .entry(mask.handle.owner)
                .or_insert_with(mod_render::next_owner);
            ensure!(
                permissions.entity_highlight
                    && (mask.depth == mod_api::drawing::MaskDepth::Test
                        || permissions.render_through_walls),
                "mask authority denied"
            );
            ensure!(
                mask.entities.len() <= mod_api::drawing::MAX_MASK_ENTITIES
                    && mask.style.is_none_or(|style| style.valid())
                    && mask.position.is_none(),
                "invalid mask"
            );
            let mut vertices = 0u64;
            for entity in &mask.entities {
                ensure!(
                    entity.connection == context.connection
                        && entity.dimension == context.dimension,
                    "foreign mask actor"
                );
                if let Some((_, count)) = context.targets.iter().find(|(key, _)| key == entity) {
                    vertices = vertices.saturating_add(*count);
                }
            }
            ensure!(
                vertices <= mod_api::drawing::MAX_MASK_VERTICES,
                "mask vertex budget exceeded"
            );
            let members = mask.entities.len();
            mask.entities.retain(|entity| {
                valid(mod_render::retained::EntityKey {
                    session: entity.connection,
                    dimension: entity.dimension,
                    runtime_id: entity.runtime_id,
                    spawn_revision: entity.spawn_revision,
                })
            });
            stale_references += members - mask.entities.len();
            mask.session = mod_api::drawing::DrawingSession {
                connection: context.connection,
                dimension: context.dimension,
                local: false,
                permissions,
            };
            ensure!(
                !masks
                    .iter()
                    .any(|old: &mod_render::masks::Mask| old.handle == mask.handle),
                "duplicate mask"
            );
            masks.push(mask);
        }
        output.masks = masks.into();
        for pass in input.passes {
            ensure!(
                permissions.render_post
                    && mod_render::pass_name_valid(&pass.name)
                    && pass.params.iter().all(|v| v.is_finite()),
                "post effect denied or invalid"
            );
            ensure!(
                !output.passes.iter().any(|old| old.name == pass.name),
                "duplicate post effect"
            );
            ensure!(
                pass.masks.len() <= mod_api::drawing::MAX_MASKS,
                "mask binding budget exceeded"
            );
            let bindings: Vec<_> = pass
                .masks
                .into_iter()
                .map(|id| {
                    let owner = *owners
                        .entry(id.owner)
                        .or_insert_with(mod_render::next_owner);
                    DrawingHandle { owner, ..id }
                })
                .collect();
            ensure!(
                bindings
                    .iter()
                    .all(|id| output.masks.iter().any(|mask| mask.handle == *id)),
                "unowned mask binding"
            );
            let previous = self.output.passes.iter().find(|old| {
                old.name == pass.name && old.source.as_ref() == pass.source && old.masks == bindings
            });
            let (shader, revision) = match previous {
                Some(old) => (Arc::clone(&old.shader), old.revision),
                None => (
                    mod_render::shader::compose_masked(&pass.source, false, bindings.len())
                        .map_err(anyhow::Error::msg)?
                        .into(),
                    mod_render::next_owner(),
                ),
            };
            output.passes.push(mod_render::Pass {
                name: pass.name,
                order: pass.order,
                source: pass.source.into(),
                shader,
                revision,
                enabled: pass.enabled,
                params: pass.params,
                depth: false,
                masks: bindings,
            });
        }
        output
            .passes
            .sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.name.cmp(&b.name)));
        if !resources.geometry.is_empty()
            || !resources.materials.is_empty()
            || !resources.instances.is_empty()
            || !resources.entities.is_empty()
        {
            let retained = self
                .output
                .retained
                .first()
                .filter(|old| ***old == resources)
                .cloned()
                .unwrap_or_else(|| Arc::new(resources));
            output.retained.push(retained);
        }
        check_scene_budget(&output, &Default::default())?;
        owners.retain(|_, local| {
            output.retained.iter().any(|resources| {
                resources
                    .geometry
                    .keys()
                    .chain(resources.materials.keys())
                    .chain(resources.instances.keys())
                    .chain(resources.entities.keys())
                    .any(|id| id.owner == *local)
            }) || output.masks.iter().any(|mask| mask.handle.owner == *local)
        });
        output.gpu_reservations.push(mod_render::GpuReservation {
            owners: owners.values().copied().collect(),
            ledger: self.ledger(gpu_bytes).clone(),
            primitives: mod_render::PrimitiveRanges::all(&output.primitives),
            passes: output.passes.iter().map(|pass| pass.revision).collect(),
        });
        self.owners = owners;
        self.output = output;
        self.revision = mod_render::next_owner();
        self.time = input.time;
        self.weather = input.weather;
        self.stale_references = self
            .stale_references
            .saturating_add(stale_references as u64);
        Ok(())
    }
}

/// Charges every active retained placement and immediate primitive alongside legacy objects.
fn check_scene_budget(
    output: &RenderOutput,
    contributions: &server_experience::runtime::Contributions,
) -> Result<()> {
    let draws = output.primitives.draw_count()
        + output
            .retained
            .iter()
            .map(|resources| resources.instances.len())
            .sum::<usize>();
    let triangles = mod_render::geometry::vertex_count(&output.primitives) / 3
        + output
            .retained
            .iter()
            .map(|resources| {
                resources
                    .instances
                    .values()
                    .filter_map(|instance| resources.geometry.get(&instance.geometry))
                    .map(|geometry| geometry.triangles())
                    .sum::<usize>()
            })
            .sum::<usize>();
    contributions.check_scene_budget(draws, triangles as u64)
}

#[cfg(test)]
mod tests;
