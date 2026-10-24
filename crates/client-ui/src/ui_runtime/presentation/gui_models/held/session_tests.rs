use super::*;
use crate::test_support::fixture_hud;
use crate::ui_runtime::presentation::{
    SessionIcon, SessionIcons, UiPresentationRuntime, session_icons,
};
use ui_engine::test_support::fixture_font;

const ITEM: &str = "minecraft:diamond_sword";

/// Original asymmetric sword art differs from the base sprite in size, color and alpha edges.
fn sword(metadata: u32, color: [u8; 3]) -> SessionIcon {
    let [width, height] = [8, 12];
    let rgba8 = (0..width * height)
        .flat_map(|pixel| {
            let [x, y] = [pixel % width, pixel / width];
            let opaque = (x == 3 && y < 10) || (y == 8 && (1..7).contains(&x));
            [color[0], color[1], color[2], if opaque { 255 } else { 0 }]
        })
        .collect::<Vec<_>>()
        .into();
    SessionIcon {
        identifier: Arc::from(ITEM),
        metadata,
        width,
        height,
        rgba8,
    }
}

/// Uses original carried sprites and hand bones without installed game carriers.
fn presentation() -> UiPresentationRuntime {
    let (entities, icons) = tests::sprite_catalog(ITEM, ItemVisualDefinitionRoute::Missing);
    let mut presentation =
        UiPresentationRuntime::with_hud_and_icons(fixture_font(), fixture_hud(), Arc::new(icons))
            .unwrap();
    presentation
        .set_gui_models(&assets::RuntimeAssets::diagnostic(), &entities)
        .unwrap();
    presentation.set_player_preview_skin(Some(&[255; 64 * 64 * 4]), Default::default());
    presentation
}

#[test]
fn preview_projects_body_once_for_base_and_session_items() {
    use ui::{UiLimits, UiNode, UiNodeId, UiVisual};
    for session in [false, true] {
        let mut presentation = presentation();
        if session {
            install(&mut presentation, vec![sword(0, [220, 80, 12])]);
        }
        presentation.set_player_preview_gear([None; 4], Some((ITEM, 0)));
        let custom = render_model::standard_biped_vertices().repeat(3);
        presentation.menu_preview_model.vertices = Some(custom.clone().into());
        let preview = IconRef {
            color_mask: None,
            page: u16::MAX,
            uv: [0, 0, 96, 112],
            glint: false,
        };
        presentation.player_preview_icon = Some(preview);
        let bounds = crate::ui_runtime::presentation::rect(0.0, 0.0, 96.0, 112.0).unwrap();
        for view in [
            player_preview::PreviewView::Hud,
            player_preview::PreviewView::Live { offset: [0.0; 2] },
        ] {
            presentation.player_preview_view = view;
            let body_vertices = if view == player_preview::PreviewView::Hud {
                render_model::standard_biped_vertices().len()
                    + render_model::standard_biped_overlay_vertices().len()
            } else {
                custom.len()
            };
            for surrounding in [0, UiLimits::MAX_DRAW_BATCHES] {
                for frame in 0..2 {
                    presentation.player_preview_bob = frame as f32;
                    let mut nodes = (0..surrounding)
                        .map(|index| {
                            UiNode::new(UiNodeId::new(index as u32 + 1), None, bounds).with_visual(
                                UiVisual::Solid {
                                    texture_page: 0,
                                    color: [255; 4],
                                },
                            )
                        })
                        .collect::<Vec<_>>();
                    nodes.extend((0..2).map(|index| {
                        UiNode::new(
                            UiNodeId::new((surrounding + index + 1) as u32),
                            None,
                            bounds,
                        )
                        .with_visual(UiVisual::Sprite {
                            texture_page: preview.page,
                            uv: preview.uv,
                            color: [255; 4],
                        })
                    }));
                    player_preview::geometry::take_body_projection_work();
                    presentation.apply_gui_models(&mut nodes);
                    assert_eq!(
                        player_preview::geometry::take_body_projection_work(),
                        body_vertices,
                        "one body projection serves both preview nodes in each animated frame"
                    );
                    for node in &nodes[surrounding..] {
                        let UiVisual::Mesh(mesh) = node.visual() else {
                            panic!("the preview must retain its model");
                        };
                        assert_eq!(mesh.batches().len(), 2, "the held item remains admitted");
                    }
                }
            }
        }
    }
}

