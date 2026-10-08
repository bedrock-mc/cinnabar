use super::*;
use std::io::{Cursor, Write};
use zip::{ZipWriter, write::SimpleFileOptions};

/// Creates only synthetic pack content for environment subscriber tests.
fn view(files: &[(&str, &[u8])]) -> LayeredPackView {
    let id = "00000000-0000-0000-0000-000000000011".parse().unwrap();
    let manifest = format!(
        r#"{{"format_version":2,"header":{{"uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":"resources"}}]}}"#
    );
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, bytes) in
        std::iter::once(("manifest.json", manifest.as_bytes())).chain(files.iter().copied())
    {
        writer
            .start_file(path, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    let archive = protocol::ResourcePackArchive::unencrypted(
        id,
        "1.0.0".into(),
        String::new(),
        writer.finish().unwrap().into_inner(),
    );
    LayeredPackView::new(resource_pack::validate_handoff(
        protocol::ResourcePackHandoff::from_archives(vec![archive]),
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
