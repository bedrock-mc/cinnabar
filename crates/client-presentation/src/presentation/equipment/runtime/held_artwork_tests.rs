//! Resolved fallback rasters own their pixel-derived geometry and Java raster frame.

use super::*;

/// Two item declarations share a controller and geometry, but startup rasters differ.
fn shared_items(pack: bool) -> Vec<(Box<str>, Vec<u8>)> {
    let mut files = held_pack();
    let description = files
        .iter()
        .find(|(path, _)| path.as_ref() == "attachables/held.json")
        .unwrap()
        .1
        .clone();
    files.retain(|(path, _)| !path.starts_with("attachables/") && !path.starts_with("textures/"));
    for (index, item) in ["minecraft:bow", "fixture:other"].into_iter().enumerate() {
        let mut document: serde_json::Value = serde_json::from_slice(&description).unwrap();
        let definition = &mut document["minecraft:attachable"]["description"];
        definition["identifier"] = serde_json::json!(item);
        let texture = if pack {
            "textures/items/shared".to_owned()
        } else {
            format!("textures/items/fallback_{index}")
        };
        definition["textures"]["default"] = serde_json::json!(texture);
        files.push((
            format!("attachables/item_{index}.json").into(),
            serde_json::to_vec(&document).unwrap(),
        ));
        if !pack || index == 0 {
            let width = if index == 0 { 4 } else { 8 };
            let mut image = image::RgbaImage::new(width, 4);
            for pixel in 0..=index as u32 {
                let x = if index == 0 { 0 } else { pixel * (width - 1) };
                image.put_pixel(x, 0, image::Rgba([30 + index as u8, 40, 50, 255]));
            }
            let mut bytes = std::io::Cursor::new(Vec::new());
            image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
            files.push((format!("{texture}.png").into(), bytes.into_inner()));
        }
    }
    files
}

#[test]
fn shared_pack_geometry_keeps_each_items_resolved_fallback_silhouette() {
    let (mut runtime, pages) = startup_runtime(shared_items(false));
    let body = player_body(&mut runtime);
    let owner = owner();
    let names = [Box::from("rightItem")];
    let rig = owner_rig(&owner, &names);
    let (entities, catalog) = pack_catalog(shared_items(true));
    let geometry_index = render_model::find_geometry_index(&entities, "geometry.held").unwrap();
    let (pages, mut locations) =
        pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(&catalog));
    locations.fill(None);
    runtime.set_pack_layer(Some((Arc::clone(&entities), Some(catalog), locations)));
    let (_, base) = pack_catalog(shared_items(false));
    let mut meshes = Vec::new();
    for item in ["minecraft:bow", "fixture:other"] {
        let input = held(item);
        let draw = runtime
            .first_person_attachable(
                &body,
                input.main.as_ref().unwrap(),
                &owner,
                &rig,
                actor_animation::AttachableAnimationInput {
                    first_person: true,
                    ..Default::default()
                },
                None,
            )
            .unwrap();
        assert_eq!(
            selected_pixel(&pages, &draw.presentation)[0],
            if item == "minecraft:bow" { 30 } else { 31 }
        );
        let generated = runtime
            .take_pending_geometries()
            .into_iter()
            .find(|mesh| mesh.id == draw.presentation.submission.input.rig)
            .unwrap();
        let texture = base
            .texture(&base.binding(item).unwrap().texture.identifier)
            .unwrap();
        let expected = render_model::attachable_geometry(
            &entities,
            geometry_index as usize,
            generated.id,
            texture,
        )
        .unwrap();
        assert_eq!(generated.vertices, expected.vertices);
        meshes.push(generated);
    }
    assert_ne!(meshes[0].id, meshes[1].id);
    assert_ne!(meshes[0].vertices, meshes[1].vertices);
}

#[test]
fn java_raster_frame_uses_the_bows_artwork_after_another_item_warms_the_shared_mesh() {
    for pack_definitions in [false, true] {
        let (mut runtime, pages) = startup_runtime(shared_items(false));
        let body = player_body(&mut runtime);
        let owner = owner();
        let names = [Box::from("rightItem")];
        let rig = owner_rig(&owner, &names);
        let mut files = shared_items(true);
        if !pack_definitions {
            files.retain(|(path, _)| !path.starts_with("attachables/"));
        }
        let (entities, catalog) = pack_catalog(files);
        let (_, mut locations) =
            pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(&catalog));
        locations.fill(None);
        runtime.set_pack_layer(Some((Arc::clone(&entities), Some(catalog), locations)));
        let input = held("fixture:other");
        runtime
            .first_person_attachable(
                &body,
                input.main.as_ref().unwrap(),
                &owner,
                &rig,
                actor_animation::AttachableAnimationInput {
                    first_person: true,
                    ..Default::default()
                },
                None,
            )
            .unwrap();
        let bow = held("minecraft:bow");
        let hand = render_model::java_animation::JavaHand {
            swing: 0.0,
            equip: 1.0,
            using: None,
        };
        let selected = runtime
            .first_person_attachable(
                &body,
                bow.main.as_ref().unwrap(),
                &owner,
                &rig,
                actor_animation::AttachableAnimationInput {
                    first_person: true,
                    ..Default::default()
                },
                Some(hand),
            )
            .unwrap();
        assert_eq!(selected.camera_space, !pack_definitions);
        if !pack_definitions {
            let (_, base) = pack_catalog(shared_items(false));
            let texture = base
                .texture(&base.binding("minecraft:bow").unwrap().texture.identifier)
                .unwrap();
            let index = render_model::find_geometry_index(&entities, "geometry.held").unwrap();
            let (frame, _) =
                render_model::attachable_raster_frame(&entities, index as usize, texture).unwrap();
            let expected = render_model::java_animation::first_person_item(
                hand,
                render_model::java_animation::JavaItemMesh::Raster {
                    width: texture.width,
                    height: texture.height,
                },
                false,
            ) * frame.inverse();
            assert!(selected.java_camera.unwrap().abs_diff_eq(expected, 1e-5));
        } else {
            assert!(selected.java_camera.is_none());
        }
        assert_ne!(
            selected.presentation.submission.input.rig,
            runtime
                .first_person_attachable(
                    &body,
                    input.main.as_ref().unwrap(),
                    &owner,
                    &rig,
                    actor_animation::AttachableAnimationInput {
                        first_person: true,
                        ..Default::default()
                    },
                    None
                )
                .unwrap()
                .presentation
                .submission
                .input
                .rig
        );
    }
}
