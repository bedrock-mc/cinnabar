use super::*;
use glam::{EulerRot, Quat};

fn downloaded_geometry() -> Option<assets::RuntimeEntityAssets> {
    let manifest = include_bytes!("../../../../assets/vanilla-source.json");
    let settings: serde_json::Value = serde_json::from_slice(manifest).unwrap();
    let relative = "models/entity/ender_dragon.geo.json";
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(settings["cache_dir"].as_str().unwrap())
        .join("resource_pack")
        .join(relative);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping dragon membrane fixture: {} is absent",
                path.display()
            );
            return None;
        }
        Err(error) => panic!("read {}: {error}", path.display()),
    };
    let scratch = tempfile::tempdir().unwrap();
    for family in [
        "entity",
        "models/entity",
        "animations",
        "animation_controllers",
        "render_controllers",
        "textures/entity",
    ] {
        std::fs::create_dir_all(scratch.path().join(family)).unwrap();
    }
    std::fs::write(scratch.path().join(relative), bytes).unwrap();
    let compiled = pack_compiler::compile_entity_assets(scratch.path(), manifest).unwrap();
    Some(
        assets::RuntimeEntityAssets::decode(&assets::encode_entity_blob(&compiled).unwrap())
            .unwrap(),
    )
}

#[test]
fn downloaded_dragon_membranes_keep_their_shared_hinge_when_the_parent_turns() {
    let Some(assets) = downloaded_geometry() else {
        return;
    };
    let index = super::super::asset_geometry::find_geometry_index(&assets, "geometry.dragon")
        .unwrap() as usize;
    let model = &assets.geometries()[index];
    let mesh =
        super::super::asset_geometry::entity_geometry(&assets, index, super::super::EntityRigId(0))
            .unwrap();
    let bone = |name| {
        model
            .bones
            .iter()
            .position(|bone| bone.name.as_ref() == name)
            .unwrap()
    };
    let membrane = |index| {
        let vertices = mesh
            .vertices
            .iter()
            .filter(|vertex| vertex.bone_index as usize == index)
            .copied()
            .collect::<Vec<_>>();
        assert_eq!(
            vertices.len(),
            48,
            "one spar and two authored membrane faces"
        );
        let plane = vertices[36..].to_vec();
        assert!(plane.iter().all(|vertex| vertex.back_uv == vertex.uv));
        plane
    };
    for (wing_name, tip_name) in [("wing", "wingtip"), ("wing1", "wingtip1")] {
        let wing = bone(wing_name);
        let tip = bone(tip_name);
        let inner = membrane(wing);
        let outer = membrane(tip);
        let edge = |vertices: &[ActorRigVertex], outer: bool| {
            let x = vertices
                .iter()
                .map(|vertex| vertex.position[0])
                .reduce(|a, b| if outer { a.min(b) } else { a.max(b) })
                .unwrap();
            let z = vertices
                .iter()
                .map(|vertex| vertex.position[2])
                .reduce(f32::min)
                .unwrap();
            Vec3::from_array(
                vertices
                    .iter()
                    .find(|vertex| vertex.position[0] == x && vertex.position[2] == z)
                    .unwrap()
                    .position,
            )
        };
        let inner_edge = edge(&inner, false);
        let outer_edge = edge(&outer, true);
        let shoulder = Vec3::from_array(mesh.bone_pivots[wing]);
        let hinge = Vec3::from_array(mesh.bone_pivots[tip]);
        let base = model.bones[wing].rotation.unwrap().map(|value| value.get());
        for phase in [0.0_f32, 0.7, 2.0, 4.8] {
            let angles = [
                base[0] + phase.sin() * 20.0,
                base[1] + 30.0,
                base[2] + phase.cos() * 50.0,
            ];
            let parent_rotation = Quat::from_euler(
                EulerRot::ZYX,
                angles[2].to_radians(),
                -angles[1].to_radians(),
                -angles[0].to_radians(),
            );
            let tip_rotation = parent_rotation * Quat::from_rotation_z(phase.sin() * 0.6);
            let parent_point = shoulder + parent_rotation * (inner_edge - shoulder);
            let tip_point = shoulder
                + parent_rotation * (hinge - shoulder)
                + tip_rotation * (outer_edge - hinge);
            assert!(
                (parent_point - tip_point).length() < 1e-5,
                "{tip_name} membrane detached at phase {phase}: {parent_point:?} != {tip_point:?}"
            );
            assert!(inner.chunks_exact(3).all(|triangle| {
                let points: [Vec3; 3] = std::array::from_fn(|index| {
                    let vertex = triangle[index];
                    shoulder + parent_rotation * (Vec3::from_array(vertex.position) - shoulder)
                });
                (points[1] - points[0])
                    .cross(points[2] - points[0])
                    .length()
                    > 0.0
            }));
        }
    }
}

