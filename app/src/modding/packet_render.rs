//! Admits packet-extension world scenes through the retained drawing owner.
use bevy::prelude::*;
use mod_api::{
    drawing::{DrawingHandle, DrawingSession},
    native_render::Scene,
};
use mod_render::{
    RenderOutput,
    retained::{self as draw, Anchor, GeometrySpec, Instance, Transform, Vertex},
};
use std::sync::Arc;

#[derive(Resource)]
pub(super) struct Drawing {
    store: draw::Store,
    session: DrawingSession,
    source: Option<Arc<Scene>>,
    geometry: Vec<DrawingHandle>,
    instances: Vec<DrawingHandle>,
    output: RenderOutput,
    generation: u64,
    highlights: super::packet_highlights::Highlights,
}
impl Default for Drawing {
    /// Allocates one host-owned drawing namespace for geometry and actor masks.
    fn default() -> Self {
        let store = draw::Store::default();
        let highlights = super::packet_highlights::Highlights::new(store.owner());
        Self {
            store,
            highlights,
            session: Default::default(),
            source: None,
            geometry: Vec::new(),
            instances: Vec::new(),
            output: Default::default(),
            generation: 0,
        }
    }
}
impl Drawing {
    /// Validates resources once per changed desired scene or trusted connection fence.
    pub(super) fn update(&mut self, scene: Option<Arc<Scene>>, session: DrawingSession) {
        let scene = scene.filter(|scene| {
            scene.session == session.connection
                && scene.dimension == session.dimension
                && scene.is_valid()
        });
        if self.session == session && self.source == scene && !self.highlights.dirty {
            return;
        }
        let changed_scope = self.session != session;
        self.session = session;
        self.source = scene.clone();
        self.generation = self.generation.wrapping_add(1);
        if changed_scope {
            self.store
                .bind_session(session.connection, session.dimension);
            self.geometry.clear();
            self.instances.clear();
        }
        self.store.begin();
        let result = self.stage(scene.as_deref());
        if result.is_err() {
            // A partially admitted transaction must never retain yesterday's output.
            self.store.rollback();
            self.store.revoke();
            self.store
                .bind_session(session.connection, session.dimension);
            self.geometry.clear();
            self.instances.clear();
        } else {
            self.store.commit();
        }
        self.output.masks = self.highlights.update(scene.as_deref(), session);
        self.output.retained = if self.store.snapshot().instances.is_empty() {
            Vec::new()
        } else {
            vec![Arc::clone(self.store.snapshot())]
        };
    }

    /// Avoids actor observation work when the packet scene selects no meshes.
    pub(super) fn needs_targets(&self) -> bool {
        self.source
            .as_ref()
            .is_some_and(|scene| !scene.entities.is_empty())
    }
    /// Reconciles actor identity and vertex admission through the shared mask owner.
    pub(super) fn targets(&mut self, targets: Vec<(mod_api::drawing::MaskEntity, u64)>) {
        self.highlights.targets(targets);
    }
    /// Shares full-block markers with the existing loaded-block overlay renderer.
    pub(super) fn publish_blocks(&self, scene: &mut render_scene::ModRenderScene) {
        scene.set_drawn_block_highlights(
            self.source
                .as_ref()
                .filter(|_| {
                    self.session.connection != 0
                        && self.session.permissions.world_draw
                        && self.session.permissions.render_through_walls
                })
                .map(|source| source.blocks.as_slice())
                .unwrap_or_default(),
        );
    }

