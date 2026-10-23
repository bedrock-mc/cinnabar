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
fn copper_golem_statue_inventory_uses_standing_geometry_and_oxidation() {
    let root = tempfile::tempdir().unwrap();
    let geometries = [geometry(
        "geometry.copper_golem",
        64,
        vec![
            bone(
                "body",
                None,
                None,
                &[([-4.0, 0.0, -3.0], [8.0, 12.0, 6.0], [0.0, 0.0])],
            ),
            bone(
                "head",
                Some("body"),
                None,
                &[([-5.0, 12.0, -5.0], [10.0, 8.0, 10.0], [0.0, 0.0])],
            ),
        ],
    )];
    let colors = [
        ("", [180, 90, 30, 255]),
        ("_exposed", [130, 110, 70, 255]),
        ("_weathered", [70, 150, 110, 255]),
        ("_oxidized", [40, 190, 170, 255]),
    ];
    for (suffix, color) in colors {
        let path = root.path().join(format!(
            "textures/entity/copper_golem/copper_golem{suffix}.png"
        ));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        image::RgbaImage::from_pixel(64, 64, image::Rgba(color))
            .save(path)
            .unwrap();
        image::RgbaImage::new(64, 64)
            .save(root.path().join(format!(
                "textures/entity/copper_golem/copper_golem_eyes{suffix}.png"
            )))
            .unwrap();
    }
    let facts = assets::facts::BlockEntityFacts::pinned();
    let mut seen = std::collections::BTreeSet::new();
    for record in assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap()
    .iter()
    {
        if facts
            .by_block_identifier(&record.name)
            .is_none_or(|entry| entry.renderer != assets::facts::BlockEntityRenderer::Statue)
            || !seen.insert(record.name.clone())
        {
            continue;
        }
        let icon = raster(root.path(), &record.name, 0, &geometries)
            .unwrap()
            .unwrap_or_else(|| panic!("{} has an inventory icon", record.name));
        let suffix = if record.name.contains("exposed") {
            "_exposed"
        } else if record.name.contains("weathered") {
            "_weathered"
        } else if record.name.contains("oxidized") {
            "_oxidized"
        } else {
            ""
        };
        let expected = colors.iter().find(|(name, _)| *name == suffix).unwrap().1;
        let pixels: Vec<_> = icon
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] != 0)
            .collect();
        assert!(pixels.len() > SIDE * SIDE / 4);
        assert!(
            pixels.iter().all(|p| **p == expected),
            "{} keeps its oxidation artwork",
            record.name
        );
    }
    assert_eq!(seen.len(), 8);
    image::RgbaImage::from_pixel(64, 64, image::Rgba([255, 0, 0, 255]))
        .save(
            root.path()
                .join("textures/entity/copper_golem/copper_golem_eyes.png"),
        )
        .unwrap();
    let eyes = raster(root.path(), "minecraft:copper_golem_statue", 0, &geometries)
        .unwrap()
        .unwrap();
    assert!(
        eyes.rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] != 0)
            .all(|p| *p == [255, 0, 0, 255]),
        "eye artwork overlays equal-depth body faces"
    );
    assert!(
        raster(root.path(), "minecraft:copper_golem_statue", 0, &[])
            .unwrap()
            .is_none()
    );
}

#[test]
fn lectern_inventory_board_slopes_toward_the_viewer() {
    let parts = lectern();
    let reading_surface = parts
        .iter()
        .find(|part| part.texture == geometry::LECTERN_TEXTURES[2])
        .unwrap();
    assert_eq!(reading_surface.faces.len(), 1);
    let corners = reading_surface.faces[0].0;
    let near_z = corners.iter().map(|p| p[2]).fold(f32::MIN, f32::max);
    let far_z = corners.iter().map(|p| p[2]).fold(f32::MAX, f32::min);
    let height_at = |z: f32| {
        let points: Vec<_> = corners
            .iter()
            .filter(|p| (p[2] - z).abs() < 0.0001)
            .collect();
        assert_eq!(points.len(), 2);
        (points[0][1] + points[1][1]) * 0.5
    };
    assert!(
        height_at(near_z) < height_at(far_z),
        "reading surface faces south"
    );
    assert_eq!(reading_surface.faces[0].1, [0.0, 1.0, 16.0, 13.0]);
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
