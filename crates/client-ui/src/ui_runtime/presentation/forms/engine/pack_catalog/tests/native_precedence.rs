use std::sync::Arc;

use assets::{RuntimeUiAssets, UiAtlasPage, UiFile, UiTexturePlacement, encode_ui_catalog};

use crate::ui_runtime::{
    UiRuntime,
    presentation::forms::{ServerUiPack, snapshot},
};

const CARRIER: &str = "textures/ui/priority_carrier";
const ICON: &str = "textures/items/priority_icon";
const BLUE: [u8; 4] = [20, 80, 220, 255];
const GREEN: [u8; 4] = [30, 210, 70, 255];
const RED: [u8; 4] = [220, 30, 40, 255];

/// An original large image that can expose accidental fallback precedence.
fn large_png(color: [u8; 4]) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(2048, 8, image::Rgba(color))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

/// Renders both priority sources at a fixed caller-owned clock.
fn pixels(
    presentation: &mut crate::ui_runtime::presentation::UiPresentationRuntime,
    runtime: &UiRuntime,
) -> image::RgbaImage {
    let input = presentation
        .build(
            &player_state::PlayerState::new(1),
            runtime,
            0,
            [200, 128],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    input.validate().unwrap();
    snapshot::rasterize(&input)
}

#[test]
fn large_local_fallbacks_preserve_carrier_and_icon_pixels_below_server_overrides() {
    let files = vec![
        ("ui/_global_variables.json".to_owned(), b"{}".to_vec()),
        (
            "ui/_ui_defs.json".to_owned(),
            br#"{"ui_defs":["custom/priority.json"]}"#.to_vec(),
        ),
        (
            "custom/priority.json".to_owned(),
            serde_json::to_vec(&serde_json::json!({
                "namespace":"hud",
                "hud_screen":{"type":"screen","controls":[
                    {"carrier":{"type":"image","texture":CARRIER,"keep_ratio":false,
                        "anchor_from":"top_left","anchor_to":"top_left",
                        "offset":[16,16],"size":[64,64]}},
                    {"icon":{"type":"image","texture":ICON,"keep_ratio":false,
                        "anchor_from":"top_left","anchor_to":"top_left",
                        "offset":[104,16],"size":[64,64]}}
                ]}
            }))
            .unwrap(),
        ),
    ];
    let mut carrier_files: Vec<_> = files
        .iter()
        .map(|(path, bytes)| UiFile {
            path: path.clone().into(),
            bytes: bytes.clone().into(),
        })
        .collect();
    carrier_files.sort_by(|a, b| a.path.cmp(&b.path));
    let encoded = encode_ui_catalog(
        [2; 32],
        &[UiAtlasPage {
            width: 4,
            height: 4,
            rgba8: BLUE.repeat(16).into(),
        }],
        &[UiTexturePlacement {
            path: CARRIER.into(),
            page: 0,
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        }],
        &[],
        &carrier_files,
    )
    .unwrap();
    let mut presentation = crate::ui_runtime::presentation::UiPresentationRuntime::new(
        crate::test_support::fixture_font(),
    )
    .unwrap();
    presentation
        .enable_json_ui(Arc::new(RuntimeUiAssets::decode(&encoded).unwrap()))
        .unwrap();
    presentation.gui_scale_preference = Some(1);
    let root = tempfile::tempdir().unwrap();
    for key in [CARRIER, ICON] {
        let path = root.path().join(format!("{key}.png"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, large_png(GREEN)).unwrap();
    }
    presentation
        .engine
        .as_mut()
        .unwrap()
        .textures
        .set_fallbacks(
            [(
                ICON.to_owned(),
                ui::IconRef {
                    page: 0,
                    uv: [0, 0, 1, 1],
                    glint: false,
                },
            )]
            .into(),
            root.path().to_owned(),
        );
    let mut runtime = UiRuntime::new(1);
    let pack = ServerUiPack {
        ui_layers: vec![files],
        ..Default::default()
    };
    runtime.set_server_ui(Some(Arc::new(pack.clone())));
    let first = pixels(&mut presentation, &runtime);
    assert_eq!(
        first.get_pixel(48, 48).0,
        BLUE,
        "the carrier owns its texture"
    );
    assert_eq!(
        first.get_pixel(136, 48).0,
        [255; 4],
        "the item icon owns its texture"
    );

    let mut override_pack = pack.clone();
    override_pack.textures = [CARRIER, ICON]
        .map(|key| (format!("{key}.png"), large_png(RED)))
        .into();
    runtime.set_server_ui(Some(Arc::new(override_pack)));
    test_time::eventually("large server images override both existing atlases", || {
        let rendered = pixels(&mut presentation, &runtime);
        rendered.get_pixel(48, 48).0 == RED && rendered.get_pixel(136, 48).0 == RED
    });
    let mut broken = large_png(RED);
    broken[41] ^= 0xff;
    assert_eq!(
        image::ImageReader::new(std::io::Cursor::new(&broken))
            .with_guessed_format()
            .unwrap()
            .into_dimensions()
            .unwrap(),
        (2048, 8),
    );
    assert!(image::load_from_memory(&broken).is_err());
    let mut broken_pack = pack.clone();
    broken_pack.textures = [CARRIER, ICON]
        .map(|key| (format!("{key}.png"), broken.clone()))
        .into();
    runtime.set_server_ui(Some(Arc::new(broken_pack)));
    test_time::eventually(
        "failed server images reveal carrier and icon pixels",
        || {
            let rendered = pixels(&mut presentation, &runtime);
            rendered.get_pixel(48, 48).0 == BLUE && rendered.get_pixel(136, 48).0 == [255; 4]
        },
    );
    runtime.set_server_ui(Some(Arc::new(pack)));
    let restored = pixels(&mut presentation, &runtime);
    assert_eq!(restored.get_pixel(48, 48).0, BLUE);
    assert_eq!(restored.get_pixel(136, 48).0, [255; 4]);
}
