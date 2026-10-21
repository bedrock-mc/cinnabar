//! Component adapter for retained drawing; every operation stages into the render transaction.

use super::{State, cinnabar::world_draw::drawing as wit};
use mod_api::drawing::{DrawingHandle, DrawingPermissions};
use mod_render::retained as domain;

impl State {
    /// Charges all calls, including denials, before admitting world resource mutations.
    fn drawing_allowed(&mut self) -> anyhow::Result<Result<DrawingPermissions, wit::DrawError>> {
        self.render.budget()?;
        let permissions = self.grants.drawing_permissions();
        Ok(if permissions.world_draw {
            Ok(permissions)
        } else {
            Err(convert_error(domain::DrawError {
                code: domain::ErrorCode::Denied,
                detail: "world drawing",
            }))
        })
    }
}

impl wit::Host for State {
    fn observe_entities(
        &mut self,
    ) -> anyhow::Result<Result<Vec<wit::ObservedEntity>, wit::DrawError>> {
        let permissions = match self.drawing_allowed()? {
            Ok(p) => p,
            Err(e) => return Ok(Err(e)),
        };
        if !permissions.entity_observe {
            return Ok(Err(convert_error(domain::DrawError {
                code: domain::ErrorCode::Denied,
                detail: "unfiltered entity observation",
            })));
        }
        Ok(self
            .render
            .retained
            .observe_entities()
            .map(|entities| {
                entities
                    .into_iter()
                    .map(|(entity, key)| wit::ObservedEntity {
                        entity: output_handle(entity),
                        runtime_id: key.runtime_id,
                    })
                    .collect()
            })
            .map_err(convert_error))
    }

    fn capabilities(&mut self) -> anyhow::Result<wit::Permissions> {
        self.render.budget()?;
        let p = self.grants.drawing_permissions();
        let mut result = wit::Permissions::empty();
        for (enabled, permission) in [
            (p.world_draw, wit::Permissions::WORLD_DRAW),
            (p.entity_observe, wit::Permissions::ENTITY_OBSERVE),
            (p.entity_highlight, wit::Permissions::ENTITY_HIGHLIGHT),
            (
                p.render_through_walls,
                wit::Permissions::RENDER_THROUGH_WALLS,
            ),
            (p.render_material, wit::Permissions::RENDER_MATERIAL),
            (p.render_post, wit::Permissions::RENDER_POST),
        ] {
            if enabled {
                result |= permission;
            }
        }
        Ok(result)
    }

    fn create_material(
        &mut self,
        source: String,
    ) -> anyhow::Result<Result<wit::Handle, wit::DrawError>> {
        let permissions = match self.drawing_allowed()? {
            Ok(p) => p,
            Err(e) => return Ok(Err(e)),
        };
        Ok(self
            .render
            .retained
            .create_material(&source, permissions)
            .map(output_handle)
            .map_err(convert_error))
    }

    fn create_geometry(
        &mut self,
        spec: wit::GeometrySpec,
    ) -> anyhow::Result<Result<wit::Handle, wit::DrawError>> {
        if let Err(error) = self.drawing_allowed()? {
            return Ok(Err(error));
        }
        let spec = match spec {
            wit::GeometrySpec::Triangles(mesh) => domain::GeometrySpec::Triangles {
                vertices: mesh.vertices.into_iter().map(vertex).collect(),
                indices: mesh.indices,
            },
            wit::GeometrySpec::Line(line) => domain::GeometrySpec::Polyline {
                vertices: line.vertices.into_iter().map(vertex).collect(),
                closed: line.closed,
            },
        };
        Ok(self
            .render
            .retained
            .create_geometry(spec)
            .map(output_handle)
            .map_err(convert_error))
    }

    fn create(
        &mut self,
        spec: wit::Instance,
    ) -> anyhow::Result<Result<wit::Handle, wit::DrawError>> {
        let permissions = match self.drawing_allowed()? {
            Ok(p) => p,
            Err(e) => return Ok(Err(e)),
        };
        if spec.params.len() > mod_api::materials::PARAMS {
            return Ok(Err(parameter_error()));
        }
        Ok(self
            .render
            .retained
            .create(instance(spec), permissions)
            .map(output_handle)
            .map_err(convert_error))
    }

    fn update(
        &mut self,
        id: wit::Handle,
        spec: wit::Instance,
    ) -> anyhow::Result<Result<(), wit::DrawError>> {
        let permissions = match self.drawing_allowed()? {
            Ok(p) => p,
            Err(e) => return Ok(Err(e)),
        };
        if spec.params.len() > mod_api::materials::PARAMS {
            return Ok(Err(parameter_error()));
        }
        Ok(self
            .render
            .retained
            .update(handle(id), instance(spec), permissions)
            .map_err(convert_error))
    }

    fn replace_frame_group(
        &mut self,
        name: String,
        draws: Vec<wit::Instance>,
    ) -> anyhow::Result<Result<(), wit::DrawError>> {
        let permissions = match self.drawing_allowed()? {
            Ok(p) => p,
            Err(e) => return Ok(Err(e)),
        };
        if draws
            .iter()
            .any(|draw| draw.params.len() > mod_api::materials::PARAMS)
        {
            return Ok(Err(parameter_error()));
        }
        Ok(self
            .render
            .retained
            .replace_group(name, draws.into_iter().map(instance).collect(), permissions)
            .map_err(convert_error))
    }