#[test]
fn preview_held_sword_uses_session_icon_texels_and_alpha_edges() {
    let mut presentation = presentation();
    let art = sword(0, [12, 38, 96]);
    let expected = held_sprite_vertices(
        art.width as usize,
        art.height as usize,
        &art.rgba8,
        [0.0, 0.0, 1.0, 1.0],
    )
    .unwrap();
    let icons = Arc::new(SessionIcons {
        icons: vec![art],
        ..Default::default()
    });
    session_icons::observe(&mut presentation, Some(&icons));
    presentation.set_player_preview_gear([None; 4], Some((ITEM, 0)));
    let hotbar = presentation.item_icon(ITEM, 0).unwrap();
    let mesh = presentation.gui_player_mesh().unwrap();
    let held = mesh.batches().last().unwrap();
    assert_eq!(
        held.texture_page, hotbar.page,
        "preview and hotbar must sample the active pack"
    );
    assert_eq!(
        held.index_range.len(),
        expected.len(),
        "the pack alpha mask must determine extrusion edges"
    );
    for vertex in &mesh.vertices()[held.index_range.start as usize..held.index_range.end as usize] {
        assert!(vertex.uv[0] >= f32::from(hotbar.uv[0]) && vertex.uv[0] <= f32::from(hotbar.uv[2]));
        assert!(vertex.uv[1] >= f32::from(hotbar.uv[1]) && vertex.uv[1] <= f32::from(hotbar.uv[3]));
    }
}

/// Publishes original artwork through the production session page.
fn install(presentation: &mut UiPresentationRuntime, icons: Vec<SessionIcon>) -> Arc<SessionIcons> {
    let icons = Arc::new(SessionIcons {
        icons,
        ..Default::default()
    });
    session_icons::observe(presentation, Some(&icons));
    icons
}

#[test]
fn preview_session_variants_and_both_hands_share_inventory_regions_and_cached_edges() {
    let mut presentation = presentation();
    let icons = install(
        &mut presentation,
        vec![sword(0, [12, 38, 96]), sword(7, [220, 80, 12])],
    );
    for metadata in [7, 29] {
        presentation.set_player_preview_gear([None; 4], Some((ITEM, metadata)));
        presentation.player_preview_gear.hands[1] =
            presentation.player_preview_gear.hands[0].clone();
        let icon = presentation.item_icon(ITEM, metadata).unwrap();
        let (_, first) = presentation
            .session_icons
            .held_sprite(ITEM, metadata)
            .unwrap();
        let textures = Arc::clone(&presentation.textures);
        for view in [
            player_preview::PreviewView::Hud,
            player_preview::PreviewView::Live {
                offset: [4.0, -2.0],
            },
        ] {
            presentation.player_preview_view = view;
            let mesh = presentation.gui_player_mesh().unwrap();
            let held = &mesh.batches()[mesh.batches().len() - 2..];
            for batch in held {
                assert_eq!(batch.texture_page, icon.page);
                assert_eq!(batch.index_range.len(), first.len());
                for index in batch.index_range.clone() {
                    let vertex = &mesh.vertices()[mesh.indices()[index as usize] as usize];
                    assert!(
                        vertex.uv[0] >= f32::from(icon.uv[0])
                            && vertex.uv[0] <= f32::from(icon.uv[2])
                    );
                    assert!(
                        vertex.uv[1] >= f32::from(icon.uv[1])
                            && vertex.uv[1] <= f32::from(icon.uv[3])
                    );
                }
            }
        }
        session_icons::observe(&mut presentation, Some(&icons));
        let (_, again) = presentation
            .session_icons
            .held_sprite(ITEM, metadata)
            .unwrap();
        assert!(Arc::ptr_eq(&first, &again));
        assert!(Arc::ptr_eq(&textures, &presentation.textures));
        let pixels = &presentation
            .session_icons
            .sprite(ITEM, metadata)
            .unwrap()
            .rgba8;
        let expected = if metadata == 7 {
            [220, 80, 12]
        } else {
            [12, 38, 96]
        };
        assert_eq!(&pixels[..3], expected);
    }
}

