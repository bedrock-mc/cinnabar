//! Offline software witness over real carrier textures and the production chunk mesher.

use super::{
    pack_reload::PackReload,
    pack_reload_tests::{app_with_assets, settle, stack},
};
use crate::runtime::world::ClientWorld;
use assets::{BlockFace, NetworkIdMode, RuntimeAssets};
use meshing::{Face, PackedQuad};
use std::{io::Cursor, path::Path, sync::Arc};

const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;

#[test]
#[ignore = "offline PNG evidence; needs CINNABAR_RELOAD_WORLD and CINNABAR_RELOAD_OUTPUT"]
fn live_reload_world_software_witness() {
    let _sounds = crate::audio::SERVER_SOUNDS_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let carrier = std::env::var_os("CINNABAR_RELOAD_WORLD").expect("world carrier path");
    let out = std::env::var_os("CINNABAR_RELOAD_OUTPUT").expect("PNG output directory");
    let base = Arc::new(RuntimeAssets::decode(&std::fs::read(carrier).unwrap()).unwrap());
    let (id, visual) = (0..base.visual_count() as u32)
        .map(|id| (id, base.resolve(NetworkIdMode::Sequential, id)))
        .find(|(_, visual)| {
            visual.kind() == assets::VisualKind::Cube
                && visual.face(BlockFace::Up).material_id() != 0
        })
        .unwrap();
    let keys = assets::MaterialKeys::from_entries(
        BlockFace::ALL.map(|face| (visual.face(face).material_id(), "reload_witness")),
    );
    super::resource_packs::set_base_material_keys(keys);
    let air = base.air_network_id(NetworkIdMode::Sequential).unwrap();
    let terrain = terrain(air, id);
    let mut app = app_with_assets(base);
    let before = draw(&app, air, &terrain);
    let texture = image::RgbaImage::from_fn(16, 16, |x, y| {
        if (x / 4 + y / 4) % 2 == 0 {
            image::Rgba([40, 180, 220, 255])
        } else {
            image::Rgba([250, 205, 40, 255])
        }
    });
    let mut png = Cursor::new(Vec::new());
    texture.write_to(&mut png, image::ImageFormat::Png).unwrap();
    app.world_mut()
        .resource_mut::<PackReload>()
        .request_globals(stack(&[
        (
            "textures/terrain_texture.json",
            br#"{"texture_data":{"reload_witness":{"textures":"textures/blocks/reload_witness"}}}"#,
        ),
        ("textures/blocks/reload_witness.png", &png.into_inner()),
    ]));
    let (elapsed, peak) = settle(&mut app, 1);
    let after = draw(&app, air, &terrain);
    assert_ne!(
        before.as_raw(),
        after.as_raw(),
        "live stack must change the real mesher's sampled texture"
    );
    let changed = Arc::downgrade(
        app.world()
            .resource::<ClientWorld>()
            .stream
            .as_ref()
            .unwrap()
            .runtime_assets(),
    );
    app.world_mut()
        .resource_mut::<PackReload>()
        .request_globals(stack(&[]));
    settle(&mut app, 2);
    let removed = draw(&app, air, &terrain);
    assert_eq!(before, removed, "removal restores exact carrier pixels");
    assert!(
        changed.upgrade().is_none(),
        "old CPU atlas ownership is released"
    );
    std::fs::create_dir_all(&out).unwrap();
    for (name, image) in [
        ("world-before.png", before),
        ("world-live-applied.png", after),
        ("world-removed.png", removed),
    ] {
        image.save(Path::new(&out).join(name)).unwrap();
    }
    eprintln!(
        "WORLD_RELOAD_WITNESS swap_ms={:.3} peak_cpu_update_ms={:.3}; production mesher, software projection, no native GPU acceptance",
        elapsed.as_secs_f64() * 1000.0,
        peak.as_secs_f64() * 1000.0
    );
}

