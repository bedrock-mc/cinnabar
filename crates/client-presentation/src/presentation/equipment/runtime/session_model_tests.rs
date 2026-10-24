//! Session block geometry survives atlas placement and replacement in both hand views.

use super::*;
use client_ui::ui_runtime::presentation::{SessionIcon, SessionIcons};

#[test]
fn custom_content_models_reach_both_hands_and_replace_with_artwork() {
    let (mut runtime, body, mut item) = block_fixture();
    item.identifier = Arc::from("fixture:post");
    item.kind = HeldKind::Other;
    for width in [0.25, 0.5] {
        let mut vertices = textured_cube_vertices(render_model::equipment::blocks::face_rects([
            0.0, 0.0, 1.0, 1.0,
        ]));
        for vertex in &mut vertices {
            vertex.position[0] *= width;
            vertex.position[2] *= width;
        }
        let [w, h] = assets::BLOCK_ITEM_SHEET_SIZE.map(u32::from);
        let items = crate::session_assets::SessionItems {
            components: Arc::new(Default::default()),
            icons: Some(Arc::new(SessionIcons {
                block_sheets: vec![SessionIcon {
                    identifier: Arc::clone(&item.identifier),
                    metadata: 0,
                    width: w,
                    height: h,
                    rgba8: vec![255; (w * h * 4) as usize].into(),
                }],
                block_models: BTreeMap::from([(Arc::clone(&item.identifier), vertices.into())]),
                ..Default::default()
            })),
        };
        let staged = StagedSessionIcons::stage(Some(&items)).unwrap();
        let (_, locations) = ActorArtworkPages::default().with_equipment_rasters(staged.rasters());
        runtime.set_session_items(Some(&items), Some(staged), locations);
        let main = runtime
            .first_person_item(&body, &item, ItemAnimationState::default())
            .unwrap();
        let off = runtime.first_person_offhand(&body, &item).unwrap();
        assert_eq!(
            main.presentation.submission.input.rig,
            off.presentation.submission.input.rig
        );
        let input = ActorEquipmentInput {
            main: Some(item.clone()),
            ..Default::default()
        };
        assert_eq!(runtime.layers_for(&body, &input, None).len(), 1);
        let mesh = runtime.take_pending_geometries().pop().unwrap();
        assert_eq!(mesh.vertices.len(), 36);
        for vertex in mesh.vertices.iter() {
            assert!((vertex.position[0].abs() - width / 2.0).abs() < 1e-6);
            assert!((vertex.position[2].abs() - width / 2.0).abs() < 1e-6);
            assert_eq!(vertex.position[1].abs(), 0.5);
            assert!(vertex.uv.iter().all(|uv| (0.0..=1.0).contains(uv)));
        }
    }
    runtime.set_session_items(None, None, Vec::new());
    assert!(
        runtime
            .first_person_item(&body, &item, ItemAnimationState::default())
            .is_none()
    );
}

#[test]
fn held_session_artwork_precedes_startup_carried_cubes() {
    let (mut runtime, _, item) = block_fixture();
    let (base, _, block, _) = runtime.held_mesh(&item, false).unwrap();
    assert!(block);
    for sheet in [false, true] {
        let [width, height] = assets::BLOCK_ITEM_SHEET_SIZE.map(u32::from);
        let icons = SessionIcons {
            icons: vec![SessionIcon {
                identifier: Arc::clone(&item.identifier),
                metadata: 0,
                width: 1,
                height: 1,
                rgba8: Box::new([190, 20, 80, 255]),
            }],
            block_sheets: if sheet {
                vec![SessionIcon {
                    identifier: Arc::clone(&item.identifier),
                    metadata: 0,
                    width,
                    height,
                    rgba8: vec![255; (width * height * 4) as usize].into(),
                }]
            } else {
                vec![]
            },
            ..Default::default()
        };
        let staged = StagedSessionIcons::stage_icons(&icons).unwrap();
        let (_, locations) = ActorArtworkPages::default().with_equipment_rasters(staged.rasters());
        runtime.set_session_items(None, Some(staged), locations);
        let (mesh, _, block, _) = runtime.held_mesh(&item, false).unwrap();
        assert_eq!(
            block, sheet,
            "a session icon without a sheet must be held as a sprite"
        );
        assert_ne!(mesh, base);
    }
    runtime.set_session_items(None, None, Vec::new());
    let (restored, _, block, _) = runtime.held_mesh(&item, false).unwrap();
    assert!(block);
    assert_eq!(restored, base);
}