#[test]
fn downloaded_dragon_membranes_preserve_the_same_cutout_from_both_sides() {
    let Some(assets) = downloaded_geometry() else {
        return;
    };
    let settings: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../assets/vanilla-source.json")).unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(settings["cache_dir"].as_str().unwrap())
        .join("resource_pack/textures/entity/dragon/dragon.tga");
    if !path.is_file() {
        eprintln!(
            "skipping dragon membrane texture fixture: {} is absent",
            path.display()
        );
        return;
    }
    let texture = image::open(&path).unwrap().to_rgba8();
    let index = super::super::asset_geometry::find_geometry_index(&assets, "geometry.dragon")
        .unwrap() as usize;
    let model = &assets.geometries()[index];
    let mesh =
        super::super::asset_geometry::entity_geometry(&assets, index, super::super::EntityRigId(0))
            .unwrap();
    for name in ["wing", "wingtip", "wing1", "wingtip1"] {
        let bone = model
            .bones
            .iter()
            .position(|bone| bone.name.as_ref() == name)
            .unwrap();
        let vertices = mesh
            .vertices
            .iter()
            .filter(|vertex| vertex.bone_index as usize == bone)
            .copied()
            .collect::<Vec<_>>();
        let plane = &vertices[36..];
        let min: [f32; 2] = [0, 2].map(|axis| {
            plane
                .iter()
                .map(|vertex| vertex.position[axis])
                .reduce(f32::min)
                .unwrap()
        });
        let max: [f32; 2] = [0, 2].map(|axis| {
            plane
                .iter()
                .map(|vertex| vertex.position[axis])
                .reduce(f32::max)
                .unwrap()
        });
        let dimensions = [0, 2].map(|axis| model.bones[bone].cubes[1].size[axis].get() as usize);
        let alpha = |uv: [f32; 2]| {
            let x = (uv[0] * texture.width() as f32).floor() as u32;
            let y = (uv[1] * texture.height() as f32).floor() as u32;
            texture.get_pixel(x.min(texture.width() - 1), y.min(texture.height() - 1))[3]
        };
        let mut mismatches = 0;
        for x in 0..dimensions[0] {
            for z in 0..dimensions[1] {
                let point = [
                    min[0] + (max[0] - min[0]) * (x as f32 + 0.5) / dimensions[0] as f32,
                    min[1] + (max[1] - min[1]) * (z as f32 + 0.5) / dimensions[1] as f32,
                ];
                let uv = sample_membrane_uv(plane, point);
                mismatches += usize::from(alpha(uv[0]) != alpha(uv[1]));
            }
        }
        assert_eq!(
            mismatches, 0,
            "{name} changes its cutout silhouette when seen from behind"
        );
    }
}

fn sample_membrane_uv(vertices: &[ActorRigVertex], point: [f32; 2]) -> [[f32; 2]; 2] {
    assert_eq!(vertices.len(), 12);
    std::array::from_fn(|side| sample_face_uv(&vertices[side * 6..(side + 1) * 6], point))
}

fn sample_face_uv(vertices: &[ActorRigVertex], point: [f32; 2]) -> [f32; 2] {
    for triangle in vertices.chunks_exact(3) {
        let delta = |vertex: &ActorRigVertex| {
            [
                vertex.position[0] - triangle[0].position[0],
                vertex.position[2] - triangle[0].position[2],
            ]
        };
        let [bx, bz] = delta(&triangle[1]);
        let [cx, cz] = delta(&triangle[2]);
        let px = point[0] - triangle[0].position[0];
        let pz = point[1] - triangle[0].position[2];
        let determinant = bx * cz - bz * cx;
        let b = (px * cz - pz * cx) / determinant;
        let c = (bx * pz - bz * px) / determinant;
        let weights = [1.0 - b - c, b, c];
        if weights.iter().any(|weight| *weight < -1e-5) {
            continue;
        }
        return std::array::from_fn(|axis| {
            triangle
                .iter()
                .zip(weights)
                .map(|(vertex, weight)| vertex.uv[axis] * weight)
                .sum()
        });
    }
    panic!("membrane sample falls outside both triangles: {point:?}");
}
