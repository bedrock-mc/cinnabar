//! Reports program failures without losing the first useful diagnostic.

use super::*;

/// Groups the host draws or dispatches, whose diagnostics are warnings.
fn drawn(group: ProgramGroup) -> bool {
    runs(group)
        || matches!(
            group,
            ProgramGroup::Gbuffers | ProgramGroup::Shadow | ProgramGroup::ShadowComposite
        )
}

/// Logs a program's diagnostics; failures of programs this host runs are warnings.
pub(super) fn log_program(pack: &str, program: &ProgramTranslation) {
    let required = program.enabled && drawn(program.program.group);
    for diagnostic in program.all_diagnostics() {
        let location = diagnostic
            .location
            .as_ref()
            .map(|l| format!("{}:{}", l.file, l.line))
            .unwrap_or_default();
        if required {
            bevy::log::warn!(
                target: "cinnabar::shaderpack",
                pack, program = %program.program.name, %location,
                "{:?}: {}", diagnostic.phase, diagnostic.message
            );
        } else {
            bevy::log::debug!(
                target: "cinnabar::shaderpack",
                pack, program = %program.program.name, %location,
                "{:?}: {}", diagnostic.phase, diagnostic.message
            );
        }
    }
}

/// `program: first diagnostic` on one line; the log carries every diagnostic.
pub(super) fn failure_line(program: &ProgramTranslation) -> String {
    let mut line = format!("{}:", program.program.name);
    match program.all_diagnostics().next() {
        Some(diagnostic) => {
            for word in diagnostic.to_string().split_whitespace() {
                line.push(' ');
                line.push_str(word);
            }
        }
        None => line.push_str(" failed to translate"),
    }
    line
}
