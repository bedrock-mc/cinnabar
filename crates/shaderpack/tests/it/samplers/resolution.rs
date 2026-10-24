//! Binding precedence across sampler spellings, declarations and released overrides.

use super::*;

#[test]
fn sampler_resolution_follows_declaration_order() {
    for geometry in [false, true] {
        let stage = if geometry { "gbuffers" } else { "composite" };
        let first = if geometry {
            "gbuffers_basic"
        } else {
            "composite"
        };
        for uniform in [
            "texture",
            "gtexture",
            "tex",
            "u_MainSampler",
            "dust",
            "unknown",
            "colortex0",
        ] {
            let normalized = if uniform == "texture" {
                "gtexture"
            } else {
                uniform
            };
            let canonical = if matches!(normalized, "tex" | "u_MainSampler") {
                "gtexture"
            } else {
                normalized
            };
            for declaration in ["none", "image", "raw", "plain"] {
                let properties = match declaration {
                    "image" => format!(
                        "image.scratch = {normalized} rgba rgba8 unsigned_byte false false 4 4\n"
                    ),
                    "raw" => format!(
                        "texture.{stage}.{normalized} = lut.dat TEXTURE_2D RGBA8 1 1 RGBA UNSIGNED_BYTE\n"
                    ),
                    "plain" => format!("texture.{stage}.{canonical} = sample.png\n"),
                    _ => String::new(),
                };
                let fragment = format!(
                    "#version 120\nuniform sampler2D {uniform}; void main() {{ gl_FragData[0] = texture2D({uniform}, vec2(0.5)); }}"
                );
                let files = [
                    (format!("{first}.vsh"), VERTEX.to_owned()),
                    (format!("{first}.fsh"), fragment.clone()),
                    ("composite1.vsh".to_owned(), VERTEX.to_owned()),
                    ("composite1.fsh".to_owned(), fragment),
                    ("shaders.properties".to_owned(), properties),
                ];
                let translated =
                    ShaderPack::new(PackSource::from_files(files)).translate(&Default::default());
                for released in [false, true] {
                    // Geometry has no stage flips; composite1 exercises release after a write.
                    let name = if released && !geometry {
                        "composite1"
                    } else {
                        first
                    };
                    let program = program(&translated, name);
                    fragment_stage(program);
                    assert_eq!(program.released_overrides, u32::from(released && !geometry));
                    let (expected, class) = match declaration {
                        "image" => ("image.scratch".to_owned(), SamplerClass::ClampLinear),
                        "raw" => (
                            format!("texture.{stage}.{normalized}"),
                            SamplerClass::ClampLinear,
                        ),
                        "plain" if !(released && !geometry && canonical == "colortex0") => {
                            (canonical.to_owned(), SamplerClass::RepeatNearest)
                        }
                        _ if geometry && canonical != "colortex0" => {
                            ("gtexture".to_owned(), SamplerClass::RepeatNearest)
                        }
                        _ => ("colortex0".to_owned(), SamplerClass::ClampLinear),
                    };
                    let reflection = reflection(program);
                    assert_eq!(reflection.textures.len(), 1);
                    assert_eq!(
                        reflection.textures[0].name, expected,
                        "{stage}/{uniform}/{declaration}/{released}"
                    );
                    assert_eq!(
                        reflection
                            .samplers
                            .iter()
                            .filter_map(|s| s.class())
                            .collect::<Vec<_>>(),
                        [class],
                        "{stage}/{uniform}/{declaration}/{released}"
                    );
                }
            }
        }
    }
}

#[test]
fn explicit_images_win_over_raw_and_shared_overrides() {
    for uniform in [
        "gtexture",
        "tex",
        "u_MainSampler",
        "colortex0",
        "dust",
        "noisetex",
    ] {
        let fragment = format!(
            "#version 120\nuniform sampler2D {uniform}; void main() {{ gl_FragData[0] = texture2D({uniform}, vec2(0.5)); }}"
        );
        let properties = format!(
            "image.scratch = {uniform} rgba rgba8 unsigned_byte false false 4 4\ntexture.composite.{uniform} = lut.dat TEXTURE_2D RGBA8 1 1 RGBA UNSIGNED_BYTE\ntexture.composite.colortex0 = sample.png\n"
        );
        let translated = pack(
            &fragment,
            &properties,
            &[("composite1.vsh", VERTEX), ("composite1.fsh", &fragment)],
        )
        .translate(&Default::default());
        for name in ["composite", "composite1"] {
            assert_eq!(
                reflection(program(&translated, name)).textures[0].name,
                "image.scratch",
                "{uniform}/{name}"
            );
        }
    }
}

#[test]
fn raw_directives_match_normalized_names_and_shapes_only() {
    for (uniform, property, raw) in [
        ("texture", "gtexture", true),
        ("gcolor", "gtexture", true),
        ("texture", "texture", false),
        ("tex", "gtexture", false),
        ("gtexture", "tex", false),
    ] {
        let fragment = format!(
            "#version 120\nuniform sampler2D {uniform}; void main() {{ gl_FragData[0] = texture2D({uniform}, vec2(0.5)); }}"
        );
        for shape in ["TEXTURE_2D RGBA8 1 1", "TEXTURE_3D RGBA8 1 1 1"] {
            let properties =
                format!("texture.composite.{property} = lut.dat {shape} RGBA UNSIGNED_BYTE\n");
            let translated = pack(&fragment, &properties, &[]).translate(&Default::default());
            let expected = if raw && shape.starts_with("TEXTURE_2D") {
                "texture.composite.gtexture"
            } else {
                "colortex0"
            };
            assert_eq!(
                reflection(program(&translated, "composite")).textures[0].name,
                expected,
                "{uniform}/{property}/{shape}"
            );
        }
    }
}

#[test]
fn core_sampler_declarations_keep_their_exact_names() {
    for uniform in ["texture", "gcolor"] {
        for declaration in ["image", "raw"] {
            let fragment = format!(
                "#version 330 core\nuniform sampler2D {uniform}; out vec4 color; void main() {{ color = texture2D({uniform}, vec2(0.5)); }}"
            );
            let (properties, expected) = if declaration == "image" {
                (
                    format!("image.scratch = {uniform} rgba rgba8 unsigned_byte false false 4 4\n"),
                    "image.scratch".to_owned(),
                )
            } else {
                (
                    format!(
                        "texture.composite.{uniform} = lut.dat TEXTURE_2D RGBA8 1 1 RGBA UNSIGNED_BYTE\n"
                    ),
                    format!("texture.composite.{uniform}"),
                )
            };
            let translated = pack(&fragment, &properties, &[]).translate(&Default::default());
            assert_eq!(
                reflection(program(&translated, "composite")).textures[0].name,
                expected,
                "{uniform}/{declaration}"
            );
        }
    }
}
