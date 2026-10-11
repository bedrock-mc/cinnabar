use super::*;
use std::io::{Cursor, Write};
use zip::{ZipWriter, write::SimpleFileOptions};

/// Creates only synthetic pack content for environment subscriber tests.
fn view(files: &[(&str, &[u8])]) -> LayeredPackView {
    stack(&[files])
}

/// Builds a precedence-ordered stack from independently admitted synthetic archives.
fn stack(layers: &[&[(&str, &[u8])]]) -> LayeredPackView {
    let archives = layers.iter().enumerate().map(|(index, files)| {
        let id = format!("00000000-0000-0000-0000-{:012x}", index + 11).parse().unwrap();
        let manifest = format!(r#"{{"format_version":2,"header":{{"uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":"resources"}}]}}"#);
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (path, bytes) in std::iter::once(("manifest.json", manifest.as_bytes())).chain(files.iter().copied()) {
            writer.start_file(path, SimpleFileOptions::default()).unwrap();
            writer.write_all(bytes).unwrap();
        }
        protocol::ResourcePackArchive::unencrypted(id, "1.0.0".into(), String::new(), writer.finish().unwrap().into_inner())
    }).collect();
    LayeredPackView::new(resource_pack::validate_handoff(
        protocol::ResourcePackHandoff::from_archives(archives),
    ))
}

#[test]
fn high_resolution_optional_clouds_cannot_reach_the_fixed_size_mesher() {
    use assets::AtmosphereRole;
    let size = meshing::CLOUD_MASK_SIZE * 4;
    let image = image::RgbaImage::from_pixel(size, size, image::Rgba([255; 4]));
    let mut png = Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png).unwrap();
    let path = "textures/environment/clouds.png";
    let decoded = decode_pack_texture(&view(&[(path, png.get_ref())]), path).unwrap();
    assert!(!supports_texture_dimensions(
        AtmosphereRole::Clouds,
        decoded.width,
        decoded.height
    ));
    assert!(!supports_texture_dimensions(
        AtmosphereRole::Clouds,
        meshing::CLOUD_MASK_SIZE,
        size
    ));
    assert!(supports_texture_dimensions(
        AtmosphereRole::Clouds,
        meshing::CLOUD_MASK_SIZE,
        meshing::CLOUD_MASK_SIZE
    ));
    for role in [AtmosphereRole::Sun, AtmosphereRole::MoonPhases] {
        assert!(supports_texture_dimensions(role, size, size));
    }
}

#[test]
fn fog_overrides_skip_unknown_media_and_nonfinite_or_reversed_distances() {
    let json = serde_json::json!({"minecraft:fog_settings": {"description": {"identifier":"test:fog"}, "distance": {
        "air": {"fog_start": 2, "fog_end": 4, "fog_color":"#112233", "render_distance_type":"fixed"},
        "water": {"fog_start": 9, "fog_end": 4, "fog_color":"#112233", "render_distance_type":"fixed"},
        "future_medium": {"fog_start": 0, "fog_end": 4, "fog_color":"#112233", "render_distance_type":"fixed"}
    }}});
    let fog = fog_profile(&json).unwrap();
    assert_eq!(fog.distances.len(), 1);
    assert_eq!(fog.distances[0].rgb8, 0x112233);
    assert_eq!(fog.distances[0].start(), 2.0);
}

#[test]
fn colormap_apply_and_removal_restore_base_payload() {
    let base = assets::RuntimeAssets::diagnostic();
    let image = image::RgbImage::from_pixel(
        assets::TINT_MAP_SIZE,
        assets::TINT_MAP_SIZE,
        image::Rgb([12, 34, 56]),
    );
    let mut png = Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png).unwrap();
    let map_path = format!(
        "textures/colormap/{}.png",
        assets::TintMapId::Grass.source_name()
    );
    let mut overlay = assets::BlockOverlay::default();
    apply_biome_overlay(&view(&[(&map_path, png.get_ref())]), &base, &mut overlay);
    let changed = base
        .with_block_overlay(base.visual_count() as u32, &overlay)
        .unwrap();
    assert_eq!(&changed.biome_assets().tint_maps_rgb8[..3], &[12, 34, 56]);
    let mut removed = assets::BlockOverlay::default();
    apply_biome_overlay(&view(&[]), &base, &mut removed);
    assert!(removed.biomes.is_none());
    let restored = base
        .with_block_overlay(base.visual_count() as u32, &removed)
        .unwrap();
    assert_eq!(restored.biome_assets(), base.biome_assets());
}

