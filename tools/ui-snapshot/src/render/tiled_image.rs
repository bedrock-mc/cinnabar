//! An asset-free tiled-image fixture through JSON-UI and the production draw-list builder.

use image::ImageEncoder;
use jsonui_editor::scene::{Session, View};

use crate::carriers::Error;

/// Repeats an original two-pixel checker across the entire capture viewport.
pub(super) fn png(size: [u32; 2]) -> Result<Vec<u8>, Error> {
    let encode = |pixels: &[u8], size: [u32; 2]| {
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(pixels, size[0], size[1], image::ExtendedColorType::Rgba8)
            .map_err(|error| Error::Invalid(error.to_string()))?;
        Ok(png)
    };
    let checker = encode(
        &[
            32, 96, 192, 255, 240, 192, 64, 255, 240, 192, 64, 255, 32, 96, 192, 255,
        ],
        [2, 2],
    )?;
    let mut session = Session::default();
    let layer = session.workspace.add_layer("Tiled image");
    let document = serde_json::json!({
        "namespace": "snapshot",
        "tiled_image": {
            "type": "image", "size": size,
            "anchor_from": "top_left", "anchor_to": "top_left",
            "texture": "textures/checker", "tiled": true
        }
    });
    session.workspace.add_files(
        layer,
        vec![
            (
                json_ui::UI_DEFS_PATH.into(),
                br#"{"ui_defs":["ui/checker.json"]}"#.to_vec(),
            ),
            ("ui/checker.json".into(), document.to_string().into_bytes()),
            ("textures/checker.png".into(), checker),
        ],
        Vec::new(),
    );
    let frame = session.frame(&View {
        reference: "snapshot.tiled_image".into(),
        size,
        gui_scale: Some(1),
        ..Default::default()
    });
    let pixels = session.paint(&frame, 0.0).map_err(Error::Invalid)?;
    encode(&pixels, size)
}