#[test]
fn preview_session_replacement_removal_and_transparency_retire_old_meshes() {
    let mut presentation = presentation();
    presentation.set_player_preview_gear([None; 4], Some((ITEM, 0)));
    let base = presentation.gui_player_mesh().unwrap();
    let base_icon = presentation.item_icon(ITEM, 0).unwrap();
    install(&mut presentation, vec![sword(0, [12, 38, 96])]);
    let (_, first_mesh) = presentation.session_icons.held_sprite(ITEM, 0).unwrap();
    let first = Arc::downgrade(&first_mesh);
    drop(first_mesh);
    let changed = presentation.gui_player_mesh().unwrap();
    let body = base.batches()[0].index_range.clone();
    for (base, changed) in base.vertices()[body.start as usize..body.end as usize]
        .iter()
        .zip(&changed.vertices()[body.start as usize..body.end as usize])
    {
        assert_eq!(base.position, changed.position);
        assert_eq!(base.uv, changed.uv);
        assert_eq!(base.color, changed.color);
        assert_eq!(base.model_light, changed.model_light);
    }
    let mut transparent = sword(0, [220, 80, 12]);
    transparent.rgba8.fill(0);
    install(&mut presentation, vec![transparent]);
    assert!(first.upgrade().is_none());
    let (source, transparent_mesh) = presentation.session_icons.held_sprite(ITEM, 0).unwrap();
    assert_eq!(
        transparent_mesh.len(),
        12,
        "transparent art retains only alpha-tested front/back faces"
    );
    assert!(
        presentation
            .session_icons
            .sprite(ITEM, 0)
            .unwrap()
            .rgba8
            .iter()
            .all(|byte| *byte == 0)
    );
    let empty = presentation.gui_player_mesh().unwrap();
    assert_eq!(empty.batches().last().unwrap().texture_page, source.page);
    assert_eq!(
        empty.batches().last().unwrap().index_range.len(),
        transparent_mesh.len()
    );
    session_icons::observe(&mut presentation, None);
    assert_eq!(presentation.item_icon(ITEM, 0), Some(base_icon));
    let restored = presentation.gui_player_mesh().unwrap();
    assert_eq!(restored.vertices(), base.vertices());
    assert_eq!(restored.indices(), base.indices());
}

#[test]
fn preview_custom_session_identity_and_cpu_fallback_use_admitted_pixels() {
    let mut presentation = presentation();
    let identifier = "fixture:preview_sword";
    let mut art = sword(0, [220, 80, 12]);
    art.identifier = identifier.into();
    install(&mut presentation, vec![art]);
    assert!(
        presentation
            .icon_catalog
            .as_ref()
            .unwrap()
            .lookup(identifier, 0)
            .is_none()
    );
    presentation.set_player_preview_gear([None; 4], Some((identifier, 42)));
    let mesh = presentation.gui_player_mesh().unwrap();
    assert_eq!(
        mesh.batches().last().unwrap().texture_page,
        presentation.item_icon(identifier, 42).unwrap().page
    );
    let cpu = presentation.player_preview_gear.held.as_ref().unwrap();
    assert_eq!([cpu.width, cpu.height], [8, 12]);
    assert_eq!(&cpu.rgba[..3], [220, 80, 12]);
    session_icons::observe(&mut presentation, None);
    presentation.set_player_preview_gear([None; 4], Some((identifier, 42)));
    assert!(presentation.player_preview_gear.held.is_none());
    assert_eq!(
        presentation.gui_player_mesh().unwrap().batches().len() + 1,
        mesh.batches().len()
    );
}

#[test]
fn preview_session_thumbnail_does_not_replace_block_or_authored_held_geometry() {
    let mut presentation = presentation();
    let source = IconRef {
        color_mask: None,
        page: 3,
        uv: [0, 0, 64, 48],
        glint: true,
    };
    let model = block(source, [[0.0; 3]; 2]);
    presentation.gui_models.sources.held.insert(
        ItemVisualKey {
            identifier: ITEM.into(),
            metadata: 0,
        },
        HeldModel::ready(model),
    );
    presentation.set_player_preview_gear([None; 4], Some((ITEM, 0)));
    let base = presentation.gui_player_mesh().unwrap();
    install(&mut presentation, vec![sword(0, [220, 80, 12])]);
    let changed = presentation.gui_player_mesh().unwrap();
    assert_eq!(base.vertices(), changed.vertices());
    assert_eq!(changed.batches().last().unwrap().texture_page, source.page);
    assert!(
        changed
            .vertices()
            .iter()
            .any(|vertex| vertex.style_flags & u16::from(ui::UI_STYLE_GLINT) != 0)
    );
}

