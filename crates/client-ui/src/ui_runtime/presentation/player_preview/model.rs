use std::sync::Arc;

use render_model::ActorVertex;

use super::{PlayerPreviewPose, PreviewEquipment, skin::validated_ui_skin};

#[derive(Default)]
pub(crate) struct MenuPreviewModel {
    source: Option<Arc<protocol::SkinGeometrySource>>,
    pub(crate) vertices: Option<Arc<[ActorVertex]>>,
    pub(crate) cape: Option<protocol::CapeImage>,
    pub(crate) cape_key: Option<String>,
    pub(super) bounds: Option<super::fitting::OrbitBounds>,
    body_bounds: Option<super::fitting::OrbitBounds>,
    pending: Option<PendingModel>,
    completed: Option<PreparedModel>,
    rejected: Option<Arc<protocol::SkinGeometrySource>>,
}

/// One changed source in flight; stale results are discarded before publication.
struct PendingModel {
    source: Option<Arc<protocol::SkinGeometrySource>>,
    receiver: crossbeam_channel::Receiver<Option<PreparedModel>>,
}

struct PreparedModel {
    source: Option<Arc<protocol::SkinGeometrySource>>,
    vertices: Arc<[ActorVertex]>,
    bounds: Option<super::fitting::OrbitBounds>,
}

/// Compares retained pointers without parsing or hashing on the frame thread.
fn same_source(
    a: Option<&Arc<protocol::SkinGeometrySource>>,
    b: Option<&Arc<protocol::SkinGeometrySource>>,
) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        _ => false,
    }
}

impl MenuPreviewModel {
    /// Keeps the current model while only texture or cape inputs change.
    fn body_matches(&self, skin: &protocol::StandardSkin) -> bool {
        same_source(self.source.as_ref(), skin.geometry.as_ref())
    }
}

impl super::UiPresentationRuntime {
    /// Publishes a worker-built model, retaining the previous preview while a changed source prepares.
    pub fn set_menu_preview_skin(&mut self, skin: &protocol::StandardSkin) -> bool {
        let cape = skin.cape.as_ref().filter(|cape| cape.is_valid());
        let body_matches = self.menu_preview_model.body_matches(skin);
        let cape_matches = super::cape::same(self.menu_preview_model.cape.as_ref(), cape);
        if body_matches && cape_matches {
            return true;
        }
        let (vertices, mut bounds) = if body_matches {
            (
                self.menu_preview_model.vertices.clone(),
                self.menu_preview_model.body_bounds,
            )
        } else {
            let Some(prepared) = self.menu_preview_model.prepare(skin) else {
                return false;
            };
            (Some(prepared.vertices), prepared.bounds)
        };
        let body_bounds = bounds;
        if cape.is_some() {
            bounds = Some(
                bounds
                    .unwrap_or_else(super::fitting::standard)
                    .with_static_geometry(super::cape::rest_vertices()),
            );
        }
        self.menu_preview_model = MenuPreviewModel {
            source: skin.geometry.clone(),
            cape_key: if cape_matches {
                self.menu_preview_model.cape_key.clone()
            } else {
                cape.map(super::super::menu_artwork::cape_texture_key)
            },
            cape: cape.cloned(),
            bounds,
            body_bounds,
            vertices,
            pending: self.menu_preview_model.pending.take(),
            completed: None,
            rejected: None,
        };
        self.player_preview_source_hash = None;
        self.last_frame = None;
        true
    }
}

impl MenuPreviewModel {
    /// Admits one source at a time and never waits for an unfinished preparation.
    fn prepare(&mut self, skin: &protocol::StandardSkin) -> Option<PreparedModel> {
        if same_source(self.rejected.as_ref(), skin.geometry.as_ref()) && self.rejected.is_some() {
            return None;
        }
        if let Some(pending) = &self.pending {
            match pending.receiver.try_recv() {
                Ok(prepared) => {
                    if prepared.is_none() {
                        self.rejected = pending.source.clone();
                    }
                    self.completed = prepared;
                    self.pending = None;
                }
                Err(crossbeam_channel::TryRecvError::Empty) => return None,
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    self.pending = None;
                }
            }
        }
        if let Some(prepared) = self.completed.take()
            && same_source(prepared.source.as_ref(), skin.geometry.as_ref())
        {
            return Some(prepared);
        }
        if self.rejected.is_some() && same_source(self.rejected.as_ref(), skin.geometry.as_ref()) {
            return None;
        }
        let source = skin.geometry.clone();
        let skin = skin.clone();
        let (sender, receiver) = crossbeam_channel::bounded(1);
        if std::thread::Builder::new()
            .name("menu-skin-model".into())
            .spawn(move || {
                let prepared = model_vertices(&skin).map(|vertices| PreparedModel {
                    bounds: skin
                        .geometry
                        .as_ref()
                        .map(|_| super::fitting::OrbitBounds::new(&vertices)),
                    source: skin.geometry,
                    vertices,
                });
                let _ = sender.send(prepared);
            })
            .is_err()
        {
            self.rejected = source;
            return None;
        }
        self.pending = Some(PendingModel { source, receiver });
        None
    }
}

