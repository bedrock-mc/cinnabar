use super::*;
use std::{io::Read, time::Instant};

#[test]
fn prepared_screen_settings_survive_repeated_pack_publication() {
    let mut presentation = crate::test_support::mini_engine_presentation();
    let base = presentation.pack_catalog_base().unwrap();
    let pack = ServerUiPack::default().prepare_catalog(&base);
    presentation.set_server_ui_pack(&pack);
    let settings = presentation.screen_settings();
    presentation.set_server_ui_pack(&pack);
    assert!(
        Arc::ptr_eq(&settings, &presentation.screen_settings()),
        "unchanged worker-prepared settings retain their immutable ownership"
    );
}

#[test]
fn replaced_catalog_rejects_prepared_screen_settings() {
    let mut presentation = crate::test_support::mini_engine_presentation();
    let base = presentation.pack_catalog_base().unwrap();
    let mut pack = (*ServerUiPack::default().prepare_catalog(&base)).clone();
    presentation.set_server_ui_pack(&pack);
    let previous = presentation.screen_settings();
    let mut changed = (**pack.catalog.as_ref().unwrap()).clone();
    let (namespace, name) = json_ui::HUD_SCREEN.split_once('.').unwrap();
    let definition = serde_json::json!({
        "namespace": namespace,
        name: {"type":"screen", "absorbs_input":true}
    });
    let definition = serde_json::to_vec(&definition).unwrap();
    changed.apply_pack([
        (
            "ui/_ui_defs.json",
            br#"{"ui_defs":["ui/policy.json"]}"#.as_slice(),
        ),
        ("ui/policy.json", definition.as_slice()),
    ]);
    pack.catalog = Some(Arc::new(changed));
    presentation.set_server_ui_pack(&pack);
    let current = presentation.screen_settings();
    assert!(!Arc::ptr_eq(&previous, &current));
    assert!(current.get(json_ui::HUD_SCREEN).unwrap().absorbs_input);
}

fn fixture_pack(paths: &str) -> ServerUiPack {
    let archives = std::env::split_paths(paths)
        .map(|path| {
            let bytes = std::fs::read(path).unwrap();
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
            let mut manifest = Vec::new();
            archive
                .by_name("manifest.json")
                .unwrap()
                .read_to_end(&mut manifest)
                .unwrap();
            let manifest: serde_json::Value =
                serde_json::from_slice(&resource_pack::normalize_jsonc(&manifest).unwrap())
                    .unwrap();
            let header = &manifest["header"];
            let id = header["uuid"].as_str().unwrap().parse().unwrap();
            let version = header["version"]
                .as_array()
                .unwrap()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(".");
            protocol::ResourcePackArchive::unencrypted(id, version, String::new(), bytes)
        })
        .collect();
    let stack =
        resource_pack::validate_handoff(protocol::ResourcePackHandoff::from_archives(archives));
    assert!(stack.rejections().is_empty());
    let view = resource_pack::LayeredPackView::new(stack);
    let ui_layers = view
        .layers()
        .map(|layer| {
            layer
                .files_under("ui/")
                .iter()
                .filter(|path| path.ends_with(".json"))
                .filter_map(|path| {
                    Some((path.to_string(), layer.read_file(path).ok()??.into_vec()))
                })
                .collect()
        })
        .collect();
    ServerUiPack {
        ui_layers,
        view: Some(view),
        ..Default::default()
    }
}

#[test]
fn local_server_ui_publication_profile() {
    let (Ok(carrier), Ok(font), Ok(packs)) = (
        std::env::var("CINNABAR_UI_PUBLICATION_CARRIER"),
        std::env::var("CINNABAR_UI_PUBLICATION_FONT"),
        std::env::var("CINNABAR_UI_PUBLICATION_PACKS"),
    ) else {
        eprintln!(
            "missing fixture: CINNABAR_UI_PUBLICATION_CARRIER, CINNABAR_UI_PUBLICATION_FONT and CINNABAR_UI_PUBLICATION_PACKS"
        );
        return;
    };
    let carrier =
        Arc::new(assets::RuntimeUiAssets::decode(&std::fs::read(carrier).unwrap()).unwrap());
    let manifest = assets::canonical_source_manifest_sha256(include_bytes!(
        "../../../../../../assets/cinnangles-sans-source.json"
    ));
    let font = Arc::new(
        assets::RuntimeFontCatalog::decode(&std::fs::read(font).unwrap(), manifest).unwrap(),
    );
    let mut presentation = UiPresentationRuntime::new(font.clone()).unwrap();
    presentation.enable_json_ui(carrier).unwrap();
    let pack = fixture_pack(&packs);
    let base = presentation.pack_catalog_base().unwrap();
    let pack = pack.prepare_catalog(&base);
    let mut cells = Vec::new();
    let view = pack.view.as_ref().unwrap();
    for path in view.list("font/glyph_") {
        let Some(high_byte) = path
            .strip_prefix("font/glyph_")
            .and_then(|path| path.strip_suffix(".png"))
            .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        else {
            continue;
        };
        let Some(image) = view
            .read(path)
            .and_then(|bytes| image::load_from_memory(&bytes).ok())
        else {
            continue;
        };
        let image = image.into_rgba8();
        cells.extend(assets::extract_cells(&assets::GlyphSheet {
            high_byte,
            width: image.width(),
            height: image.height(),
            rgba8: image.into_raw().into(),
        }));
    }
    let named = std::collections::BTreeMap::from([
        ("unicode".into(), cells.clone()),
        ("rune".into(), Vec::new()),
    ]);
    eprintln!(
        "captured glyph cells={} named_fonts={}",
        cells.len(),
        named.len()
    );
    let sheets =
        Arc::new(super::super::session_glyphs::SessionGlyphSheets::with_named(cells, named));
    let context = presentation
        .form_presentation
        .engine
        .as_ref()
        .unwrap()
        .context()
        .clone();
    let mut settings = Vec::new();
    let mut indices = Vec::new();
    let mut publication = Vec::new();
    let mut glyphs = Vec::new();
    for _ in 0..8 {
        let started = Instant::now();
        let _ = ScreenSettingsTable::for_catalog(pack.catalog.as_ref().unwrap(), &context);
        settings.push(started.elapsed());
        let started = Instant::now();
        let _ = server_pack::ServerAtlas::new(
            &pack.textures,
            pack.view.clone(),
            dynamic_textures::SERVER_UI_PAGES,
        );
        indices.push(started.elapsed());
        let started = Instant::now();
        presentation.set_server_ui_pack(&pack);
        publication.push(started.elapsed());
        super::super::session_glyphs::observe(&mut presentation, None);
        let started = Instant::now();
        super::super::session_glyphs::observe(&mut presentation, Some(&sheets));
        glyphs.push(started.elapsed());
    }
    for (name, mut samples) in [
        ("screen_settings", settings),
        ("texture_index", indices),
        ("publication", publication),
        ("glyph_publication", glyphs),
    ] {
        samples.sort();
        eprintln!(
            "local UI {name}: median_ms={:.3} max_ms={:.3}",
            samples[samples.len() / 2].as_secs_f64() * 1_000.0,
            samples.last().unwrap().as_secs_f64() * 1_000.0,
        );
    }
    eprintln!("carrier font glyphs={}", font.glyphs().len());
}
