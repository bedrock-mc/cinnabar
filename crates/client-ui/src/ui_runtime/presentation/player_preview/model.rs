use std::sync::Arc;

use render_model::ActorVertex;

#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
use browser::spawn as spawn_model;
#[cfg(not(target_arch = "wasm32"))]
use native::spawn as spawn_model;

use super::{PlayerPreviewPose, PreviewEquipment, skin::validated_ui_skin};

#[derive(Default)]
pub(crate) struct MenuPreviewModel {
    source: Option<Arc<render_api::SkinGeometrySource>>,
    pub(crate) vertices: Option<Arc<[ActorVertex]>>,
    pub(crate) cape: Option<render_api::CapeImage>,
    pub(crate) cosmetics: Vec<cosmetics::Item>,
    pub(crate) cape_key: Option<String>,
    pub(super) bounds: Option<super::fitting::OrbitBounds>,
    body_bounds: Option<super::fitting::OrbitBounds>,
    pending: Option<PendingModel>,
    completed: Option<PreparedModel>,
    rejected: Option<Arc<render_api::SkinGeometrySource>>,
}

/// One changed source in flight; stale results are discarded before publication.
struct PendingModel {
    source: Option<Arc<render_api::SkinGeometrySource>>,
    receiver: crossbeam_channel::Receiver<Option<PreparedModel>>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for PendingModel {
    /// Stops retained browser work when its runtime or request is discarded.
    fn drop(&mut self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

struct PreparedModel {
    source: Option<Arc<render_api::SkinGeometrySource>>,
    vertices: Arc<[ActorVertex]>,
    bounds: Option<super::fitting::OrbitBounds>,
}

/// Compares retained pointers without parsing or hashing on the frame thread.
fn same_source(
    a: Option<&Arc<render_api::SkinGeometrySource>>,
    b: Option<&Arc<render_api::SkinGeometrySource>>,
) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        _ => false,
    }
}

impl MenuPreviewModel {
    /// Keeps the current model while only texture or cape inputs change.
    fn body_matches(&self, skin: &render_api::StandardSkin) -> bool {
        same_source(self.source.as_ref(), skin.geometry.as_ref())
    }
}

impl super::UiPresentationRuntime {
    /// Updates headwear without reloading skin geometry or regenerating skin textures.
    pub fn set_preview_cosmetics(&mut self, items: &[cosmetics::Item]) {
        if self.menu_preview_model.cosmetics == items {
            return;
        }
        self.menu_preview_model.cosmetics = items.to_vec();
        let mut bounds = self
            .menu_preview_model
            .body_bounds
            .unwrap_or_else(super::fitting::standard);
        if self.menu_preview_model.cape.is_some() {
            bounds = bounds.with_static_geometry(super::cape::rest_vertices());
        }
        for &hat in items {
            if render_model::hats::rig(hat).is_ground_effect() {
                continue;
            }
            bounds = bounds.with_static_geometry(&render_model::hats::rig(hat).rest_vertices);
        }
        self.menu_preview_model.bounds = Some(bounds);
        self.last_frame = None;
        self.player_preview_source_hash = None;
        self.sync_equipped_cosmetic_artwork();
    }