    /// Keeps geometry immutable and reuses instance handles across ordinary movement updates.
    fn stage(&mut self, scene: Option<&Scene>) -> Result<(), draw::DrawError> {
        let Some(scene) =
            scene.filter(|_| self.session.connection != 0 && self.session.permissions.world_draw)
        else {
            for handle in self.instances.drain(..) {
                self.store.remove(handle)?;
            }
            return Ok(());
        };
        if self.geometry.is_empty() {
            let specs = mod_api::drawing::shapes::box_wireframe([[-0.5; 3], [0.5; 3]], [1.0; 4])
                .expect("unit wire box is bounded");
            for spec in specs {
                self.geometry
                    .push(self.store.create_geometry(convert(spec))?);
            }
            self.geometry.push(self.store.create_geometry(solid_box())?);
            self.geometry
                .push(self.store.create_geometry(GeometrySpec::Polyline {
                    vertices: vec![vertex([-0.5, 0.0, 0.0]), vertex([0.5, 0.0, 0.0])],
                    closed: false,
                })?);
        }
        let mut desired = Vec::new();
        for volume in &scene.boxes {
            let transform = Transform {
                translation: volume.centre,
                rotation: Quat::from_rotation_y(volume.yaw).to_array(),
                scale: volume.size,
            };
            for &geometry in if volume.filled {
                &self.geometry[12..13]
            } else {
                &self.geometry[..12]
            } {
                desired.push(instance(
                    geometry,
                    scene.dimension,
                    transform,
                    volume.colour,
                ));
            }
        }
        for line in &scene.lines {
            let start = Vec3::from_array(line.start);
            let delta = Vec3::from_array(line.end) - start;
            let Some(direction) = delta.try_normalize() else {
                continue;
            };
            let length = delta.length();
            let segments = (length / (2.0 * mod_api::MAX_PRIMITIVE_EXTENT_BLOCKS))
                .ceil()
                .max(1.0) as usize;
            if segments > mod_api::MAX_DRAW_INSTANCES.saturating_sub(desired.len()) {
                continue;
            }
            for index in 0..segments {
                let centre = start + delta * ((index as f32 + 0.5) / segments as f32);
                desired.push(instance(
                    self.geometry[13],
                    scene.dimension,
                    Transform {
                        translation: centre.to_array(),
                        rotation: Quat::from_rotation_arc(Vec3::X, direction).to_array(),
                        scale: [length / segments as f32, 1.0, 1.0],
                    },
                    line.colour,
                ));
            }
        }
        for (index, desired) in desired.iter().enumerate() {
            if let Some(&handle) = self.instances.get(index) {
                self.store
                    .update(handle, desired.clone(), self.session.permissions)?;
            } else {
                self.instances.push(
                    self.store
                        .create(desired.clone(), self.session.permissions)?,
                );
            }
        }
        for handle in self.instances.drain(desired.len()..) {
            self.store.remove(handle)?;
        }
        Ok(())
    }
    /// Supplies the shared renderer's generation cache and immutable resources.
    pub(super) fn output(&self) -> (&RenderOutput, u64) {
        (&self.output, self.generation)
    }
}

/// Converts the generated shape-builder topology into the retained owner's inputs.
fn convert(spec: mod_api::drawing::api::GeometrySpec) -> GeometrySpec {
    use mod_api::drawing::api::GeometrySpec as Wire;
    let v = |v: mod_api::drawing::api::Vertex| Vertex {
        position: v.position.into(),
        color: v.color.into(),
        uv: v.uv.into(),
        data: v.data.into(),
    };
    match spec {
        Wire::Triangles(mesh) => GeometrySpec::Triangles {
            vertices: mesh.vertices.into_iter().map(v).collect(),
            indices: mesh.indices,
        },
        Wire::Line(line) => GeometrySpec::Polyline {
            vertices: line.vertices.into_iter().map(v).collect(),
            closed: line.closed,
        },
    }
}
/// Supplies unlit unit geometry; colour and dimensions belong to instances.
fn vertex(position: [f32; 3]) -> Vertex {
    Vertex {
        position,
        color: [1.0; 4],
        uv: [0.0; 2],
        data: [0.0; 4],
    }
}
/// Builds the six faces of the reusable centred unit volume.
fn solid_box() -> GeometrySpec {
    let vertices = (0..8)
        .map(|i| {
            vertex(std::array::from_fn(|axis| {
                if i & (1 << axis) == 0 { -0.5 } else { 0.5 }
            }))
        })
        .collect();
    GeometrySpec::Triangles {
        vertices,
        indices: vec![
            0, 2, 3, 0, 3, 1, 4, 5, 7, 4, 7, 6, 0, 1, 5, 0, 5, 4, 2, 6, 7, 2, 7, 3, 0, 4, 6, 0, 6,
            2, 1, 3, 7, 1, 7, 5,
        ],
    }
}
/// Depth authority remains the retained store's admission decision.
fn instance(
    geometry: DrawingHandle,
    dimension: i32,
    mut transform: Transform,
    tint: [f32; 4],
) -> Instance {
    let origin = transform.translation.map(f64::from);
    transform.translation = [0.0; 3];
    Instance {
        geometry,
        material: None,
        params: mod_api::materials::Preset::default().0,
        anchor: Anchor::World { dimension, origin },
        transform,
        tint,
        depth: draw::Depth::Always,
        blend: draw::Blend::Premultiplied,
        width: 1.5,
        width_mode: draw::WidthMode::Pixels,
        join: draw::Join::Bevel,
        cap: draw::Cap::Butt,
    }
}