/// Encodes an original stepped-block fixture using the Bedrock subchunk framing.
fn terrain(air: u32, block: u32) -> world::SubChunk {
    let mut words = [0u32; 128];
    for x in 2..14usize {
        for z in 2..14usize {
            for y in 0..(1 + usize::from((5..11).contains(&x) && (5..11).contains(&z)) * 3) {
                let at = (x << 8) | (z << 4) | y;
                words[at / 32] |= 1 << (at % 32);
            }
        }
    }
    let mut bytes = vec![9, 1, 0, 3];
    for word in words {
        bytes.extend(word.to_le_bytes());
    }
    varint(&mut bytes, 4);
    varint(&mut bytes, air << 1);
    varint(&mut bytes, block << 1);
    world::SubChunk::decode(&bytes, &world::RawBlockIds { air })
}

/// Writes an unsigned varint; palette values above were zig-zag encoded already.
fn varint(bytes: &mut Vec<u8>, mut value: u32) {
    loop {
        let more = value >= 128;
        bytes.push((value as u8 & 127) | if more { 128 } else { 0 });
        value >>= 7;
        if !more {
            break;
        }
    }
}

/// Renders mesh-emitted quads using the current live stream's texture pages.
fn draw(app: &bevy::prelude::App, air: u32, chunk: &world::SubChunk) -> image::RgbaImage {
    let world = app.world().resource::<ClientWorld>();
    let assets = world.stream.as_ref().unwrap().runtime_assets();
    let mesh = meshing::mesh_sub_chunk(
        &meshing::BlockClassifier::new(air),
        assets,
        NetworkIdMode::Sequential,
        &meshing::Neighbourhood::default(),
        chunk,
    );
    assert!(mesh.quad_count() > 0);
    let mut image = image::RgbaImage::from_pixel(WIDTH, HEIGHT, image::Rgba([32, 45, 58, 255]));
    let mut depth = vec![f32::NEG_INFINITY; (WIDTH * HEIGHT) as usize];
    for quad in mesh.quads() {
        draw_quad(&mut image, &mut depth, assets, quad);
    }
    image
}

/// Projects positive faces from a fixed orthographic camera, with a depth buffer.
fn draw_quad(
    image: &mut image::RgbaImage,
    depth: &mut [f32],
    assets: &RuntimeAssets,
    quad: &PackedQuad,
) {
    let (axis, u_axis, v_axis, shade) = match quad.face() {
        Face::PositiveX => (0, 2, 1, 0.75),
        Face::PositiveY => (1, 0, 2, 1.0),
        Face::PositiveZ => (2, 0, 1, 0.55),
        _ => return,
    };
    let mut start = quad.origin().map(f32::from);
    start[axis] += 1.0;
    let mut u = start;
    u[u_axis] += f32::from(quad.width());
    let mut v = start;
    v[v_axis] += f32::from(quad.height());
    let project = |[x, y, z]: [f32; 3]| [320.0 + (x - z) * 19.0, 130.0 + (x + z) * 9.5 - y * 23.0];
    let p = project(start);
    let q = project(u);
    let r = project(v);
    let a = [q[0] - p[0], q[1] - p[1]];
    let b = [r[0] - p[0], r[1] - p[1]];
    let determinant = a[0] * b[1] - a[1] * b[0];
    let material = assets.material(quad.material_id());
    let mip = &assets.texture_pages()[material.texture.page() as usize]
        .texture
        .mips[0];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let d = [x as f32 + 0.5 - p[0], y as f32 + 0.5 - p[1]];
            let u = (d[0] * b[1] - d[1] * b[0]) / determinant;
            let v = (a[0] * d[1] - a[1] * d[0]) / determinant;
            if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
                continue;
            }
            let z = start.iter().sum::<f32>()
                + u * f32::from(quad.width())
                + v * f32::from(quad.height());
            let index = (y * WIDTH + x) as usize;
            if z < depth[index] {
                continue;
            }
            depth[index] = z;
            let tx = ((u * f32::from(quad.width()) * mip.size as f32) as u32) % mip.size;
            let ty = ((v * f32::from(quad.height()) * mip.size as f32) as u32) % mip.size;
            let offset = ((material.texture.layer() * mip.size * mip.size + ty * mip.size + tx) * 4)
                as usize;
            let pixel = &mip.rgba8[offset..offset + 4];
            image.put_pixel(
                x,
                y,
                image::Rgba([
                    (pixel[0] as f32 * shade) as u8,
                    (pixel[1] as f32 * shade) as u8,
                    (pixel[2] as f32 * shade) as u8,
                    255,
                ]),
            );
        }
    }
}
