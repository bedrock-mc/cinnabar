//! Formatting palette regressions through the presentation pipeline.

use super::tests::mini_engine_presentation;

// Zeqa recolors §2 from dark green to yellow through the UI pack globals.
#[test]
fn server_pack_formatting_colors_reach_text_vertices() {
    let mut presentation = mini_engine_presentation();
    presentation.set_server_ui_pack(&super::ServerUiPack {
        ui_layers: vec![vec![(
            "ui/_global_variables.json".into(),
            br#"{"$2_color_format":[0.976,0.859,0.427]}"#.to_vec(),
        )]],
        ..Default::default()
    });
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);
    let runtime = super::pack_harness::action_form(&mut player_runtime, "Menu", &["§2A"]);
    let frame = presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    assert!(
        frame
            .vertices
            .iter()
            .any(|vertex| vertex.color[..3] == [249, 219, 109]),
        "pack-defined §2 must reach the draw list, rather than hardcoded dark green"
    );
}

#[test]
fn formatting_palette_follows_pack_replacement_and_removal() {
    let mut presentation = mini_engine_presentation();
    let install = |presentation: &mut super::UiPresentationRuntime, text: &str| {
        presentation.set_server_ui_pack(&super::ServerUiPack {
            ui_layers: vec![vec![(
                "ui/_global_variables.json".into(),
                text.as_bytes().to_vec(),
            )]],
            ..Default::default()
        });
    };
    install(
        &mut presentation,
        r#"{"$2_color_format":[0.976,0.859,0.427],"$material_diamond_color":[0.373,0.926,1],"$coin_color":[0.8,0.7,0.1]}"#,
    );
    let palette = presentation.formatting_palette().unwrap();
    assert_eq!(
        palette.rgb(ui::BedrockColor::DarkGreen),
        Some([249, 219, 109])
    );
    assert_eq!(
        palette.rgb(ui::BedrockColor::MaterialDiamond),
        Some([95, 236, 255])
    );
    assert_eq!(
        palette.rgb(ui::BedrockColor::MinecoinGold),
        Some([204, 179, 26])
    );
    assert_eq!(palette.rgb(ui::BedrockColor::Base), None);
    install(&mut presentation, r#"{"$2_color_format":[0.5,0.25,0,0]}"#);
    assert_eq!(
        presentation
            .formatting_palette()
            .unwrap()
            .rgb(ui::BedrockColor::DarkGreen),
        Some([128, 64, 0])
    );
    install(&mut presentation, r#"{"$2_color_format":[1,0]}"#);
    assert_eq!(
        presentation
            .formatting_palette()
            .unwrap()
            .rgb(ui::BedrockColor::DarkGreen),
        Some([255; 3])
    );
    presentation.set_server_ui_pack(&super::ServerUiPack::default());
    assert_eq!(
        presentation
            .formatting_palette()
            .unwrap()
            .rgb(ui::BedrockColor::DarkGreen),
        Some([255; 3])
    );
}

/// Finds installed pack data through this worktree's symlink, without needing it on CI.
fn local(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../.local")
        .join(relative)
}

#[test]
fn installed_vanilla_and_zeqa_palettes() {
    use std::io::Read;
    let vanilla = local(&format!(
        "{}/ui/_global_variables.json",
        assets::vanilla_source().installed_pack_dir("resource_pack")
    ));
    let globals = match std::fs::read_to_string(&vanilla) {
        Ok(globals) => globals,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping installed palette test: missing vanilla fixture {}",
                vanilla.display()
            );
            return;
        }
        Err(error) => panic!(
            "read vanilla palette fixture {}: {error}",
            vanilla.display()
        ),
    };
    let mut catalog = json_ui::Catalog::default();
    catalog.overlay_globals_text(&globals);
    let base = palette(catalog.clone());
    assert_eq!(base.rgb(ui::BedrockColor::Gray), Some([198; 3]));
    assert_eq!(base.rgb(ui::BedrockColor::Blue), Some([68, 127, 255]));
    assert_eq!(
        base.rgb(ui::BedrockColor::MaterialDiamond),
        Some([95, 236, 255])
    );
    let objects_root = local("resource-packs/v1/objects");
    let objects = match std::fs::read_dir(&objects_root) {
        Ok(objects) => objects,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping installed Zeqa UI palette fixture: missing {}",
                objects_root.display()
            );
            return;
        }
        Err(error) => panic!(
            "read installed pack directory {}: {error}",
            objects_root.display()
        ),
    };
    let mut checked = 0;
    for object in objects {
        let path = object
            .unwrap_or_else(|error| {
                panic!(
                    "read installed pack entry under {}: {error}",
                    objects_root.display()
                )
            })
            .path();
        if path
            .extension()
            .is_none_or(|extension| extension != "mcpack")
        {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let Ok(mut archive) = zip::ZipArchive::new(std::io::Cursor::new(&bytes)) else {
            continue;
        };
        let mut manifest = String::new();
        if archive.by_name("manifest.json").is_err() {
            continue;
        }
        archive
            .by_name("manifest.json")
            .unwrap()
            .read_to_string(&mut manifest)
            .unwrap();
        let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        if !manifest["header"]["name"]
            .as_str()
            .is_some_and(|name| name.starts_with("Mineville Zeqa") && name.contains("[UI]"))
        {
            continue;
        }
        let mut globals = String::new();
        archive
            .by_name("ui/_global_variables.json")
            .unwrap()
            .read_to_string(&mut globals)
            .unwrap();
        let mut merged = catalog.clone();
        merged.overlay_globals_text(&globals);
        let zeqa = palette(merged);
        for (color, rgb) in [
            (ui::BedrockColor::DarkBlue, [58, 169, 255]),
            (ui::BedrockColor::DarkGreen, [249, 219, 109]),
            (ui::BedrockColor::DarkRed, [180, 42, 43]),
            (ui::BedrockColor::DarkPurple, [149, 53, 189]),
        ] {
            assert_eq!(zeqa.rgb(color), Some(rgb));
        }
        assert_eq!(
            zeqa.rgb(ui::BedrockColor::MaterialDiamond),
            base.rgb(ui::BedrockColor::MaterialDiamond)
        );
        checked += 1;
    }
    if checked == 0 {
        eprintln!(
            "skipping installed Zeqa UI palette fixture: no matching Mineville Zeqa [UI] mcpack under {}",
            objects_root.display()
        );
    } else {
        eprintln!("checked {checked} installed Zeqa UI palettes");
    }
}

/// Publishes a catalog through the same engine installation used by pack reloads.
fn palette(catalog: json_ui::Catalog) -> ui::FormattingPalette {
    let mut presentation = mini_engine_presentation();
    presentation.set_server_ui_pack(&super::ServerUiPack {
        catalog: Some(std::sync::Arc::new(catalog)),
        ..Default::default()
    });
    *presentation.formatting_palette().unwrap()
}