/// Builds immutable preview vertices only on a worker or in a synchronous fixture.
fn model_vertices(skin: &protocol::StandardSkin) -> Option<Arc<[ActorVertex]>> {
    #[cfg(test)]
    BUILT_ON_THIS_THREAD.with(|built| built.set(built.get() + 1));
    let Some(source) = &skin.geometry else {
        let mut vertices = render_model::standard_biped_vertices();
        vertices.extend(render_model::standard_biped_overlay_vertices());
        return Some(vertices.into());
    };
    let geometry =
        assets::parse_skin_geometry(&source.resource_patch, &source.geometry_data).ok()??;
    let rig = render_model::skin_geometry(&geometry, render_model::EntityRigId(0)).ok()?;
    let parts: Vec<_> = (0..geometry.bones.len())
        .map(|index| part(&geometry, index))
        .collect();
    Some(
        rig.vertices
            .iter()
            .map(|vertex| ActorVertex {
                // The preview faces +Z and places the actor's right side at -X.
                position: [-vertex.position[0], vertex.position[1], -vertex.position[2]],
                uv: vertex.uv,
                part: parts[vertex.bone_index as usize],
            })
            .collect::<Vec<_>>()
            .into(),
    )
}

/// Maps a custom bone to its nearest named player part for the preview pose.
fn part(geometry: &assets::SkinGeometry, mut index: usize) -> u32 {
    for _ in 0..geometry.bones.len() {
        let bone = &geometry.bones[index];
        if let Some(part) = ["head", "body", "rightArm", "leftArm", "rightLeg", "leftLeg"]
            .iter()
            .position(|name| bone.name.eq_ignore_ascii_case(name))
        {
            return part as u32;
        }
        let Some(parent) = bone.parent.as_deref().and_then(|parent| {
            geometry
                .bones
                .iter()
                .position(|bone| bone.name.eq_ignore_ascii_case(parent))
        }) else {
            break;
        };
        index = parent;
    }
    1
}