#[test]
fn particle_definitions_reload_and_removal_revert_to_base() {
    let effect = br#"{"particle_effect":{"description":{"identifier":"test:effect","basic_render_parameters":{"material":"particles_alpha","texture":"textures/particle/test"}},"components":{"minecraft:emitter_lifetime_once":{"active_time":1},"minecraft:emitter_rate_instant":{"num_particles":1},"minecraft:particle_lifetime_expression":{"max_lifetime":1},"minecraft:particle_appearance_billboard":{"size":[1,1]}}}}"#;
    let changed = particles::prepare_particles(&view(&[("particles/test.json", effect)]), None);
    assert!(changed.has_effect("test:effect"));
    let removed = particles::prepare_particles(&view(&[]), None);
    assert!(!removed.has_effect("test:effect"));
}

#[test]
fn reloaded_fog_preserves_initial_fields_and_transition_timeline() {
    let json = serde_json::json!({"minecraft:fog_settings": {
        "description": {"identifier":"test:transition"}, "distance": {
            "water": {"fog_start": -0.2, "fog_end": 0.8, "fog_color":"#8899AA",
                "render_distance_type":"render", "transition_fog": {
                    "init_fog": {"fog_start": -1, "fog_end": 4, "fog_color":"#112233",
                        "render_distance_type":"fixed"},
                    "min_percent": 0.1, "mid_seconds": 3, "mid_percent": 0.5, "max_seconds": 8
                }}
        }
    }});
    let fog = fog_profile(&json).unwrap();
    let distance = fog.distances[0];
    assert_eq!(distance.mode, assets::FogDistanceMode::RenderRelative);
    assert_eq!(distance.start(), -0.2);
    let transition = distance.transition.unwrap();
    assert_eq!(transition.mode, assets::FogDistanceMode::Fixed);
    assert_eq!(transition.rgb8, 0x112233);
    let fields = [
        transition.start_bits,
        transition.end_bits,
        transition.min_percent_bits,
        transition.mid_seconds_bits,
        transition.mid_percent_bits,
        transition.max_seconds_bits,
    ];
    assert_eq!(fields.map(f32::from_bits), [-1.0, 4.0, 0.1, 3.0, 0.5, 8.0]);

    for malformed in [
        serde_json::json!({"mid_seconds": 8}),
        serde_json::json!({"min_percent": 2}),
        serde_json::json!({"init_fog": {"transition_fog": {}}}),
    ] {
        let mut invalid = json.clone();
        let target = &mut invalid["minecraft:fog_settings"]["distance"]["water"]["transition_fog"];
        for (key, value) in malformed.as_object().unwrap() {
            if key == "init_fog" {
                target[key]["transition_fog"] = value["transition_fog"].clone();
            } else {
                target[key] = value.clone();
            }
        }
        invalid["minecraft:fog_settings"]["distance"]["air"] = serde_json::json!({
            "fog_start":0, "fog_end":10, "fog_color":"#010203", "render_distance_type":"fixed"
        });
        let retained = fog_profile(&invalid).unwrap();
        assert_eq!(retained.distances.len(), 1);
        assert_eq!(retained.distances[0].medium, assets::FogMedium::Air);
        assert_eq!(retained.distances[0].transition, None);
    }
}