#[test]
fn preview_rejects_invalid_duplicate_session_sprite_before_the_valid_source() {
    let mut presentation = presentation();
    let mut bad = sword(0, [1, 2, 3]);
    bad.rgba8 = Box::from([]);
    install(
        &mut presentation,
        vec![bad, sword(0, [12, 38, 96]), sword(0, [220, 80, 12])],
    );
    assert_eq!(
        &presentation.session_icons.sprite(ITEM, 0).unwrap().rgba8[..3],
        [12, 38, 96]
    );
    presentation.set_player_preview_gear([None; 4], Some((ITEM, 0)));
    assert_eq!(
        presentation
            .gui_player_mesh()
            .unwrap()
            .batches()
            .last()
            .unwrap()
            .texture_page,
        presentation.item_icon(ITEM, 0).unwrap().page
    );
}

#[test]
fn session_preview_cpu_pixels_are_reused_between_dress_updates() {
    let mut presentation = presentation();
    install(&mut presentation, vec![sword(0, [12, 38, 96])]);
    presentation.set_player_preview_gear([None; 4], Some((ITEM, 0)));
    let first = Arc::clone(&presentation.player_preview_gear.held.as_ref().unwrap().rgba);
    presentation.set_player_preview_gear([None; 4], Some((ITEM, 0)));
    assert!(Arc::ptr_eq(
        &first,
        &presentation.player_preview_gear.held.as_ref().unwrap().rgba
    ));
    install(&mut presentation, vec![sword(0, [220, 80, 12])]);
    presentation.set_player_preview_gear([None; 4], Some((ITEM, 0)));
    let changed = &presentation.player_preview_gear.held.as_ref().unwrap().rgba;
    assert!(!Arc::ptr_eq(&first, changed));
    assert_eq!(&changed[..3], [220, 80, 12]);
}

/// Original checkerboard alpha maximizes isolated extrusion edges at a bounded resolution.
fn dense_sprite(side: u32) -> SessionIcon {
    let rgba8 = (0..side * side)
        .flat_map(|pixel| {
            let alpha = if (pixel % side + pixel / side).is_multiple_of(2) {
                255
            } else {
                0
            };
            [12, 38, 96, alpha]
        })
        .collect::<Vec<_>>()
        .into();
    SessionIcon {
        identifier: ITEM.into(),
        metadata: 0,
        width: side,
        height: side,
        rgba8,
    }
}

#[test]
fn session_preview_dense_hands_preserve_the_player_within_ui_budgets() {
    for side in [192, 128] {
        let mut presentation = presentation();
        install(&mut presentation, vec![dense_sprite(side)]);
        presentation.set_player_preview_gear([None; 4], Some((ITEM, 0)));
        presentation.player_preview_gear.hands[1] =
            presentation.player_preview_gear.hands[0].clone();
        if side == 192 {
            assert!(
                presentation.session_icons.held_sprite(ITEM, 0).is_none(),
                "impossible UI geometry is rejected before retention"
            );
            assert!(presentation.session_icons.held_sprite(ITEM, 0).is_none());
        } else {
            let (_, held) = presentation.session_icons.held_sprite(ITEM, 0).unwrap();
            assert!(
                held.len() <= ui::UiLimits::MAX_UI_VERTICES
                    && held.len() * 2 > ui::UiLimits::MAX_UI_VERTICES
            );
        }
        let mesh = presentation
            .gui_player_mesh()
            .expect("an oversized held mesh must not hide the player");
        assert!(mesh.vertices().len() <= ui::UiLimits::MAX_UI_VERTICES);
        assert_eq!(mesh.batches().len(), if side == 192 { 1 } else { 2 });
        assert_eq!(
            mesh.batches()[0].texture_page,
            (presentation.textures.dynamic_start() + super::super::SKIN_PAGE) as u16
        );
    }
}

