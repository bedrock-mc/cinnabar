//! Original text artwork with an unavailable primary and an installed backup face.

use json_ui::{Catalog, Context, ViewState, render_bound, resolve};
use ui_engine::engine::{FormEngine, ScreenArt, screen::PaintFrame};

use crate::carriers::Error;

/// Measures and paints the authored backup through the production label paths.
pub(super) fn paint(engine: &FormEngine, frame: &mut PaintFrame<'_>) -> Result<(), Error> {
    let alias = assets::carriers::FONT_TEN
        .font_face
        .unwrap()
        .json_ui_alias
        .unwrap();
    if std::ptr::eq(frame.font, frame.font.font_named(alias)) {
        return Err(Error::Missing(
            "backup font face; make font-ten-assets".into(),
        ));
    }
    let json = serde_json::json!({
        "namespace":"backup_font",
        "screen":{"type":"panel","size":[280,130],"controls":[
            {"title":{"type":"label","text":"Backup font","size":[260,20],"offset":[0,-45]}},
            {"sample":{"type":"label","text":"A backup font keeps\nthe layout in sync.","localize":false,
                "font_type":"unavailable-face","backup_font_type":alias,"size":["default","default"]}}
        ]}
    }).to_string();
    let catalog = Catalog::from_files([
        ("ui/_global_variables.json", b"{}".as_slice()),
        (
            "ui/_ui_defs.json",
            br#"{"ui_defs":["ui/backup_font.json"]}"#.as_slice(),
        ),
        ("ui/backup_font.json", json.as_bytes()),
    ])
    .map_err(|error| error.to_string())?;
    let root = resolve(&catalog, "backup_font.screen", &Context::desktop())
        .control
        .ok_or_else(|| Error::Invalid("backup label did not resolve".into()))?;
    engine
        .draw(ScreenArt::default(), frame, |env, size| {
            Some(render_bound(root, size, env, &ViewState::default()))
        })
        .map_err(|error| error.to_string())?
        .ok_or_else(|| Error::Invalid("backup label did not paint".into()))?;
    Ok(())
}
