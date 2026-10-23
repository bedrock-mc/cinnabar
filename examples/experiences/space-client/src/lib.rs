//! Original planet and star cube rendered through the shared personal-mod drawing imports.

use experience_sdk::client::{ClientPart, drawing as draw, environment};
use serde::Deserialize;

mod declared {
    experience_sdk::include_declarations!();
}

struct Space;

impl ClientPart for Space {
    fn init() {
        rebuild();
    }
    fn epoch() {
        rebuild();
    }
}

/// Rebuilds session-scoped resources after initialization or a server world change.
fn rebuild() {
    let model: Model =
        serde_json::from_slice(&declared::assets::PLANET.read().expect("signed model"))
            .expect("original model");
    let texture = declared::assets::SURFACE.read().expect("signed texture");
    assert_eq!(texture.len(), 32 * 16 * 4);
    let source = String::from_utf8(declared::assets::SPACE.read().expect("signed shader"))
        .expect("shader UTF-8");
    let material = draw::create_material(&source).expect("drawing material grant");
    let vertices = model
        .vertices
        .into_iter()
        .map(|vertex| {
            let x = ((vertex.uv[0] * 31.) as usize).min(31);
            let y = ((vertex.uv[1] * 15.) as usize).min(15);
            let at = (y * 32 + x) * 4;
            draw::Vertex {
                position: vertex.position.into(),
                uv: vertex.uv.into(),
                color: (
                    f32::from(texture[at]) / 255.,
                    f32::from(texture[at + 1]) / 255.,
                    f32::from(texture[at + 2]) / 255.,
                    1.,
                ),
                data: (0., 0., 0., 0.),
            }
        })
        .collect();
    let planet = draw::create_geometry(&draw::GeometrySpec::Triangles(draw::Mesh {
        vertices,
        indices: model.indices,
    }))
    .expect("planet geometry");
    let sky =
        draw::create_geometry(&draw::GeometrySpec::Triangles(skybox())).expect("sky geometry");
    draw::create(&instance(sky, material, [0.; 3], 40_000., true)).expect("sky cube");
    draw::create(&instance(
        planet,
        material,
        [0., 0., -18_000.],
        6000.,
        false,
    ))
    .expect("celestial planet");
    environment::set_time_override(Some(18_000)).expect("visual time grant");
    environment::set_weather_override(Some(environment::Weather::Clear))
        .expect("visual weather grant");
}

/// Places either mesh in camera space and supplies the eye position in model coordinates.
fn instance(
    geometry: draw::Handle,
    material: draw::Handle,
    translation: [f32; 3],
    scale: f32,
    sky: bool,
) -> draw::Instance {
    draw::Instance {
        geometry,
        material: Some(material),
        params: vec![
            if sky { 1. } else { 0. },
            -translation[0] / scale,
            -translation[1] / scale,
            -translation[2] / scale,
        ],
        anchor: draw::Anchor::Camera,
        transform: draw::Transform {
            translation: translation.into(),
            rotation: (0., 0., 0., 1.),
            scale: (scale, scale, scale),
        },
        tint: (1., 1., 1., 1.),
        depth: draw::Depth::Test,
        blend: draw::Blend::Premultiplied,
        width: 1.,
        width_mode: draw::WidthMode::World,
        join: draw::Join::Bevel,
        cap: draw::Cap::Butt,
    }
}

/// Creates an inward-visible original cube; both sides use the common mesh pipeline.
fn skybox() -> draw::Mesh {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for axis in 0..3 {
        for side in [-1., 1.] {
            let start = vertices.len() as u32;
            for (u, v) in [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)] {
                let mut position = [0.; 3];
                position[axis] = side;
                position[(axis + 1) % 3] = u;
                position[(axis + 2) % 3] = v;
                vertices.push(draw::Vertex {
                    position: position.into(),
                    color: (1., 1., 1., 1.),
                    uv: ((u + 1.) * 0.5, (v + 1.) * 0.5),
                    data: (0., 0., 0., 0.),
                });
            }
            indices.extend([start, start + 1, start + 2, start, start + 2, start + 3]);
        }
    }
    draw::Mesh { vertices, indices }
}

#[derive(Deserialize)]
struct Model {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
}
#[derive(Deserialize)]
struct Vertex {
    position: [f32; 3],
    uv: [f32; 2],
}

experience_sdk::export_client_part!(Space);