    /// Publishes a worker-built model, retaining the previous preview while a changed source prepares.
    pub fn set_menu_preview_skin(&mut self, skin: &render_api::StandardSkin) -> bool {
        if let Some(pending) = &self.menu_preview_model.pending
            && !same_source(pending.source.as_ref(), skin.geometry.as_ref())
        {
            pending
                .cancelled
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
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
        for &hat in &self.menu_preview_model.cosmetics {
            if render_model::hats::rig(hat).is_ground_effect() {
                continue;
            }
            bounds = Some(
                bounds
                    .unwrap_or_else(super::fitting::standard)
                    .with_static_geometry(&render_model::hats::rig(hat).rest_vertices),
            );
        }
        self.menu_preview_model = MenuPreviewModel {
            cosmetics: self.menu_preview_model.cosmetics.clone(),
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
    fn prepare(&mut self, skin: &render_api::StandardSkin) -> Option<PreparedModel> {
        if same_source(self.rejected.as_ref(), skin.geometry.as_ref()) && self.rejected.is_some() {
            return None;
        }
        if let Some(pending) = &self.pending {
            match pending.receiver.try_recv() {
                Ok(prepared) => {
                    if prepared.is_none()
                        && !pending.cancelled.load(std::sync::atomic::Ordering::Relaxed)
                    {
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
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        if spawn_model(skin, sender, cancelled.clone()).is_err() {
            self.rejected = source;
            return None;
        }
        self.pending = Some(PendingModel {
            source,
            receiver,
            cancelled,
        });
        None
    }
}

/// Builds immutable preview vertices only on a worker or in a synchronous fixture.
fn model_vertices(skin: &render_api::StandardSkin) -> Option<Arc<[ActorVertex]>> {
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
            .map(|vertex| preview_vertex(vertex, &parts))
            .collect::<Vec<_>>()
            .into(),
    )
}

/// Converts the actor rig into the preview's facing and body-part convention.
fn preview_vertex(vertex: &render_model::ActorRigVertex, parts: &[u32]) -> ActorVertex {
    ActorVertex {
        position: [-vertex.position[0], vertex.position[1], -vertex.position[2]],
        uv: vertex.uv,
        part: parts[vertex.bone_index as usize],
    }
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
pub fn render_skin_thumbnail(skin: &render_api::StandardSkin) -> Option<Vec<u8>> {
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

    fn skin(arm_width: f32) -> render_api::StandardSkin {
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
        render_api::StandardSkin {
            width: 64,
            height: 64,
            rgba8: vec![255; 64 * 64 * 4].into(),
            cape: None,
            geometry: Some(Arc::new(render_api::SkinGeometrySource {
                resource_patch: r#"{"geometry":{"default":"geometry.preview.fixture"}}"#.into(),
                geometry_data: geometry.to_string().into(),
                animations: Arc::from([]),
            })),
        }
    }

    /// Awaits this fixture's own model job without relying on frame timing.
    fn select(runtime: &mut super::super::UiPresentationRuntime, skin: &render_api::StandardSkin) {
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
            super::super::UiPresentationRuntime::new(ui_engine::test_support::fixture_font())
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
    fn superseded_preview_work_never_replaces_the_latest_selected_model() {
        let mut runtime =
            super::super::UiPresentationRuntime::new(ui_engine::test_support::fixture_font())
                .unwrap();
        let initial = skin(4.0);
        select(&mut runtime, &initial);
        let previous = runtime.menu_preview_model.vertices.clone().unwrap();
        let obsolete = skin(3.0);
        let latest = skin(2.0);
        assert!(!runtime.set_menu_preview_skin(&obsolete));
        test_time::eventually("latest selected preview replaces superseded work", || {
            let ready = runtime.set_menu_preview_skin(&latest);
            if !ready {
                assert!(Arc::ptr_eq(
                    &previous,
                    runtime.menu_preview_model.vertices.as_ref().unwrap()
                ));
            }
            ready
        });
        assert_eq!(
            runtime.menu_preview_model.vertices.as_deref(),
            model_vertices(&latest).as_deref()
        );
    }

    #[test]
    fn returning_to_the_displayed_skin_cancels_obsolete_preview_work() {
        let mut runtime =
            super::super::UiPresentationRuntime::new(ui_engine::test_support::fixture_font())
                .unwrap();
        let displayed = skin(4.0);
        select(&mut runtime, &displayed);
        assert!(!runtime.set_menu_preview_skin(&skin(3.0)));
        let cancelled = runtime
            .menu_preview_model
            .pending
            .as_ref()
            .unwrap()
            .cancelled
            .clone();
        assert!(runtime.set_menu_preview_skin(&displayed));
        assert!(cancelled.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn selected_skin_geometry_changes_the_gallery_outline() {
        let wide = render_skin_thumbnail(&skin(4.0)).unwrap();
        let slim = render_skin_thumbnail(&skin(3.0)).unwrap();
        assert!(wide != slim, "the selected model changes the rendered arms");
        let opaque = |image: &[u8]| {
            image
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| pixel[3] != 0)
                .count()
        };
        assert!(opaque(&slim) < opaque(&wide));
    }

    #[test]
    fn selected_cape_changes_the_character_thumbnail_without_changing_the_skin() {
        let bare = skin(4.0);
        let mut caped = bare.clone();
        caped.cape = Some(render_api::CapeImage {
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
            super::super::UiPresentationRuntime::new(ui_engine::test_support::fixture_font())
                .expect("diagnostic presentation");
        let mut skin = skin(4.0);
        select(&mut runtime, &skin);
        let body = runtime.menu_preview_model.vertices.clone().unwrap();
        skin.cape = Some(render_api::CapeImage {
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

#[cfg(test)]
mod ground_tests {
    use crate::ui_runtime::presentation::UiPresentationRuntime;
    #[test]
    fn equipping_a_ground_item_preserves_the_character_frame() {
        let mut runtime =
            UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
        let control = [0.0, 0.0, 200.0, 300.0];
        let expected = super::super::fitting::standard().frame(control, 15.0);
        runtime.set_preview_cosmetics(&[render_model::hats::fixture::ground_effect()]);
        assert_eq!(
            runtime
                .menu_preview_model
                .bounds
                .unwrap()
                .frame(control, 15.0),
            expected
        );
    }
}
