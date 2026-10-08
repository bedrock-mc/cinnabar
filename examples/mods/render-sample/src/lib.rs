//! Minimal render mod: a vignette post pass and a pulsing ring at the player's feet.
//! F8 toggles the vignette.

use mod_api::bindings::{
    Guest,
    cinnabar::extension::{
        gameplay, hud, input,
        render::{self, Decal, DecalStyle, PassSpec, Primitives, Rgba, Vector3},
    },
};
use std::cell::Cell;

const EYE_HEIGHT: f32 = 1.62;
const VIGNETTE: &str = "fn effect(uv: vec2<f32>) -> vec3<f32> {
    let edge = length(uv - vec2<f32>(0.5)) * 1.41;
    return scene(uv) * (1.0 - param(0u) * smoothstep(0.5, 1.0, edge));
}";

thread_local! {
    static SECONDS: Cell<f32> = const { Cell::new(0.0) };
    static VIGNETTE_ON: Cell<bool> = const { Cell::new(true) };
}

struct RenderSample;

impl Guest for RenderSample {
    fn init() {
        let spec = PassSpec {
            name: "vignette".into(),
            order: 0,
            source: VIGNETTE.into(),
            depth: false,
        };
        let label = match render::register_pass(&spec) {
            Ok(()) => "Render sample: F8 toggles the vignette".into(),
            Err(error) => format!("Render sample: {error}"),
        };
        let label: String = label
            .chars()
            .filter(|c| !c.is_control())
            .take(120)
            .collect();
        let _ = hud::set_label(&label);
    }

    fn frame() {
        if input::demo_pressed() {
            VIGNETTE_ON.with(|on| on.set(!on.get()));
        }
        let enabled = VIGNETTE_ON.with(Cell::get);
        let _ = render::update_pass("vignette", enabled, &[0.85]);
        let Ok(Some(frame)) = gameplay::read_frame() else {
            return;
        };
        let seconds = SECONDS.with(|s| {
            s.set(s.get() + frame.frame_seconds);
            s.get()
        });
        let ring = Decal {
            center: Vector3 {
                x: frame.eye.x,
                y: frame.eye.y - EYE_HEIGHT,
                z: frame.eye.z,
            },
            radius: 3.0,
            color: Rgba {
                r: 1.0,
                g: 0.25,
                b: 0.1,
                a: 0.9,
            },
            progress: (seconds * 0.6).fract(),
            style: DecalStyle::Telegraph,
        };
        let _ = render::draw(&Primitives {
            decals: vec![ring],
            ribbons: Vec::new(),
            beams: Vec::new(),
            billboards: Vec::new(),
        });
    }
}

mod_api::bindings::export!(RenderSample with_types_in mod_api::bindings);
