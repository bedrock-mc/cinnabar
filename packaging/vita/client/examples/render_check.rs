//! Internal native link/render check. This is NOT a Bedrock game or release artifact.
//! Uses the real palette decoder and mesher with diagnostic assets, without a server.

#[cfg(not(target_os = "vita"))]
fn main() {
    panic!("render-check is a Vita-only developer example, not the Cinnabar client");
}

#[cfg(target_os = "vita")]
fn main() {
    use assets::{NetworkIdMode, RuntimeAssets};
    use cinnabar_vita::{
        controls::{SELECT, START, VitaInput},
        memory::Budget,
        native::Renderer,
        terrain::{CubeUpload, QUADS_PER_UPLOAD},
    };
    use meshing::{BlockClassifier, Neighbourhood, mesh_sub_chunk};
    use semantic_input::InputContext;
    use world::{RawBlockIds, SubChunk};

    let _ = std::fs::create_dir_all("ux0:data/cinnabar");
    let _ = std::fs::File::create("ux0:data/cinnabar/render-check.log");
    log(format_args!(
        "Cinnabar native render check; not a game build"
    ));
    std::panic::set_hook(Box::new(|info| log(format_args!("FAIL: {info}"))));
    // Rust's Vita std target does not expose argv. The harness owns this marker.
    let automated = std::path::Path::new("ux0:data/cinnabar/render-check.auto").is_file();
    log(format_args!("automated={automated}"));

    let mut words = [0_u32; 128];
    for x in 0..16 {
        for z in 0..16 {
            for y in 0..=1 + ((x ^ z) & 3) {
                let index = x * 256 + z * 16 + y;
                words[index / 32] |= 1 << (index % 32);
            }
        }
    }
    let mut bytes = vec![9, 1, 0, 3];
    for word in words {
        bytes.extend(word.to_le_bytes());
    }
    bytes.extend([4, 0, 2]); // two-entry runtime palette: air=0, diagnostic block=1
    let sub = SubChunk::decode(&bytes, &RawBlockIds { air: 0 });
    let assets = RuntimeAssets::diagnostic();
    let mesh = mesh_sub_chunk(
        &BlockClassifier::new(0),
        &assets,
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );
    let budget = Budget::default();
    let renderer = Renderer::new(budget.clone()).expect("VitaGL initialization");
    let mut gpu = Vec::new();
    for quads in mesh.cube_quads().chunks(QUADS_PER_UPLOAD) {
        let upload = CubeUpload::new(quads, &budget).expect("bounded CPU upload");
        gpu.push(renderer.upload(&upload).expect("mapped GPU upload"));
    }
    log(format_args!(
        "{} quads; {:?}",
        mesh.quad_count(),
        budget.usage()
    ));
    let mut input = VitaInput::default();
    log(format_args!("input ready"));
    let mut yaw = 0.6_f32;
    let mut previous = renderer.time_us();
    let started = previous;
    let mut frames = 0_u32;
    let mut failed = false;
    let [width, height] = renderer.dimensions();
    let f = 1. / (65_f32.to_radians() / 2.).tan();
    let projection = [
        f / (width as f32 / height as f32),
        0.,
        0.,
        0.,
        0.,
        f,
        0.,
        0.,
        0.,
        0.,
        -1.002,
        -1.,
        0.,
        0.,
        -0.2002,
        0.,
    ];
    loop {
        if automated && frames < 3 {
            log(format_args!("frame {frames}: poll"));
        }
        let now = renderer.time_us();
        let dt = (now.saturating_sub(previous) as f32 / 1_000_000.).min(0.1);
        previous = now;
        let Some(pad) = renderer.poll() else {
            continue;
        };
        if pad.buttons & (START | SELECT) == (START | SELECT) {
            break;
        }
        let frame = input
            .sample(pad, now, InputContext::Gameplay)
            .expect("input sample");
        if automated && frames < 3 {
            log(format_args!("frame {frames}: begin"));
        }
        yaw += frame.actions.look_delta[0] * dt;
        let (s, c) = yaw.sin_cos();
        let view = [
            c, 0., s, 0., 0., 1., 0., 0., -s, 0., c, 0., 0., -7., -30., 1.,
        ];
        renderer.begin(&projection, &view);
        if automated && frames < 3 {
            log(format_args!("frame {frames}: draw"));
        }
        for batch in &gpu {
            batch.draw([-8., 0., -8.], [0.25, 0.7, 0.3, 1.]);
        }
        renderer.present();
        if automated && frames == 59 {
            if let Err(error) = capture(&renderer, &budget) {
                log(format_args!("FAIL: frame readback: {error}"));
                failed = true;
                break;
            }
        }
        if automated && frames < 3 {
            log(format_args!("frame {frames}: presented"));
        }
        frames += 1;
        if frames % 60 == 0 {
            log(format_args!("frames={frames} elapsed_us={}", now - started));
        }
        if automated && frames == 120 {
            log(format_args!(
                "PASS: native render check; game playability untested"
            ));
            break;
        }
    }
    drop(gpu);
    drop(renderer);
    if failed {
        std::process::exit(1);
    }
}

/// Close each record so a guest abort cannot strand a buffered libc stream.
#[cfg(target_os = "vita")]
fn log(message: std::fmt::Arguments<'_>) {
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("ux0:data/cinnabar/render-check.log")
    {
        let _ = writeln!(file, "{message}");
    }
}

/// Save the actual framebuffer for inspection, using one admitted temporary buffer.
#[cfg(target_os = "vita")]
fn capture(
    renderer: &cinnabar_vita::native::Renderer,
    budget: &cinnabar_vita::memory::Budget,
) -> Result<(), String> {
    use cinnabar_vita::memory::Pool;
    use std::io::Write;
    let [width, height] = renderer.dimensions();
    let width = u16::try_from(width).map_err(|e| e.to_string())?;
    let height = u16::try_from(height).map_err(|e| e.to_string())?;
    let bytes = usize::from(width) * usize::from(height) * 4;
    let _lease = budget
        .reserve(Pool::Main, bytes)
        .map_err(|e| format!("{e:?}"))?;
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(bytes).map_err(|e| e.to_string())?;
    let _padding = budget
        .reserve(Pool::Main, pixels.capacity() - bytes)
        .map_err(|e| format!("{e:?}"))?;
    pixels.resize(bytes, 0);
    renderer
        .read_pixels(&mut pixels)
        .map_err(|e| format!("{e:?}"))?;
    let varied = pixels
        .chunks_exact(4)
        .any(|pixel| pixel[..3] != pixels[..3]);
    for rgba in pixels.chunks_exact_mut(4) {
        rgba.swap(0, 2);
    }
    let mut header = [0_u8; 18];
    header[2] = 2; // Uncompressed BGRA, origin bottom-left (matching GL readback).
    header[12..14].copy_from_slice(&width.to_le_bytes());
    header[14..16].copy_from_slice(&height.to_le_bytes());
    header[16] = 32;
    header[17] = 8;
    let mut file =
        std::fs::File::create("ux0:data/cinnabar/render-check.tga").map_err(|e| e.to_string())?;
    file.write_all(&header).map_err(|e| e.to_string())?;
    file.write_all(&pixels).map_err(|e| e.to_string())?;
    if varied {
        Ok(())
    } else {
        Err("the terrain frame has no RGB variation".into())
    }
}
