use super::*;

fn fixture() -> UiPresentationRuntime {
    fixture_with_container_overlay(false)
}

fn fixture_with_container_overlay(container_overlay: bool) -> UiPresentationRuntime {
    fixture_with_options(container_overlay, false)
}

fn fixture_with_options(container_overlay: bool, title: bool) -> UiPresentationRuntime {
    let page = assets::UiAtlasPage {
        width: 4,
        height: 4,
        rgba8: vec![255; 64].into(),
    };
    let mut files = vec![
        assets::UiFile {
            path: "ui/_global_variables.json".into(),
            bytes: b"{}".to_vec().into(),
        },
        assets::UiFile {
            path: "ui/_ui_defs.json".into(),
            bytes: br#"{"ui_defs":["ui/credits_screen.json"]}"#.to_vec().into(),
        },
        assets::UiFile {
            path: "ui/credits_screen.json".into(),
            bytes: br##"{
          "namespace":"credits", "credits_screen":{"type":"screen","controls":[
            {"content":{"type":"custom","renderer":"credits_renderer","size":["65%","100%"]}},
            {"skip":{"type":"button","size":[96,24],
              "anchor_from":"bottom_right","anchor_to":"bottom_right","offset":[-8,-8],
              "button_mappings":[{"from_button_id":"button.menu_select",
                "to_button_id":"button.menu_exit","mapping_type":"pressed"}],
              "bindings":[{"binding_name":"#skip_button_visible",
                "binding_name_override":"#visible","binding_type":"global"}]}}
          ]}
        }"##
            .to_vec()
            .into(),
        },
    ];
    if container_overlay {
        let file = files
            .iter_mut()
            .find(|file| file.path.as_ref() == "ui/credits_screen.json")
            .unwrap();
        let mut screen: serde_json::Value = serde_json::from_slice(&file.bytes).unwrap();
        screen["credits_screen"]["controls"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"container_overlay": {
                "type": "panel", "size": ["100%", "100%"],
                "bindings": [{"binding_name": "#is_container_screen",
                    "binding_name_override": "#visible"}],
                "controls": [{"tooltip": {"type": "label", "text": "container tooltip",
                    "size": [100, 10]}}]
            }}));
        file.bytes = serde_json::to_vec(&screen).unwrap().into();
    }
    if title {
        let image = image::RgbaImage::from_pixel(600, 100, image::Rgba([20, 160, 240, 255]));
        let mut encoded = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        files.push(assets::UiFile {
            path: "textures/ui/title.png".into(),
            bytes: encoded.into_inner().into(),
        });
    }
    for (path, bytes) in assets::UI_CREDITS_FILES.into_iter().zip([
        b"first PLAYERNAME\n\nsecond".as_slice(),
        b"[]".as_slice(),
        b"last".as_slice(),
    ]) {
        files.push(assets::UiFile {
            path: path.into(),
            bytes: bytes.to_vec().into(),
        });
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let bytes = assets::encode_ui_catalog([1; 32], &[page], &[], &[], &files).unwrap();
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    presentation
        .enable_json_ui(Arc::new(assets::RuntimeUiAssets::decode(&bytes).unwrap()))
        .unwrap();
    presentation
}

fn draws_title_pixel(input: &render_model::UiRenderInput) -> bool {
    input.batches.iter().any(|batch| {
        let page = &input.textures.pages()[batch.texture_page as usize];
        let [width, height] = page.dimensions();
        let first = batch.first_index as usize;
        let end = first + batch.index_count as usize;
        input.indices[first..end].chunks_exact(3).any(|triangle| {
            let point = triangle.iter().fold([0.0; 2], |mut point, index| {
                for (axis, target) in point.iter_mut().enumerate() {
                    *target += input.vertices[*index as usize].position[axis] / 3.0;
                }
                point
            });
            let clip = batch.scissor;
            if point[0] < clip.x as f32
                || point[1] < clip.y as f32
                || point[0] >= (clip.x + clip.width) as f32
                || point[1] >= (clip.y + clip.height) as f32
            {
                return false;
            }
            let uv = triangle.iter().fold([0.0; 2], |mut uv, index| {
                for (target, source) in uv.iter_mut().zip(input.vertices[*index as usize].uv) {
                    *target += source / 3.0;
                }
                uv
            });
            let x = (uv[0].floor() as u32).min(width - 1);
            let y = (uv[1].floor() as u32).min(height - 1);
            let offset = ((y * width + x) * 4) as usize;
            page.pixels()[offset..offset + 4] == [20, 160, 240, 255]
        })
    })
}