/// Uses the same connected-world permission ceiling as regular extensions.
pub(super) fn session(gameplay: &super::gameplay::GameplayContext<'_>) -> DrawingSession {
    let Some(stream) = gameplay
        .world
        .as_ref()
        .filter(|world| !world.dimension_transfer.active())
        .and_then(|world| world.stream.as_ref())
    else {
        return DrawingSession::default();
    };
    let authority = stream.authority();
    DrawingSession {
        connection: authority.actor_session_id(),
        dimension: authority.current_dimension(),
        local: gameplay
            .session
            .as_ref()
            .is_some_and(|session| session.is_local_world()),
        permissions: client_extensions::modding::drawing_policy::session_permissions(
            authority.actor_session_id(),
        ),
    }
}
/// Publishes owner identity through the existing same-frame drawing authority.
pub(super) fn publish_frame(
    drawing: &Drawing,
    gameplay: &super::gameplay::GameplayContext<'_>,
    frame: &mut render_scene::mod_scene::retained::DrawingFrame,
) {
    let fence = session(gameplay);
    frame.connection = fence.connection;
    frame.dimension = fence.dimension;
    let Some(stream) = gameplay
        .world
        .as_ref()
        .and_then(|world| world.stream.as_ref())
    else {
        return;
    };
    for resources in &drawing.output.retained {
        super::drawing_policy::publish_retained(resources, stream.authority().terrain(), frame);
    }
}
/// Supports a packet-only configuration without introducing an empty component host.
pub(super) fn publish_alone(scene: &mut render_scene::ModRenderScene, drawing: &Drawing) {
    drawing.publish_blocks(scene);
    if scene.retained != drawing.output.retained
        || !scene.passes.is_empty()
        || scene.masks != drawing.output.masks
        || !scene.vertices.is_empty()
    {
        scene.apply(&drawing.output, scene.generation().wrapping_add(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Supplies the SDK's distant world-space geometry, independent of a GPU or game assets.
    fn scene() -> Arc<Scene> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../crates/mod-api/native-render-v1.json"
        ))
        .unwrap();
        Arc::new(serde_json::from_value(fixture["scene"].clone()).unwrap())
    }
    #[test]
    fn distant_drawings_reuse_geometry_and_release_on_scope_or_permission_loss() {
        let mut drawing = Drawing::default();
        let scope = DrawingSession {
            connection: 1,
            dimension: 0,
            permissions: mod_api::drawing::DrawingPermissions::ALL,
            ..Default::default()
        };
        let scene = scene();
        drawing.update(Some(Arc::clone(&scene)), scope);
        let before = Arc::clone(drawing.store.snapshot());
        assert_eq!(before.instances.len(), 3); // one volume and two bounded line segments
        drawing.update(Some(Arc::clone(&scene)), scope);
        assert!(Arc::ptr_eq(&before, drawing.store.snapshot()));
        let mut moved = (*scene).clone();
        moved.boxes[0].centre[0] += 1.;
        drawing.update(Some(Arc::new(moved)), scope);
        let after = drawing.store.snapshot();
        assert_eq!(before.geometry_revision, after.geometry_revision);
        assert!(
            before
                .geometry
                .iter()
                .all(|(handle, geometry)| Arc::ptr_eq(geometry, &after.geometry[handle]))
        );
        drawing.update(None, scope);
        assert!(drawing.output.retained.is_empty());
        drawing.update(
            Some(scene),
            DrawingSession {
                permissions: Default::default(),
                ..scope
            },
        );
        assert!(drawing.output.retained.is_empty());
    }
    #[test]
    fn packet_only_output_uses_the_current_scene_generation() {
        let mut drawing = Drawing::default();
        drawing.update(
            Some(scene()),
            DrawingSession {
                connection: 1,
                permissions: mod_api::drawing::DrawingPermissions::ALL,
                ..Default::default()
            },
        );
        let mut scene = render_scene::ModRenderScene::default();
        scene.apply(&RenderOutput::default(), drawing.generation);
        publish_alone(&mut scene, &drawing);
        assert!(!scene.retained.is_empty());
        let generation = scene.generation();
        publish_alone(&mut scene, &drawing);
        assert_eq!(scene.generation(), generation);
    }
}