/// Required carrier content for optional pack reload regression coverage.
fn atmosphere_base() -> render::AtmosphereTextureAssets {
    let textures = [
        (
            assets::AtmosphereRole::Sun,
            "textures/environment/sun.png",
            32,
            32,
        ),
        (
            assets::AtmosphereRole::MoonPhases,
            "textures/environment/moon_phases.png",
            128,
            64,
        ),
        (
            assets::AtmosphereRole::Clouds,
            "textures/environment/clouds.png",
            256,
            256,
        ),
    ]
    .map(|(role, path, width, height)| {
        let rgba8 = vec![255; (width * height * 4) as usize];
        assets::AtmosphereTexture {
            role,
            source_path: path.into(),
            source_bytes: 1,
            source_sha256: [1; 32],
            pixels_sha256: Sha256::digest(&rgba8).into(),
            width,
            height,
            rgba8: rgba8.into_boxed_slice(),
        }
    });
    let bytes = assets::encode_atmosphere_blob(&assets::CompiledAtmosphereAssets {
        source_manifest_sha256: [1; 32],
        textures: textures.into(),
        biome_profiles: Box::new([]),
        fog_profiles: Box::new([]),
    })
    .unwrap();
    render::AtmosphereTextureAssets::new(
        Arc::new(assets::RuntimeAtmosphereAssets::decode(&bytes).unwrap()),
        [1; 32],
    )
}

#[test]
fn custom_skybox_reload_admits_faces_and_removal_restores_the_carrier() {
    let base = atmosphere_base();
    let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([12, 34, 56, 255]));
    let mut png = Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png).unwrap();
    let path = format!(
        "{}0.png",
        assets::SkyboxDimension::Overworld.texture_prefix()
    );
    let prepared = prepare_atmosphere(&view(&[(&path, png.get_ref())]), &base);
    assert_ne!(prepared.identity(), base.identity());
    let cube = prepared.skybox(assets::SkyboxDimension::Overworld).unwrap();
    assert_eq!(cube.face_mask(), 1);
    assert_eq!(
        &cube.faces[0].as_ref().unwrap().rgba8[..4],
        &[12, 34, 56, 255]
    );
    let removed = prepare_atmosphere(&view(&[]), &base);
    assert_eq!(removed.identity(), base.identity());
    assert!(removed.skybox(assets::SkyboxDimension::Overworld).is_none());
}

/// Encodes only original pixels for pack subscriber tests.
fn skybox_png(width: u32, height: u32, colour: [u8; 4]) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(width, height, image::Rgba(colour))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

#[test]
fn skybox_faces_follow_pack_priority_and_dimension_namespaces_independently() {
    use assets::SkyboxDimension as Dimension;
    let low = skybox_png(1, 1, [10, 20, 30, 255]);
    let high = skybox_png(1, 1, [40, 50, 60, 255]);
    let overworld0 = format!("{}0.png", Dimension::Overworld.texture_prefix());
    let overworld1 = format!("{}1.png", Dimension::Overworld.texture_prefix());
    let overworld2 = format!("{}2.png", Dimension::Overworld.texture_prefix());
    let end2 = format!("{}2.png", Dimension::End.texture_prefix());
    let nether3 = format!("{}3.png", Dimension::Nether.texture_prefix());
    let packs = stack(&[
        &[
            (&overworld0, &low),
            (&overworld1, &low),
            (&end2, &low),
            (&nether3, &low),
        ],
        &[(&overworld0, &high), (&overworld2, b"malformed PNG")],
    ]);
    let prepared = prepare_atmosphere(&packs, &atmosphere_base());
    let overworld = prepared.skybox(Dimension::Overworld).unwrap();
    assert_eq!(overworld.face_mask(), 3);
    assert_eq!(
        &overworld.faces[0].as_ref().unwrap().rgba8[..4],
        &[40, 50, 60, 255]
    );
    assert_eq!(
        &overworld.faces[1].as_ref().unwrap().rgba8[..4],
        &[10, 20, 30, 255]
    );
    assert_eq!(prepared.skybox(Dimension::End).unwrap().face_mask(), 4);
    assert_eq!(prepared.skybox(Dimension::Nether).unwrap().face_mask(), 8);
}

