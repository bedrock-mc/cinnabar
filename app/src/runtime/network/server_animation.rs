//! Packet expression admission, separate from tick-owned playback.

pub(super) fn compile_stop(source: &str, version: i32) -> Option<assets::MolangProgram> {
    match pack_compiler::compile_molang_expression(source) {
        Ok(program) => Some(program),
        Err(error) => {
            bevy::log::warn!(%error, version, "skipped invalid server animation stop expression");
            None
        }
    }
}
