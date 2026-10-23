use assets::{
    EntityGeometry, EntityGeometryBone, EntityGeometryCube, EntityGeometryScalar, EntityGeometryUv,
};

use super::*;
use crate::icon::model::SIDE;

fn scalar(value: f32) -> EntityGeometryScalar {
    EntityGeometryScalar::new(value).unwrap()
}

fn bone(
    name: &str,
    parent: Option<&str>,
    turn: Option<([f32; 3], [f32; 3])>,
    cubes: &[([f32; 3], [f32; 3], [f32; 2])],
) -> EntityGeometryBone {
    EntityGeometryBone {
        locators: Box::default(),
        name: name.into(),
        binding: None,
        texture_meshes: Box::new([]),
        parent: parent.map(Into::into),
        pivot: turn.map(|(pivot, _)| pivot.map(scalar)),
        rotation: turn.map(|(_, rotation)| rotation.map(scalar)),
        bind_pose_rotation: None,
        mirror: None,
        inflate: None,
        never_render: None,
        reset: None,
        cubes: cubes
            .iter()
            .map(|&(origin, size, uv)| EntityGeometryCube {
                origin: origin.map(scalar),
                size: size.map(scalar),
                pivot: [scalar(0.0); 3],
                rotation: [scalar(0.0); 3],
                uv: EntityGeometryUv::Box(uv.map(scalar)),
                inflate: scalar(0.0),
                mirror: false,
            })
            .collect(),
    }
}

fn geometry(identifier: &str, side: u16, bones: Vec<EntityGeometryBone>) -> EntityGeometry {
    EntityGeometry {
        animation_no_head_bob: false,
        visible_bounds: None,
        identifier: identifier.into(),
        inherits: None,
        source_index: 0,
        texture_width: side,
        texture_height: side,
        bones: bones.into(),
    }
}

/// Shapes after the vanilla head trees: a long-snouted dragon head with a jaw bone, and a
/// wide piglin head with a turned ear; each mob's body stays out of its head.
fn mob_geometries() -> [EntityGeometry; 2] {
    [
        geometry(
            "geometry.dragon",
            256,
            vec![
                bone(
                    "head",
                    None,
                    None,
                    &[
                        ([-6.0, 20.0, -24.0], [12.0, 5.0, 16.0], [176.0, 44.0]),
                        ([-8.0, 16.0, -10.0], [16.0; 3], [112.0, 30.0]),
                    ],
                ),
                bone(
                    "jaw",
                    Some("head"),
                    Some(([0.0, 20.0, -8.0], [0.0; 3])),
                    &[([-6.0, 16.0, -24.0], [12.0, 4.0, 16.0], [176.0, 65.0])],
                ),
                bone(
                    "body",
                    None,
                    None,
                    &[([-12.0, -40.0, -8.0], [24.0, 24.0, 64.0], [0.0, 0.0])],
                ),
            ],
        ),
        geometry(
            "geometry.piglin",
            64,
            vec![
                bone(
                    "body",
                    None,
                    None,
                    &[([-4.0, 12.0, -2.0], [8.0, 12.0, 4.0], [16.0, 16.0])],
                ),
                bone(
                    "head",
                    Some("body"),
                    Some(([0.0, 24.0, 0.0], [0.0; 3])),
                    &[([-5.0, 24.0, -4.0], [10.0, 8.0, 8.0], [0.0, 0.0])],
                ),
                bone(
                    "leftear",
                    Some("head"),
                    Some(([5.0, 30.0, 0.0], [0.0, 0.0, -30.0])),
                    &[([4.0, 25.0, -2.0], [1.0, 5.0, 4.0], [51.0, 6.0])],
                ),
            ],
        ),
    ]
}

#[test]
fn dragon_and_piglin_heads_draw_their_mob_head_facing_the_south_side() {
    let root = tempfile::tempdir().unwrap();
    // Base colors per mob and a pure red face on each head's main box.
    for (path, side, color, face) in [
        (
            "textures/entity/dragon/dragon.png",
            256,
            [60, 30, 70, 255],
            [128..144, 46..62],
        ),
        (
            "textures/entity/piglin/piglin.png",
            64,
            [230, 160, 150, 255],
            [8..18, 8..16],
        ),
    ] {
        let mut texture = image::RgbaImage::from_pixel(side, side, image::Rgba(color));
        for (x, y, pixel) in texture.enumerate_pixels_mut() {
            if face[0].contains(&x) && face[1].contains(&y) {
                *pixel = image::Rgba([255, 0, 0, 255]);
            }
        }
        let file = root.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        texture.save(file).unwrap();
    }
    let geometries = mob_geometries();
    // GUI faces are Up, South and West; an unlit red texel keeps only the south brightness.
    let south = (255.0 * assets::gui_item::CUBE_FACES[1].3).round() as u8;
    let mut icons = Vec::new();
    for name in ["minecraft:dragon_head", "minecraft:piglin_head"] {
        let icon = raster(root.path(), name, 0, &geometries)
            .unwrap()
            .unwrap_or_else(|| panic!("{name} draws its mob geometry's head"));
        let pixels = icon.rgba8.as_chunks::<4>().0;
        let covered = pixels.iter().filter(|pixel| pixel[3] != 0).count();
        assert!(covered > SIDE * SIDE / 4, "{name}: {covered} pixels");
        let face: Vec<usize> = (0..pixels.len())
            .filter(|&index| matches!(pixels[index], [red, 0, 0, 255] if red > 0))
            .collect();
        assert!(face.len() > 8, "{name}: its face is drawn");
        assert!(
            face.iter().all(|&index| pixels[index][0] == south),
            "{name}: the face is lit as a south face, like the other heads"
        );
        let edges = (0..SIDE).flat_map(|row| [row * SIDE, row * SIDE + SIDE - 1]);
        assert!(
            edges.into_iter().all(|index| pixels[index][3] == 0),
            "{name}: the whole head fits the slot"
        );
        icons.push(icon);
    }
    assert_ne!(icons[0], icons[1]);
    assert!(
        raster(root.path(), "minecraft:dragon_head", 0, &[])
            .unwrap()
            .is_none(),
        "a pack without the dragon geometry leaves its icon unresolved"
    );
}

#[test]
fn shulker_inventory_applies_each_face_brightness_once() {
    let root = tempfile::tempdir().unwrap();
    let texture = root
        .path()
        .join("textures/entity/shulker/shulker_white.png");
    std::fs::create_dir_all(texture.parent().unwrap()).unwrap();
    image::RgbaImage::from_pixel(64, 64, image::Rgba([255, 255, 255, 255]))
        .save(texture)
        .unwrap();
    let icon = raster(root.path(), "minecraft:white_shulker_box", 0, &[])
        .unwrap()
        .unwrap();
    let shown: std::collections::BTreeSet<_> = icon
        .rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[3] != 0)
        .map(|pixel| pixel[0])
        .collect();
    let expected = assets::gui_item::CUBE_FACES
        .map(|face| (255.0 * face.3).round() as u8)
        .into_iter()
        .collect();
    assert_eq!(shown, expected);
}
