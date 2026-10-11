use super::{pack_reload_tests::stack, resource_packs::PackApplication};
use pack_runtime::reload_diff::{Changes, Dependencies, Subscriber, compile};
use resource_pack::PackAdmission;
use std::io::Cursor;

#[test]
fn aim_highlight_overrides_track_only_their_texture_inputs() {
    let mut png = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(1, 1, image::Rgba([7, 11, 13, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let path = format!("{}.png", render::AIM_ASSIST_TEXTURES[0]);
    let before = stack(&[(path.as_str(), png.get_ref())]);
    let mut dependencies = Dependencies::default();
    let textures = compile(
        Subscriber::AimAssist,
        &before,
        &mut dependencies,
        crate::camera::aim_highlight::prepare_pack_textures,
    );
    assert_eq!(
        textures[0].as_ref().unwrap().rgba.as_ref(),
        [7, 11, 13, 255]
    );
    assert!(textures[1].is_none());
    let previous = PackApplication {
        admission: PackAdmission::Validated(before),
        dependencies,
        aim_assist_textures: textures,
        ..Default::default()
    };
    let unrelated = stack(&[
        (path.as_str(), png.get_ref()),
        ("textures/unrelated.png", b"new"),
    ]);
    assert!(
        !Changes::between(
            &unrelated,
            Some((&previous.admission, &previous.dependencies))
        )
        .aim_assist
    );
    let removed = stack(&[]);
    assert!(
        Changes::between(
            &removed,
            Some((&previous.admission, &previous.dependencies))
        )
        .aim_assist
    );
}