#[test]
fn skybox_selection_chooses_the_pack_before_the_image_extension() {
    use assets::SkyboxDimension as Dimension;
    let png = skybox_png(1, 1, [10, 20, 30, 255]);
    let mut tga = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(1, 1, image::Rgba([40, 50, 60, 255]))
        .write_to(&mut tga, image::ImageFormat::Tga)
        .unwrap();
    let prefix = Dimension::Overworld.texture_prefix();
    let png_path = format!("{prefix}0.png");
    let tga_path = format!("{prefix}0.tga");
    for packs in [
        stack(&[&[(&png_path, &png)], &[(&tga_path, tga.get_ref())]]),
        view(&[(&png_path, &png), (&tga_path, tga.get_ref())]),
    ] {
        let prepared = prepare_atmosphere(&packs, &atmosphere_base());
        let cube = prepared.skybox(Dimension::Overworld).unwrap();
        assert_eq!(
            &cube.faces[0].as_ref().unwrap().rgba8[..4],
            &[40, 50, 60, 255]
        );
    }
    let malformed = stack(&[&[(&png_path, &png)], &[(&tga_path, b"invalid TGA")]]);
    assert!(
        prepare_atmosphere(&malformed, &atmosphere_base())
            .skybox(Dimension::Overworld)
            .is_none()
    );
}

#[test]
fn oversized_winning_skybox_face_does_not_reveal_a_lower_pack() {
    let path = format!(
        "{}0.png",
        assets::SkyboxDimension::Overworld.texture_prefix()
    );
    let low = skybox_png(1, 1, [10, 20, 30, 255]);
    let oversized = vec![0; resource_pack::MAX_PACK_TEXTURE_BYTES as usize + 1];
    let packs = stack(&[&[(&path, &low)], &[(&path, &oversized)]]);
    let cubes = pack_runtime::pack_skybox::prepare_pack_skyboxes(&packs);
    assert_eq!(
        cubes[assets::SkyboxDimension::Overworld as usize].face_mask(),
        0
    );
}

#[test]
fn skybox_reload_tracks_missing_faces_and_image_shape_changes() {
    let base = atmosphere_base();
    let path = format!(
        "{}0.png",
        assets::SkyboxDimension::Overworld.texture_prefix()
    );
    let tall = skybox_png(1, 4, [1, 2, 3, 255]);
    let square = skybox_png(2, 2, [1, 2, 3, 255]);
    let first = prepare_atmosphere(&view(&[(&path, &tall)]), &base);
    let second = prepare_atmosphere(&view(&[(&path, &square)]), &base);
    assert_ne!(first.identity(), second.identity());
    let tracked = LayeredPackView::tracked(view(&[]).shared_stack());
    prepare_atmosphere(&tracked, &base);
    let missing = format!(
        "{}5.png",
        assets::SkyboxDimension::Overworld.texture_prefix()
    );
    assert!(tracked.dependencies().unwrap().snapshot().iter().any(
        |input| matches!(input, resource_pack::PackDependency::File {path, ..} if path == &missing)
    ));
}

#[test]
fn supplied_skybox_pack_fixture_survives_archive_admission_and_reload() {
    let Some(path) = std::env::var_os("CINNABAR_SKYBOX_PACK_FIXTURE") else {
        eprintln!(
            "skipping supplied_skybox_pack_fixture_survives_archive_admission_and_reload: missing CINNABAR_SKYBOX_PACK_FIXTURE"
        );
        return;
    };
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping supplied_skybox_pack_fixture_survives_archive_admission_and_reload: missing archive fixture"
            );
            return;
        }
        Err(error) => panic!("read skybox archive fixture: {error}"),
    };
    let mut zip = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_reader(zip.by_name("manifest.json").unwrap()).unwrap();
    let id = manifest["header"]["uuid"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let version = manifest["header"]["version"]
        .as_array()
        .unwrap()
        .iter()
        .map(|part| part.as_u64().unwrap().to_string())
        .collect::<Vec<_>>()
        .join(".");
    drop(zip);
    let archive = protocol::ResourcePackArchive::unencrypted(id, version, String::new(), bytes);
    let view = LayeredPackView::new(resource_pack::validate_handoff(
        protocol::ResourcePackHandoff::from_archives(vec![archive]),
    ));
    let prepared = prepare_atmosphere(&view, &atmosphere_base());
    let cube = prepared
        .skybox(assets::SkyboxDimension::Overworld)
        .expect("fixture must supply its Overworld sky");
    assert_eq!(cube.face_mask(), (1 << assets::SKYBOX_FACE_COUNT) - 1);
    for face in &cube.faces {
        let face = face.as_ref().unwrap();
        assert!(face.width > 0 && face.height > 0);
        assert_eq!(face.rgba8.len(), (face.width * face.height * 4) as usize);
    }
}
