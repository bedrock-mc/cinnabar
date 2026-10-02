#[path = "support/shader_source.rs"]
mod shader_source;
// Every shader validates, not just parses: naga's parser accepts colliding varying locations
// and reserved identifiers that fail pipeline creation at runtime and silently skip the pass.
use render as ui;

#[path = "../src/nametag.rs"]
#[allow(
    dead_code,
    reason = "shader adapter shares record definitions; depth state is tested by pipeline tests"
)]
pub mod nametag;
#[path = "../src/nametag_render/shader.rs"]
mod nametag_shader;
#[path = "../src/ui_render/shader.rs"]
mod ui_shader;

/// Resolve the vanilla shader for standalone validation.
fn standalone(source: &str) -> String {
    shader_source::standalone(source, &[])
}

#[test]
fn every_shader_parses_and_validates() {
    let mut failures = Vec::new();
    let mut validated = 0;
    for entry in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/src")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !name.ends_with(".wgsl")
            || name == "lighting.wgsl"
            || name == "biome_tint.wgsl"
            || name == "material.wgsl"
        {
            continue;
        }
        let raw = std::fs::read_to_string(&path).unwrap();
        let source = if name == "ui.wgsl" {
            // Exercise the exact production constructor, including its renderer-owned style
            // constant injection, without publishing a test-only runtime API.
            let shader = ui_shader::from_wgsl(&raw, path.to_string_lossy());
            let bevy::shader::Source::Wgsl(source) = shader.source else {
                panic!("UI shader constructor must produce WGSL");
            };
            standalone(&source)
        } else if name == "nametag.wgsl" {
            let shader = nametag_shader::from_wgsl(&raw, path.to_string_lossy());
            let bevy::shader::Source::Wgsl(source) = shader.source else {
                panic!("nametag shader constructor must produce WGSL");
            };
            // Validate the tested-glyph specialization; Bevy preprocesses this define at runtime.
            shader_source::standalone(&source, &["NAMETAG_ALPHA_TEST"])
        } else {
            standalone(&raw)
        };
        match naga::front::wgsl::parse_str(&source) {
            Err(error) => failures.push(format!("{name}: {error}")),
            Ok(module) => {
                if name == "ui.wgsl" {
                    let viewport_bytes = module.types.iter().find_map(|(_, ty)| {
                        if ty.name.as_deref() == Some("UiViewport")
                            && let naga::TypeInner::Struct { span, .. } = ty.inner
                        {
                            Some(span as usize)
                        } else {
                            None
                        }
                    });
                    assert_eq!(
                        viewport_bytes,
                        Some(std::mem::size_of::<ui_shader::UiViewportUniform>())
                    );
                }
                if let Err(error) = naga::valid::Validator::new(
                    naga::valid::ValidationFlags::all(),
                    naga::valid::Capabilities::all(),
                )
                .validate(&module)
                {
                    failures.push(format!("{name}: {error:?}"));
                }
            }
        }
        validated += 1;
    }
    assert!(validated >= 10, "shader sources were not found");
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn review_render_world_actor_fragments_read_shared_fog() {
    for raw in [
        include_str!("../src/actor.wgsl"),
        include_str!("../src/dropped_item.wgsl"),
    ] {
        let module = naga::front::wgsl::parse_str(&standalone(raw)).unwrap();
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
        let fog = module
            .global_variables
            .iter()
            .find(|(_, global)| global.name.as_deref() == Some("world_atmosphere"))
            .expect("world fragments need the shared atmosphere uniform")
            .0;
        let fragment = module
            .entry_points
            .iter()
            .position(|entry| entry.stage == naga::ShaderStage::Fragment)
            .unwrap();
        assert!(!info.get_entry_point(fragment)[fog].is_empty());
    }
}