/// Builds gallery artwork from the selected skin's original texels and model.
pub fn render_skin_thumbnail(skin: &protocol::StandardSkin) -> Option<Vec<u8>> {
    let pixels = validated_ui_skin(&render_model::ActorSkinPixels {
        width: skin.width,
        height: skin.height,
        rgba8: skin.rgba8.clone(),
    })?;
    Some(super::render_body_with_cape(
        &model_vertices(skin)?,
        &pixels,
        PlayerPreviewPose::default(),
        super::MenuPreviewConfig::MENU.view([0.0; 2]),
        0.0,
        &PreviewEquipment::default(),
        skin.cape.as_ref(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skin(arm_width: f32) -> protocol::StandardSkin {
        let arm_min = -4.0 - arm_width;
        let geometry = serde_json::json!({
            "format_version":"1.12.0",
            "minecraft:geometry":[{
                "description":{"identifier":"geometry.preview.fixture","texture_width":64,"texture_height":64},
                "bones":[
                    {"name":"head","pivot":[0,24,0],"cubes":[{"origin":[-4,24,-4],"size":[8,8,8],"uv":[0,0]}]},
                    {"name":"body","pivot":[0,24,0],"cubes":[{"origin":[-4,12,-2],"size":[8,12,4],"uv":[16,16]}]},
                    {"name":"rightArm","pivot":[-5,22,0],"cubes":[{"origin":[arm_min,12,-2],"size":[arm_width,12,4],"uv":[40,16]}]},
                    {"name":"leftArm","pivot":[5,22,0],"cubes":[{"origin":[4,12,-2],"size":[arm_width,12,4],"uv":[32,48]}]},
                    {"name":"rightLeg","pivot":[-2,12,0],"cubes":[{"origin":[-4,0,-2],"size":[4,12,4],"uv":[0,16]}]},
                    {"name":"leftLeg","pivot":[2,12,0],"cubes":[{"origin":[0,0,-2],"size":[4,12,4],"uv":[16,48]}]}
                ]
            }]
        });
        protocol::StandardSkin {
            width: 64,
            height: 64,
            rgba8: vec![255; 64 * 64 * 4].into(),
            cape: None,
            geometry: Some(Arc::new(protocol::SkinGeometrySource {
                resource_patch: r#"{"geometry":{"default":"geometry.preview.fixture"}}"#.into(),
                geometry_data: geometry.to_string().into(),
                animations: Arc::from([]),
            })),
        }
    }

    /// Awaits this fixture's own model job without relying on frame timing.
    fn select(runtime: &mut super::super::UiPresentationRuntime, skin: &protocol::StandardSkin) {
        while !runtime.set_menu_preview_skin(skin) {
            let pending = runtime
                .menu_preview_model
                .pending
                .take()
                .expect("model worker");
            runtime.menu_preview_model.completed = pending.receiver.recv().unwrap();
            assert!(runtime.set_menu_preview_skin(skin));
        }
    }

    #[test]
    fn changed_preview_models_prepare_off_the_frame_thread_and_keep_the_previous_model() {
        let mut runtime =
            super::super::UiPresentationRuntime::new(super::super::super::tests::fixture_font())
                .unwrap();
        let before = BUILT_ON_THIS_THREAD.with(std::cell::Cell::get);
        let wide = skin(4.0);
        select(&mut runtime, &wide);
        let previous = runtime.menu_preview_model.vertices.clone().unwrap();
        let slim = skin(3.0);
        assert!(!runtime.set_menu_preview_skin(&slim));
        assert!(Arc::ptr_eq(
            &previous,
            runtime.menu_preview_model.vertices.as_ref().unwrap()
        ));
        select(&mut runtime, &slim);
        assert!(!Arc::ptr_eq(
            &previous,
            runtime.menu_preview_model.vertices.as_ref().unwrap()
        ));
        assert_eq!(BUILT_ON_THIS_THREAD.with(std::cell::Cell::get), before);
        let ready = runtime.menu_preview_model.vertices.clone().unwrap();
        select(&mut runtime, &slim);
        assert!(Arc::ptr_eq(
            &ready,
            runtime.menu_preview_model.vertices.as_ref().unwrap()
        ));
    }

    #[test]
    fn selected_skin_geometry_changes_the_gallery_outline() {
        let wide = render_skin_thumbnail(&skin(4.0)).unwrap();
        let slim = render_skin_thumbnail(&skin(3.0)).unwrap();
        assert!(wide != slim, "the selected model changes the rendered arms");
        let opaque = |image: &[u8]| image.chunks_exact(4).filter(|pixel| pixel[3] != 0).count();
        assert!(opaque(&slim) < opaque(&wide));
    }

    #[test]
    fn selected_cape_changes_the_character_thumbnail_without_changing_the_skin() {
        let bare = skin(4.0);
        let mut caped = bare.clone();
        caped.cape = Some(protocol::CapeImage {
            width: 64,
            height: 32,
            rgba8: [17, 229, 61, 255].repeat(64 * 32).into(),
        });
        let bare_thumbnail = render_skin_thumbnail(&bare).unwrap();
        let caped_thumbnail = render_skin_thumbnail(&caped).unwrap();
        assert!(
            bare_thumbnail != caped_thumbnail,
            "an attached cape contributes geometry with its own texels"
        );
        assert_eq!(bare.rgba8, caped.rgba8);
    }

    #[test]
    fn cape_changes_keep_the_body_model_and_unchanged_cape_keeps_the_render_cache() {
        let mut runtime =
            super::super::UiPresentationRuntime::new(super::super::super::tests::fixture_font())
                .expect("diagnostic presentation");
        let mut skin = skin(4.0);
        select(&mut runtime, &skin);
        let body = runtime.menu_preview_model.vertices.clone().unwrap();
        skin.cape = Some(protocol::CapeImage {
            width: 64,
            height: 32,
            rgba8: vec![255; 64 * 32 * 4].into(),
        });
        select(&mut runtime, &skin);
        assert!(Arc::ptr_eq(
            &body,
            runtime.menu_preview_model.vertices.as_ref().unwrap()
        ));
        assert!(runtime.menu_preview_model.cape_key.is_some());
        runtime.player_preview_source_hash = Some([17; 32]);
        select(&mut runtime, &skin.clone());
        assert_eq!(runtime.player_preview_source_hash, Some([17; 32]));
        skin.cape = None;
        select(&mut runtime, &skin);
        assert!(runtime.menu_preview_model.cape.is_none());
        assert!(runtime.menu_preview_model.cape_key.is_none());
        assert!(Arc::ptr_eq(
            &body,
            runtime.menu_preview_model.vertices.as_ref().unwrap()
        ));
    }

    #[test]
    fn gesture_paper_doll_tracks_pointer_without_tilting_the_body() {
        let data = std::collections::BTreeMap::from([
            ("rotation".into(), serde_json::json!("gesture_x")),
            ("starting_rotation".into(), serde_json::json!(30.0)),
            ("camera_tilt_degrees".into(), serde_json::json!(-10.0)),
        ]);
        let (view, _) = super::super::renderer_frame(
            "paper_doll_renderer",
            &data,
            [100.0, 100.0, 300.0, 400.0],
            2.0,
            Some([0.0, 0.0]),
        );
        let [body, head, pitch, tilt] = view.angles();
        assert_eq!(
            body, 30.0,
            "pointer hover preserves the authored body rotation"
        );
        assert_eq!(
            tilt, -10.0,
            "pointer hover preserves the authored camera tilt"
        );
        assert_ne!(
            head, body,
            "the head turns independently toward the pointer"
        );
        assert_ne!(pitch, 0.0, "the head also follows the pointer vertically");
    }
}

#[cfg(test)]
thread_local! { static BUILT_ON_THIS_THREAD: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
