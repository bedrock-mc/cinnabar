use super::world_fixture::{EYE_HEIGHT, Fixture, KEY, SPAWN};
use cinnabar_vita::{
    controls::{CROSS, L, PadSample, R, SELECT, START, VitaInput},
    memory::Budget,
    native::Renderer,
    player::PlayerController,
};
use semantic_input::{Action, InputContext};
use sim::{PaletteWorld, PlayerState};
use std::{io::Write, time::Duration};

#[path = "world_probe_capture.rs"]
mod capture;
#[path = "world_probe_assets.rs"]
mod world_assets;

pub fn run() {
    std::fs::create_dir_all("ux0:data/cinnabar").unwrap();
    std::panic::set_hook(Box::new(|info| log(format_args!("FAIL: {info}"))));
    let automated = std::path::Path::new("ux0:data/cinnabar/world-check.auto").is_file();
    log(format_args!(
        "Cinnabar interactive integration fixture; not a game build; automated={automated}"
    ));
    let budget = Budget::default();
    let (assets, mut fixture) = world_assets::load(&budget).expect("world carrier");
    let renderer = Renderer::new(budget.clone()).expect("renderer");
    let mut materials = world_assets::Materials::new();
    let mut meshes = materials
        .upload(&renderer, &budget, &fixture, &assets)
        .expect("materials");
    let mut input = VitaInput::default();
    let mut player = PlayerController::new(PlayerState::new(SPAWN));
    let mut previous = renderer.time_us();
    let mut frame = 0_u32;
    let mut paused = false;
    let mut old_buttons = 0;
    let mut edits = [0_u32; 2];
    let mut max_y = SPAWN.y;
    let mut start_image = None;
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
        let now = renderer.time_us();
        // Scripted functional checks use a reproducible simulation clock. Normal
        // interactive play uses measured elapsed time; this test proves no FPS budget.
        let elapsed = Duration::from_micros(if automated {
            16_667
        } else {
            now.saturating_sub(previous)
        });
        previous = now;
        let Some(mut pad) = renderer.poll() else {
            continue;
        };
        if pad.buttons & (START | SELECT) == START | SELECT {
            break;
        }
        if automated {
            pad = scripted_input(frame);
        }
        if pad.buttons & START != 0 && old_buttons & START == 0 {
            paused = !paused;
        }
        old_buttons = pad.buttons;
        let actions = input
            .sample(
                pad,
                now,
                if paused {
                    InputContext::UiFocused
                } else {
                    InputContext::Gameplay
                },
            )
            .unwrap()
            .actions;
        player
            .advance(
                &actions,
                elapsed,
                !paused,
                120.,
                &PaletteWorld::for_movement(&fixture.store, &fixture.registry, KEY.dimension),
            )
            .expect("shared movement");
        max_y = max_y.max(player.state().position.y);
        for (index, action) in [Action::Attack, Action::Use].into_iter().enumerate() {
            if !paused && actions.phases[action as usize].pressed {
                let [pitch, yaw] = player.rotation();
                if fixture.edit(
                    player.eye(EYE_HEIGHT),
                    sim::view_direction(pitch, yaw),
                    index == 1,
                    player.state().position,
                ) {
                    edits[index] += 1;
                    // Complete and release the old mesh before this small fixture's replacement.
                    // Production streaming requires the nonblocking retirement queue.
                    meshes.clear();
                    meshes = materials
                        .upload(&renderer, &budget, &fixture, &assets)
                        .expect("replacement mesh");
                    log(format_args!("edit counts={edits:?}"));
                }
            }
        }
        renderer.begin(&projection, &player.view_matrix(EYE_HEIGHT));
        materials.draw(&meshes);
        renderer.developer_reticle();
        renderer.present();
        if automated && frame == 59 {
            start_image = Some(
                capture::frame(
                    &renderer,
                    &budget,
                    "ux0:data/cinnabar/world-check-before.tga",
                )
                .expect("initial capture"),
            );
        }
        frame += 1;
        if automated && frame % 60 == 0 {
            log(format_args!(
                "frame={frame} feet={:?} edits={edits:?}",
                player.state().position
            ));
        }
        if automated && frame == 300 {
            let image = capture::frame(&renderer, &budget, "ux0:data/cinnabar/world-check.tga")
                .expect("final capture");
            let checkpoint = fixture.checkpoint();
            std::fs::write("ux0:data/cinnabar/world-check.chunk", &checkpoint)
                .expect("checkpoint write");
            let loaded =
                std::fs::read("ux0:data/cinnabar/world-check.chunk").expect("checkpoint read");
            let mut restored = Fixture::with_ids(fixture.air, fixture.solid);
            restored.restore(&loaded);
            assert!(
                fixture
                    .store
                    .sub_chunk_ref(KEY)
                    .unwrap()
                    .blocks_match(restored.store.sub_chunk_ref(KEY).unwrap()),
                "checkpoint differs"
            );
            assert!(player.state().position.z > SPAWN.z + 1., "did not walk");
            assert!(max_y > SPAWN.y + 0.5, "did not jump");
            assert!(edits.iter().all(|n| *n > 0), "break/place failed");
            assert_ne!(start_image, Some(image), "camera frame unchanged");
            log(format_args!(
                "PASS: textured walk/jump/break/place and subchunk checkpoint; full-game playability untested; {:?}",
                budget.usage()
            ));
            break;
        }
    }
}

fn scripted_input(frame: u32) -> PadSample {
    PadSample {
        left_y: if (60..120).contains(&frame) { 0 } else { 128 },
        right_y: if frame < 60 { 180 } else { 128 },
        buttons: match frame {
            125 => CROSS,
            210 => R,
            240 => L,
            _ => 0,
        },
        ..Default::default()
    }
}

fn log(message: std::fmt::Arguments<'_>) {
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("ux0:data/cinnabar/world-check.log")
    {
        let _ = writeln!(file, "{message}");
    }
}
