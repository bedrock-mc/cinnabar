use super::*;

fn strip(size: [u32; 2]) -> Vec<u8> {
    let mut pixels = image::RgbaImage::new(size[0], size[1]);
    for (x, _, pixel) in pixels.enumerate_pixels_mut() {
        *pixel = image::Rgba(if x % 8 < 4 { [255; 4] } else { [0; 4] });
    }
    let mut bytes = std::io::Cursor::new(Vec::new());
    pixels
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

#[test]
fn animation_strip_waits_for_original_texels_instead_of_a_resized_preview() {
    let path = "textures/ui/test_strip";
    let assets = super::super::tests::mini_carrier();
    let mut set = TextureSet::new(0);
    let mut atlas = ServerAtlas::new(&[(format!("{path}.png"), strip([640, 8]))], None, 1);
    atlas.require([path]);
    let textures = Textures {
        assets: &assets,
        set: &set,
        atlas: &atlas,
        images: None,
    };
    assert!(
        textures.sprite(path).is_some(),
        "static images retain their preview"
    );
    assert!(
        textures.animation_sprite(path).is_none(),
        "resizing destroys frame columns"
    );

    set.set_full_res(HashMap::from([(
        path.into(),
        IconRef {
            page: 7,
            uv: [0, 0, 640, 8],
            glint: false,
        },
    )]));
    let textures = Textures {
        assets: &assets,
        set: &set,
        atlas: &atlas,
        images: None,
    };
    assert_eq!(
        textures.animation_sprite(path),
        Some((7, [0.0, 0.0, 640.0, 8.0]))
    );
}

#[test]
fn animation_with_native_atlas_pixels_does_not_wait_for_artwork() {
    let path = "textures/ui/test_strip";
    let assets = super::super::tests::mini_carrier();
    let set = TextureSet::new(0);
    let mut atlas = ServerAtlas::new(&[(format!("{path}.png"), strip([32, 8]))], None, 1);
    atlas.require([path]);
    let textures = Textures {
        assets: &assets,
        set: &set,
        atlas: &atlas,
        images: None,
    };
    assert_eq!(textures.animation_sprite(path), textures.sprite(path));
    assert!(textures.animation_sprite(path).is_some());
}

#[test]
fn animation_larger_than_an_art_page_still_draws_its_settled_artwork() {
    let path = "textures/ui/test_strip";
    let assets = super::super::tests::mini_carrier();
    let mut set = TextureSet::new(0);
    let mut atlas = ServerAtlas::new(&[(format!("{path}.png"), strip([2048, 8]))], None, 1);
    atlas.require([path]);
    set.set_full_res(HashMap::from([(
        path.into(),
        IconRef {
            page: 7,
            uv: [0, 0, 1022, 3],
            glint: false,
        },
    )]));
    let textures = Textures {
        assets: &assets,
        set: &set,
        atlas: &atlas,
        images: None,
    };
    assert_eq!(
        textures.animation_sprite(path),
        Some((7, [0.0, 0.0, 1022.0, 3.0]))
    );
}
