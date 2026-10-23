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
    pub connection: u64,
    pub dimension: i32,
    pub candidates: Vec<mod_render::retained::EntityKey>,
    pub live: Vec<mod_render::retained::EntityKey>,
    pub targets: Vec<(mod_api::drawing::MaskEntity, u64)>,
}

/// Maps helper-owned tokens into the application's common renderer namespace.
#[derive(Default)]
pub struct Receiver {
    owners: BTreeMap<u64, u64>,
    assembly: transfer::Assembly,
    output: RenderOutput,
    revision: u64,
    retired: Vec<Arc<mod_render::retained::Geometry>>,
    pub time: Option<u32>,
    pub weather: Option<[f32; 2]>,
}

impl Receiver {
    /// Releases completed geometry retirements and republishes their GPU reservation.
    /// Call every frame, including frames without a new guest snapshot.
    pub fn reap_retired(&mut self) {
        let mut released_bytes = 0u64;
        self.retired.retain(|geometry| {
            if Arc::strong_count(geometry) > 1 {
                true
            } else {
                released_bytes = released_bytes.saturating_add(geometry.gpu_bytes());
                false
            }
        });
        if released_bytes != 0 {
            for reservation in &mut self.output.gpu_reservations {
                reservation.geometry_bytes =
                    reservation.geometry_bytes.saturating_sub(released_bytes);
            }
            self.revision = mod_render::next_owner();
        }
    }

    /// Returns only the latest fully validated output and its local identity.
    pub fn output(&self) -> (&RenderOutput, u64) {
        (&self.output, self.revision)
    }

    /// Assembles a committed scene within its reservation and publishes it atomically.
    pub fn receive(
        &mut self,
        chunk: Chunk,
        permissions: DrawingPermissions,
        context: &Context,
        memory_bytes: u64,
        gpu_bytes: u64,
    ) -> Result<()> {
        if let Some((output, context)) = self.assembly.receive(chunk, context, memory_bytes)? {
            self.apply(output, permissions, &context, gpu_bytes)?;
        }
        Ok(())
    }

    /// Rechecks permissions and reservations before publishing any helper output.
    pub fn apply(
        &mut self,
        input: Output,
        permissions: DrawingPermissions,
        context: &Context,
        gpu_bytes: u64,
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
        let resources = input
            .drawing
            .admit(
                self.output.retained.first().map(Arc::as_ref),
                permissions,
                context.connection,
                context.dimension,
                &context.live,
                |id| {
                    let owner = *owners
                        .entry(id.owner)
                        .or_insert_with(mod_render::next_owner);
                    DrawingHandle { owner, ..id }
                },
            )
            .map_err(|error| anyhow::anyhow!("drawing: {}", error.detail))?;
        let mut retired: Vec<_> = self
            .retired
            .iter()
            .filter(|geometry| Arc::strong_count(geometry) > 1)
            .cloned()
            .collect();
        for old in self
            .output
            .retained
            .iter()
            .flat_map(|resources| resources.geometry.values())
        {
            if !resources
                .geometry
                .values()
                .any(|live| Arc::ptr_eq(live, old))
                && !retired.iter().any(|kept| Arc::ptr_eq(kept, old))
            {
                retired.push(Arc::clone(old));
            }
        }
        Snapshot::check_budget(&resources, &retired)
            .map_err(|error| anyhow::anyhow!("drawing: {}", error.detail))?;
        let retiring_bytes: u64 = retired
            .iter()
            .filter(|old| {
                !resources
                    .geometry
                    .values()
                    .any(|live| Arc::ptr_eq(live, old))
            })
            .map(|geometry| geometry.gpu_bytes())
            .sum();
        ensure!(
            Snapshot::gpu_bytes(&resources).saturating_add(retiring_bytes) <= gpu_bytes,
            "drawing GPU budget exceeded"
        );
        ensure!(
            resources.instances.len() <= server_experience::policy::MAX_DRAWS as usize,
            "bundle draw budget exceeded"
        );
        let mut primitives = mod_render::Primitives::default();
        primitives
            .append_checked(input.primitives)
            .map_err(anyhow::Error::msg)?;
        let vertices = mod_render::geometry::vertex_count(&primitives);
        ensure!(
            resources
                .instances
                .values()
                .filter_map(|instance| resources.geometry.get(&instance.geometry))
                .map(|geometry| geometry.triangles())
                .sum::<usize>()
                + vertices / 3
                <= server_experience::policy::MAX_TRIANGLES as usize,
            "bundle triangle budget exceeded"
        );
        let geometry_bytes = Snapshot::gpu_bytes(&resources)
            .saturating_add(retiring_bytes)
            .saturating_add(
                (vertices * std::mem::size_of::<mod_render::geometry::ModVertex>()) as u64,
            );
        ensure!(geometry_bytes <= gpu_bytes, "drawing GPU budget exceeded");
        let draws = resources.instances.len()
            + primitives.decals.len()
            + primitives.ribbons.len()
            + primitives.beams.len()
            + primitives.billboards.len();
        ensure!(
            draws <= server_experience::policy::MAX_DRAWS as usize,
            "bundle draw budget exceeded"
        );
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
            mask.entities
                .retain(|entity| context.targets.iter().any(|(key, _)| key == entity));
            ensure!(
                vertices <= mod_api::drawing::MAX_MASK_VERTICES,
                "mask vertex budget exceeded"
            );
            mask.handle.owner = *owners
                .entry(mask.handle.owner)
                .or_insert_with(mod_render::next_owner);
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
            geometry_bytes,
            limit_bytes: gpu_bytes,
        });
        self.retired = retired;
        self.owners = owners;
        self.output = output;
        self.revision = mod_render::next_owner();
        self.time = input.time;
        self.weather = input.weather;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