#[test]
fn preview_dense_sprite_reserves_surrounding_controls_and_repeated_player_nodes() {
    use ui::{SafeArea, UiNode, UiNodeId, UiPoint, UiRect, UiScale, UiTree, UiVisual};
    for previews in [1, 2] {
        let mut presentation = presentation();
        let mut art = dense_sprite(148);
        let mut cleared = 0;
        for pixel in art.rgba8.as_chunks_mut::<4>().0 {
            if pixel[3] != 0 && cleared < 48 {
                pixel[3] = 0;
                cleared += 1;
            }
        }
        install(&mut presentation, vec![art]);
        presentation.set_player_preview_gear([None; 4], Some((ITEM, 0)));
        let region = IconRef {
            color_mask: None,
            page: u16::MAX,
            uv: [0, 0, 96, 112],
            glint: false,
        };
        presentation.player_preview_icon = Some(region);
        let bounds = UiRect::new(
            UiPoint::new(0.0, 0.0).unwrap(),
            UiPoint::new(96.0, 112.0).unwrap(),
        )
        .unwrap();
        let mut nodes = (0..previews)
            .map(|index| {
                UiNode::new(UiNodeId::new(index + 1), None, bounds).with_visual(UiVisual::Sprite {
                    texture_page: region.page,
                    uv: region.uv,
                    color: [255; 4],
                })
            })
            .collect::<Vec<_>>();
        for index in 0..2 {
            nodes.push(
                UiNode::new(UiNodeId::new(previews + index + 1), None, bounds).with_visual(
                    UiVisual::Solid {
                        texture_page: 0,
                        color: [255; 4],
                    },
                ),
            );
        }
        player_preview::geometry::take_held_projection_work();
        presentation.apply_gui_models(&mut nodes);
        assert_eq!(
            player_preview::geometry::take_held_projection_work(),
            0,
            "frame-budget-rejected held vertices must never be projected"
        );
        for node in &nodes[..previews as usize] {
            let UiVisual::Mesh(mesh) = node.visual() else {
                panic!("the player must retain its model")
            };
            assert_eq!(
                mesh.batches().len(),
                1,
                "only the dense held sprite is omitted"
            );
        }
        assert!(
            nodes[previews as usize..]
                .iter()
                .all(|node| matches!(node.visual(), UiVisual::Solid { .. }))
        );
        let mut tree = UiTree::new(nodes).unwrap();
        tree.layout(bounds, UiScale::default(), SafeArea::ZERO)
            .unwrap();
        assert!(
            tree.build_draw_list().is_ok(),
            "surrounding inventory controls must still publish"
        );
    }
}

#[test]
fn preview_session_mesh_cache_retires_old_items_under_aggregate_storage_pressure() {
    let mut presentation = presentation();
    let source = dense_sprite(16);
    let vertices = held_sprite_vertices(16, 16, &source.rgba8, [0.0, 0.0, 1.0, 1.0]).unwrap();
    let bytes = vertices.len() * std::mem::size_of::<render_model::ActorRigVertex>();
    let count = assets::texture::policy::MAX_SUBSCRIBER_BYTES / bytes + 1;
    let icons = install(
        &mut presentation,
        (0..count)
            .map(|metadata| {
                let mut icon = dense_sprite(16);
                icon.metadata = metadata as u32;
                icon
            })
            .collect(),
    );
    test_time::eventually("admitted original cache-pressure icons", || {
        session_icons::observe(&mut presentation, Some(&icons));
        presentation
            .session_icons
            .sprite(ITEM, (count - 1) as u32)
            .is_some()
    });
    let (_, first) = presentation.session_icons.held_sprite(ITEM, 0).unwrap();
    let old = Arc::downgrade(&first);
    drop(first);
    for metadata in 1..count {
        presentation
            .session_icons
            .held_sprite(ITEM, metadata as u32)
            .unwrap();
    }
    assert!(
        old.upgrade().is_none(),
        "unused source meshes must not accumulate beyond the storage budget"
    );
    let (_, last) = presentation
        .session_icons
        .held_sprite(ITEM, (count - 1) as u32)
        .unwrap();
    let (_, again) = presentation
        .session_icons
        .held_sprite(ITEM, (count - 1) as u32)
        .unwrap();
    assert!(
        Arc::ptr_eq(&last, &again),
        "the selected source still reuses its extrusion"
    );
}

