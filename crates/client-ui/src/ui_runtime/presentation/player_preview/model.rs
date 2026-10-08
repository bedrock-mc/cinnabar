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
}

impl MenuPreviewModel {
    fn body_matches(&self, skin: &protocol::StandardSkin) -> bool {
        match (&self.source, &skin.geometry) {
            (None, None) => true,
            (Some(old), Some(new)) => Arc::ptr_eq(old, new),
            _ => false,
        }
    }
}

impl super::UiPresentationRuntime {
    /// Retains the selected model without recompiling unchanged geometry or resampling its skin.
    pub fn set_menu_preview_skin(&mut self, skin: &protocol::StandardSkin) -> bool {
        let cape = skin.cape.as_ref().filter(|cape| cape.is_valid());
        let body_matches = self.menu_preview_model.body_matches(skin);
        let cape_matches = super::cape::same(self.menu_preview_model.cape.as_ref(), cape);
        if body_matches && cape_matches {
            return true;
        }
        let vertices = if body_matches && self.menu_preview_model.vertices.is_some() {
            self.menu_preview_model.vertices.clone()
        } else {
            model_vertices(skin)
        };
        let Some(vertices) = vertices else {
            return false;
        };
        let mut bounds = skin
            .geometry
            .as_ref()
            .map(|_| super::fitting::OrbitBounds::new(&vertices));
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
            vertices: skin.geometry.as_ref().map(|_| vertices),
        };
        self.player_preview_source_hash = None;
        self.last_frame = None;
        true
    }
}

fn model_vertices(skin: &protocol::StandardSkin) -> Option<Arc<[ActorVertex]>> {
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
        assert!(runtime.set_menu_preview_skin(&skin));
        let body = runtime.menu_preview_model.vertices.clone().unwrap();
        skin.cape = Some(protocol::CapeImage {
            width: 64,
            height: 32,
            rgba8: vec![255; 64 * 32 * 4].into(),
        });
        assert!(runtime.set_menu_preview_skin(&skin));
        assert!(Arc::ptr_eq(
            &body,
            runtime.menu_preview_model.vertices.as_ref().unwrap()
        ));
        assert!(runtime.menu_preview_model.cape_key.is_some());
        runtime.player_preview_source_hash = Some([17; 32]);
        assert!(runtime.set_menu_preview_skin(&skin.clone()));
        assert_eq!(runtime.player_preview_source_hash, Some([17; 32]));
        skin.cape = None;
        assert!(runtime.set_menu_preview_skin(&skin));
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