    fn remove(&mut self, id: wit::Handle) -> anyhow::Result<Result<(), wit::DrawError>> {
        if let Err(error) = self.drawing_allowed()? {
            return Ok(Err(error));
        }
        Ok(self
            .render
            .retained
            .remove(handle(id))
            .map_err(convert_error))
    }
}

/// Reports an oversized fixed material block without truncating caller data.
fn parameter_error() -> wit::DrawError {
    convert_error(domain::DrawError {
        code: domain::ErrorCode::Invalid,
        detail: "material parameters",
    })
}

/// Converts a copied ABI token without trusting its owner or generation.
fn handle(handle: wit::Handle) -> DrawingHandle {
    DrawingHandle {
        owner: handle.owner,
        generation: handle.generation,
        slot: handle.slot,
    }
}
/// Publishes an admitted resource token without exposing any native resource.
fn output_handle(handle: DrawingHandle) -> wit::Handle {
    wit::Handle {
        owner: handle.owner,
        generation: handle.generation,
        slot: handle.slot,
    }
}
/// Keeps the vertex layout independent of graphics API and component code generation.
fn vertex(v: wit::Vertex) -> domain::Vertex {
    domain::Vertex {
        position: v.position.into(),
        color: v.color.into(),
        uv: v.uv.into(),
        data: v.data.into(),
    }
}
/// Converts all closed state enums exhaustively; validation stays in the domain owner.
fn instance(spec: wit::Instance) -> domain::Instance {
    domain::Instance {
        geometry: handle(spec.geometry),
        material: spec.material.map(handle),
        params: std::array::from_fn(|index| spec.params.get(index).copied().unwrap_or(0.)),
        anchor: match spec.anchor {
            wit::Anchor::World(anchor) => domain::Anchor::World {
                dimension: anchor.dimension,
                origin: anchor.origin.into(),
            },
            wit::Anchor::Block(anchor) => domain::Anchor::Block {
                dimension: anchor.dimension,
                position: anchor.position.into(),
            },
            wit::Anchor::Camera => domain::Anchor::Camera,
            wit::Anchor::Entity(anchor) => domain::Anchor::Entity(domain::EntityAnchor {
                entity: handle(anchor.entity),
                body_rotation: anchor.body_rotation,
            }),
        },
        transform: domain::Transform {
            translation: spec.transform.translation.into(),
            rotation: spec.transform.rotation.into(),
            scale: spec.transform.scale.into(),
        },
        tint: spec.tint.into(),
        depth: match spec.depth {
            wit::Depth::Test => domain::Depth::Test,
            wit::Depth::Always => domain::Depth::Always,
        },
        blend: match spec.blend {
            wit::Blend::Premultiplied => domain::Blend::Premultiplied,
            wit::Blend::Additive => domain::Blend::Additive,
        },
        width: spec.width,
        width_mode: match spec.width_mode {
            wit::WidthMode::World => domain::WidthMode::World,
            wit::WidthMode::Pixels => domain::WidthMode::Pixels,
        },
        join: match spec.join {
            wit::Join::Bevel => domain::Join::Bevel,
            wit::Join::Miter => domain::Join::Miter,
            wit::Join::Round => domain::Join::Round,
        },
        cap: match spec.cap {
            wit::Cap::Butt => domain::Cap::Butt,
            wit::Cap::Round => domain::Cap::Round,
        },
    }
}
/// Preserves typed domain failures with a bounded human-readable category.
fn convert_error(error: domain::DrawError) -> wit::DrawError {
    wit::DrawError {
        detail: error.detail.into(),
        code: match error.code {
            domain::ErrorCode::Denied => wit::ErrorCode::Denied,
            domain::ErrorCode::Invalid => wit::ErrorCode::Invalid,
            domain::ErrorCode::BudgetExceeded => wit::ErrorCode::BudgetExceeded,
            domain::ErrorCode::StaleHandle => wit::ErrorCode::StaleHandle,
            domain::ErrorCode::Unsupported => wit::ErrorCode::Unsupported,
            domain::ErrorCode::InUse => wit::ErrorCode::InUse,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wit::Host as _;

    /// Creates an import-facing triangle without granting any host graphics resources.
    fn triangle() -> wit::GeometrySpec {
        wit::GeometrySpec::Triangles(wit::Mesh {
            vertices: [(0., 0., 0.), (1., 0., 0.), (0., 1., 0.)]
                .map(|position| wit::Vertex {
                    position,
                    color: (1., 1., 1., 1.),
                    uv: (0., 0.),
                    data: (0., 0., 0., 0.),
                })
                .to_vec(),
            indices: vec![0, 1, 2],
        })
    }

    #[test]
    fn resource_imports_are_denied_by_default_and_publish_only_at_commit() {
        let mut state = State::new(
            crate::ModGrants::default(),
            String::new(),
            Default::default(),
        );
        assert_eq!(
            state.create_geometry(triangle()).unwrap().unwrap_err().code,
            wit::ErrorCode::Denied
        );
        state.grants.world_draw = true;
        let geometry = state.create_geometry(triangle()).unwrap().unwrap();
        assert!(state.render.output().retained.is_empty());
        state.render.commit_retained();
        assert_eq!(state.render.output().retained[0].geometry.len(), 1);
        state.render.begin_frame();
        state.remove(geometry).unwrap().unwrap();
        state.render.revoke();
        assert!(state.render.output().retained.is_empty());
        assert_eq!(
            state.remove(geometry).unwrap().unwrap_err().code,
            wit::ErrorCode::StaleHandle
        );
    }

    #[test]
    fn geometry_uploads_share_the_callback_import_budget() {
        let mut state = State::new(
            crate::ModGrants {
                world_draw: true,
                ..Default::default()
            },
            String::new(),
            Default::default(),
        );
        state.render.begin_frame();
        for _ in 0..mod_api::MAX_RENDER_CALLS {
            state.create_geometry(triangle()).unwrap().unwrap();
        }
        state.render.commit_retained();
        assert_eq!(
            state.render.output().retained[0].geometry.len(),
            mod_api::MAX_RENDER_CALLS
        );

        let error = state.create_geometry(triangle()).unwrap_err();
        assert!(format!("{error:#}").contains("render import budget exhausted"));

        state.render.revoke();
        state.render.begin_frame();
        state.capabilities().unwrap();
        for _ in 1..mod_api::MAX_RENDER_CALLS {
            state.create_geometry(triangle()).unwrap().unwrap();
        }
        assert!(state.create_geometry(triangle()).is_err());
    }

    #[test]
    fn capability_query_covers_the_generated_contract() {
        let fields = DrawingPermissions::NAMES
            .iter()
            .map(|name| ((*name).to_owned(), serde_json::Value::Bool(true)))
            .collect();
        let grants = serde_json::from_value(serde_json::Value::Object(fields)).unwrap();
        let mut state = State::new(grants, String::new(), Default::default());
        let permissions = state.capabilities().unwrap();
        assert_eq!(permissions, wit::Permissions::all());
        assert_eq!(
            permissions
                .as_array()
                .into_iter()
                .map(u32::count_ones)
                .sum::<u32>() as usize,
            DrawingPermissions::NAMES.len()
        );
    }
}

#[cfg(test)]
mod material_tests {
    use super::*;
    use wit::Host as _;
    #[test]
    fn material_import_requires_grant_and_publishes_only_on_commit() {
        let mut state = State::new(
            crate::ModGrants::default(),
            String::new(),
            Default::default(),
        );
        state.grants.world_draw = true;
        assert_eq!(
            state
                .create_material(mod_render::material::BUILTIN.into())
                .unwrap()
                .unwrap_err()
                .code,
            wit::ErrorCode::Denied
        );
        state.grants.render_material = true;
        let handle = state
            .create_material(mod_render::material::BUILTIN.into())
            .unwrap()
            .unwrap();
        assert!(state.render.output().retained.is_empty());
        state.render.commit();
        assert_eq!(state.render.output().retained[0].materials.len(), 1);
        state.render.revoke();
        assert_eq!(
            state.remove(handle).unwrap().unwrap_err().code,
            wit::ErrorCode::StaleHandle
        );
    }
}

impl crate::ModHost {
    /// Exposes only host-owned incarnation keys for liveness checks, never guest pose data.
    pub fn drawing_entity_keys(&self) -> impl Iterator<Item = domain::EntityKey> + '_ {
        self.instance.drawing_entity_keys()
    }

    /// Supplies this callback's bounded candidates and authoritative removals before execution.
    pub fn update_drawing_entities(
        &mut self,
        candidates: Vec<domain::EntityKey>,
        despawned: &[domain::EntityKey],
    ) -> Result<(), domain::DrawError> {
        self.instance.update_drawing_entities(candidates, despawned)
    }
}

#[cfg(test)]
mod entity_tests {
    use super::*;
    use wit::Host as _;

    #[test]
    fn actor_references_require_user_observation_consent() {
        let mut state = State::new(
            crate::ModGrants {
                world_draw: true,
                ..Default::default()
            },
            String::new(),
            Default::default(),
        );
        state.drawing_policy = Some(mod_api::drawing::DrawingSession {
            connection: 1,
            ..Default::default()
        });
        assert_eq!(
            state.observe_entities().unwrap().unwrap_err().code,
            wit::ErrorCode::Denied
        );
        state.grants.entity_observe = true;
        state
            .render
            .retained
            .reconcile_entities(
                vec![domain::EntityKey {
                    session: 1,
                    dimension: 0,
                    runtime_id: 7,
                    spawn_revision: 1,
                }],
                &[],
            )
            .unwrap();
        let observed = state.observe_entities().unwrap().unwrap();
        assert_eq!(observed.len(), 1);
        assert_eq!(observed[0].runtime_id, 7);
        assert!(state.render.output().retained.is_empty());
    }
}