#[test]
fn preview_batch_admission_includes_surrounding_sprite_batches() {
    use ui::{
        SafeArea, UiBlendMode, UiLimits, UiMesh, UiMeshBatch, UiMeshVertex, UiNode, UiNodeId,
        UiPoint, UiRect, UiScale, UiTree, UiVisual,
    };
    let mut presentation = presentation();
    install(&mut presentation, vec![sword(0, [220, 80, 12])]);
    presentation.set_player_preview_gear([None; 4], Some((ITEM, 0)));
    let bounds = UiRect::new(
        UiPoint::new(0.0, 0.0).unwrap(),
        UiPoint::new(96.0, 112.0).unwrap(),
    )
    .unwrap();
    let vertices = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]].map(|position| UiMeshVertex {
        position,
        clip_z: 0.5,
        clip_w: 1.0,
        uv: [0.0; 2],
        color: [255; 4],
        model_light: 1.0,
        overlay_color: [0.0; 4],
        style_flags: 0,
        alpha_test: false,
    });
    let mesh = Arc::new(
        UiMesh::new(
            vertices.into(),
            Arc::from([0, 1, 2]),
            Arc::from([UiMeshBatch {
                texture_page: 0,
                index_range: 0..3,
                blend: UiBlendMode::Alpha,
                depth_test: true,
                depth_write: false,
                alpha_cutoff: None,
            }]),
        )
        .unwrap(),
    );
    let count = UiLimits::MAX_DRAW_BATCHES - 2;
    let mut nodes = (0..count)
        .map(|index| {
            UiNode::new(UiNodeId::new(index as u32 + 1), None, bounds)
                .with_visual(UiVisual::Mesh(Arc::clone(&mesh)))
        })
        .collect::<Vec<_>>();
    nodes.push(
        UiNode::new(UiNodeId::new(count as u32 + 1), None, bounds).with_visual(UiVisual::Sprite {
            texture_page: 0,
            uv: [0, 0, 1, 1],
            color: [255; 4],
        }),
    );
    let preview = IconRef {
        color_mask: None,
        page: u16::MAX,
        uv: [0, 0, 96, 112],
        glint: false,
    };
    presentation.player_preview_icon = Some(preview);
    nodes.push(
        UiNode::new(UiNodeId::new(count as u32 + 2), None, bounds).with_visual(UiVisual::Sprite {
            texture_page: preview.page,
            uv: preview.uv,
            color: [255; 4],
        }),
    );
    player_preview::geometry::take_held_projection_work();
    presentation.apply_gui_models(&mut nodes);
    let mut tree = UiTree::new(nodes).unwrap();
    tree.layout(bounds, UiScale::default(), SafeArea::ZERO)
        .unwrap();
    let draw = tree
        .build_draw_list()
        .expect("a held item must not stop a frame at its batch limit");
    assert!(draw.batches.len() <= UiLimits::MAX_DRAW_BATCHES);
    assert_eq!(
        player_preview::geometry::take_held_projection_work(),
        0,
        "rejected held work stays out of projection"
    );
}

#[test]
fn custom_content_preview_uses_authored_mesh_and_matching_sheet() {
    let mut presentation = presentation();
    let mut vertices =
        render_model::textured_cube_vertices(render_model::equipment::blocks::face_rects([
            0.0, 0.0, 1.0, 1.0,
        ]));
    for vertex in &mut vertices {
        vertex.position[0] *= 0.25;
        vertex.position[2] *= 0.25;
    }
    let vertices: Arc<[render_model::ActorRigVertex]> = vertices.into();
    let [width, height] = assets::BLOCK_ITEM_SHEET_SIZE.map(u32::from);
    let icons = Arc::new(SessionIcons {
        icons: vec![sword(0, [220, 30, 40])],
        block_sheets: vec![SessionIcon {
            identifier: Arc::from(ITEM),
            metadata: 0,
            width,
            height,
            rgba8: vec![255; (width * height * 4) as usize].into(),
        }],
        block_models: BTreeMap::from([(Arc::from(ITEM), Arc::clone(&vertices))]),
        ..Default::default()
    });
    session_icons::observe(&mut presentation, Some(&icons));
    presentation.set_player_preview_gear([None; 4], Some((ITEM, 0)));
    let held = presentation.preview_held_models([true; 2])[0]
        .clone()
        .unwrap();
    assert_eq!(&*held.vertices, &*vertices);
    assert_eq!(
        held.source,
        presentation.session_block_sheet(ITEM, 0).unwrap()
    );
    let layers = presentation.session_block_texture_layers(ITEM, 0).unwrap();
    let dropped = presentation
        .session_block_model(ITEM, 0, held.source)
        .unwrap();
    assert_eq!(layers, assets::BlockFace::ALL.len());
    assert_eq!(layers, dropped.texture_layers());
    assert!(
        held.placements
            .iter()
            .all(|placement| matches!(placement, player_preview::PreviewHeldPlacement::Block))
    );
    assert!(
        presentation.session_icons.models.is_empty(),
        "shaped items keep their authored thumbnail instead of drawing as cubes"
    );
}