#[test]
fn credits_request_and_paint_the_runtime_title_without_a_json_image_control() {
    let player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime.credits_mut().open(41, 7, 0);
    runtime.credits_mut().observe(5_000, false);
    let mut presentation = fixture_with_options(false, true);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        presentation.sync_menu_artwork(Vec::new());
        let input = presentation
            .build(
                &player,
                &runtime,
                5_000,
                [1280, 720],
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        if draws_title_pixel(&input) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "custom credits rendering must request and paint its runtime title"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn credits_logo_and_first_poem_row_have_a_content_separator() {
    let player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime.credits_mut().open(41, 7, 0);
    runtime.credits_mut().observe(10_000, false);
    let mut presentation = fixture_with_options(false, true);
    let viewport = [1280, 720];
    let dpi = DpiScale::new(1.0).unwrap();
    let px = TextMetrics::for_viewport(viewport, dpi, None).scale.get()
        * FONT_DESIGN_PIXEL_TEXELS as f32;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        presentation.sync_menu_artwork(Vec::new());
        let input = presentation
            .build(&player, &runtime, 10_000, viewport, dpi)
            .unwrap();
        if !draws_title_pixel(&input) {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
            continue;
        }
        let mut logo_bottom = f32::NEG_INFINITY;
        let mut first_glyph_top = f32::INFINITY;
        for batch in input.batches.iter() {
            let page = &input.textures.pages()[batch.texture_page as usize];
            let [width, height] = page.dimensions();
            let first = batch.first_index as usize;
            let end = first + batch.index_count as usize;
            for triangle in input.indices[first..end].chunks_exact(3) {
                let uv = triangle.iter().fold([0.0; 2], |mut uv, index| {
                    for (target, source) in uv.iter_mut().zip(input.vertices[*index as usize].uv) {
                        *target += source / 3.0;
                    }
                    uv
                });
                let x = (uv[0].floor() as u32).min(width - 1);
                let y = (uv[1].floor() as u32).min(height - 1);
                let offset = ((y * width + x) * 4) as usize;
                let pixel = &page.pixels()[offset..offset + 4];
                for index in triangle {
                    let y = input.vertices[*index as usize].position[1];
                    if pixel == [20, 160, 240, 255] {
                        logo_bottom = logo_bottom.max(y);
                    } else if pixel == [255; 4] {
                        first_glyph_top = first_glyph_top.min(y);
                    }
                }
            }
        }
        assert!(logo_bottom.is_finite(), "the runtime logo is painted");
        assert!(first_glyph_top.is_finite(), "the first poem row is painted");
        assert!(
            first_glyph_top - logo_bottom >= forms::credits_content::CONTENT_FILE_GAP * px,
            "the logo and poem need their content-file separator: logo bottom {logo_bottom}, first glyph top {first_glyph_top}, GUI scale {px}"
        );
        break;
    }
}

#[test]
fn credits_hide_inherited_container_overlays_without_container_bindings() {
    let player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime.credits_mut().open(41, 7, 0);
    runtime.credits_mut().observe(10_000, false);
    let mut plain = fixture();
    let mut inherited = fixture_with_container_overlay(true);
    let draw = |presentation: &mut UiPresentationRuntime| {
        presentation
            .build(
                &player,
                &runtime,
                10_000,
                [1280, 720],
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap()
    };
    let ordinary = draw(&mut plain);
    let gated = draw(&mut inherited);
    assert!(!ordinary.vertices.is_empty(), "the poem still paints");
    assert_eq!(
        ordinary.vertices.len(),
        gated.vertices.len(),
        "container-only inherited controls must stay hidden on credits"
    );
}

#[test]
fn runtime_credits_scroll_the_painted_text_and_new_screen_retires_scroll_end() {
    let player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime.set_chat_source_name(Arc::from("Reader"));
    runtime.credits_mut().open(41, 7, 0);
    let mut presentation = fixture();
    let build = |presentation: &mut UiPresentationRuntime, runtime: &UiRuntime, now| {
        presentation
            .build(
                &player,
                runtime,
                now,
                [1280, 720],
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap()
    };
    runtime.credits_mut().observe(10_000, false);
    let a = build(&mut presentation, &runtime, 10_000);
    assert!(
        !a.vertices.is_empty(),
        "runtime poem must produce painted glyphs"
    );
    runtime.credits_mut().observe(11_000, false);
    let b = build(&mut presentation, &runtime, 11_000);
    assert_eq!(a.vertices.len(), b.vertices.len());
    assert!(a.vertices[0].position[1] > b.vertices[0].position[1]);
    assert!(!presentation.credits_finished(runtime.session_id(), 7));
    runtime.credits_mut().observe(120_000, false);
    build(&mut presentation, &runtime, 120_000);
    assert!(presentation.credits_finished(runtime.session_id(), 7));
    runtime.credits_mut().skip(120_001);
    runtime.credits_mut().flush(Some(41), |_| Ok(())).unwrap();
    runtime.credits_mut().open(41, 8, 120_002);
    assert!(!presentation.credits_finished(runtime.session_id(), 8));
    build(&mut presentation, &runtime, 120_002);
    assert!(!presentation.credits_finished(runtime.session_id(), 8));
}

#[test]
fn credits_completion_cannot_carry_into_a_new_session_reusing_the_sequence() {
    let player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    let mut presentation = fixture();
    runtime.credits_mut().open(41, 7, 0);
    runtime.credits_mut().observe(120_000, false);
    presentation
        .build(
            &player,
            &runtime,
            120_000,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    assert!(presentation.credits_finished(runtime.session_id(), 7));
    runtime.begin_session(2);
    assert!(runtime.credits_mut().open(72, 7, 120_001));
    assert!(
        !presentation.credits_finished(runtime.session_id(), 7),
        "a new credits screen must not inherit the previous session's completion"
    );
    presentation
        .build(
            &player,
            &runtime,
            120_001,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    assert!(!presentation.credits_finished(runtime.session_id(), 7));
}

#[test]
fn a_new_credits_screen_cannot_use_an_old_skip_hit_region_before_its_first_paint() {
    let player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    let mut presentation = fixture();
    runtime.credits_mut().open(41, 7, 0);
    runtime.credits_mut().select(10_000, false);
    presentation
        .build(
            &player,
            &runtime,
            10_000,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let hit = (0..720)
        .step_by(8)
        .flat_map(|y| (0..1280).step_by(8).map(move |x| [x as f32, y as f32]))
        .find(|point| presentation.credits_skip_contains(runtime.session_id(), 7, *point))
        .expect("the visible Skip control must produce an actionable hit region");
    runtime.credits_mut().skip(10_001);
    runtime.credits_mut().flush(Some(41), |_| Ok(())).unwrap();
    assert!(runtime.credits_mut().open(41, 8, 10_002));
    assert!(
        !presentation.credits_skip_contains(runtime.session_id(), 8, hit),
        "a new screen cannot act on the preceding screen's Skip geometry"
    );
}

#[test]
fn credits_pack_skip_can_be_revealed_again_late_in_the_poem() {
    let Some(carrier) = forms::pack_harness::carrier() else {
        return;
    };
    let player = player_state::PlayerState::new(1);
    let mut runtime = forms::pack_harness::menu_runtime();
    runtime.credits_mut().open(41, 7, 0);
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    presentation.enable_json_ui(carrier).unwrap();
    let paint = |presentation: &mut UiPresentationRuntime, runtime: &UiRuntime, now| {
        presentation
            .build(
                &player,
                runtime,
                now,
                [1280, 720],
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        (0..720)
            .step_by(8)
            .flat_map(|y| (0..1280).step_by(8).map(move |x| [x as f32, y as f32]))
            .any(|point| presentation.credits_skip_contains(runtime.session_id(), 7, point))
    };
    assert!(!paint(&mut presentation, &runtime, 10_000));
    runtime.credits_mut().select(10_000, false);
    assert!(
        paint(&mut presentation, &runtime, 10_000),
        "first selection must produce the authored actionable Skip control"
    );
    let visible_millis = crate::ui_runtime::credits::CREDITS_SKIP_VISIBLE_MILLIS;
    assert!(!paint(&mut presentation, &runtime, 10_000 + visible_millis));
    runtime.credits_mut().select(240_000, false);
    assert!(
        paint(&mut presentation, &runtime, 240_000),
        "selection must recreate Skip after its earlier reveal expired"
    );
    runtime.credits_mut().select(240_001, true);
    assert!(runtime.credits().active().is_none());
    assert!(runtime.credits().owns_input());
    runtime.credits_mut().flush(Some(41), |_| Ok(())).unwrap();
    assert!(!runtime.credits().owns_input());
}
