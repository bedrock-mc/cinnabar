use std::borrow::Cow;

use bevy::{prelude::Shader, shader::ValidateShader};

/// Keeps runtime bounds and loop checks enabled for Cinnabar's GPU programs.
///
/// Bevy's ordinary WGSL constructor opts out of these checks. Vertex pulling
/// must not turn a stale GPU address into an unchecked device-memory access.
pub(crate) fn from_wgsl(source: impl Into<Cow<'static, str>>, path: impl Into<String>) -> Shader {
    let mut shader = Shader::from_wgsl(source, path);
    shader.validate_shader = ValidateShader::Enabled;
    shader
}

/// Both actor and hand shaders pull the same packed Rust instances. Substitute their single
/// layout source before parsing, without disabling the checked WGSL constructor.
pub(crate) fn from_actor_wgsl(
    source: &str,
    path: impl Into<String>,
    words: usize,
    vertex_words: usize,
) -> Shader {
    from_wgsl(
        crate::material_shader::source(source)
            .replace("ACTOR_GPU_INSTANCE_WORDS", &format!("{words}u"))
            .replace("ACTOR_RIG_VERTEX_WORDS", &format!("{vertex_words}u")),
        path,
    )
}

/// Block entities share one packed Rust vertex layout across models and overlays.
pub(crate) fn from_block_entity_wgsl(
    source: &str,
    path: impl Into<String>,
    words: usize,
) -> Shader {
    from_wgsl(
        source.replace("BLOCK_ENTITY_VERTEX_WORDS", &format!("{words}u")),
        path,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};

    #[test]
    fn constructor_preserves_source_and_enables_runtime_checks() {
        let source = "@compute @workgroup_size(1) fn main() {}";
        let shader = from_wgsl(source, "checked.wgsl");
        assert!(matches!(shader.validate_shader, ValidateShader::Enabled));
        assert_eq!(shader.path, "checked.wgsl");
        let bevy::shader::Source::Wgsl(actual) = shader.source else {
            panic!("checked constructor must retain WGSL");
        };
        assert_eq!(actual, source);
    }

    #[test]
    fn actor_constructor_substitutes_the_supplied_layout_and_keeps_checks_enabled() {
        let words = std::mem::size_of::<u64>() / std::mem::size_of::<u32>();
        let shader = from_actor_wgsl(
            "const WORDS: u32 = ACTOR_GPU_INSTANCE_WORDS;",
            "actor.wgsl",
            words,
            words,
        );
        assert!(matches!(shader.validate_shader, ValidateShader::Enabled));
        let bevy::shader::Source::Wgsl(actual) = shader.source else {
            panic!("actor constructor must retain WGSL");
        };
        assert_eq!(actual, format!("const WORDS: u32 = {words}u;"));
    }

    #[test]
    fn custom_shaders_cannot_bypass_checked_constructor() {
        let render_source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
        let wrapper = render_source.join("shader_safety.rs");
        let app_source = render_source.join("../../../app/src");
        let mut pending = vec![render_source, app_source];
        let mut checked_files = 0;
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(&directory).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if entry.file_type().unwrap().is_dir() {
                    pending.push(path);
                } else if path.extension().is_some_and(|extension| extension == "rs")
                    && path != wrapper
                {
                    let source = fs::read_to_string(&path).unwrap();
                    assert!(
                        !source.contains("Shader::from_wgsl"),
                        "{} bypasses the runtime-checked shader constructor",
                        path.display()
                    );
                    checked_files += 1;
                }
            }
        }
        assert!(
            checked_files > 0,
            "no production shader sources were checked"
        );
    }
}
