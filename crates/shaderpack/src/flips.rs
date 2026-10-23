//! Iris's ping-pong flips of full-screen passes, and the `texture.<stage>.colortexN`
//! bindings an earlier flip in the stage releases.

use crate::buffers::{BufferId, MAX_COLOR_BUFFERS, canonical_sampler_name};
use crate::programs::{ProgramGroup, ProgramKind};
use crate::properties::{FlipRule, ShaderProperties};
use crate::translate::ProgramTranslation;

/// A full-screen pass as flip planning sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlipPass<'a> {
    /// Program name, e.g. `composite3`; a program's `.csh` stage shares its draw's name.
    pub name: &'a str,
    pub group: ProgramGroup,
    pub kind: ProgramKind,
    /// Bit per colour buffer the pass draws into.
    pub writes: u32,
}

/// Bit per colour buffer each of `passes` (full-screen passes in frame order) flips, as Iris
/// does after the pass: every buffer it writes unless `flip.<program>.<buffer>=false`, plus
/// every `flip.<program>.<buffer>=true`. A program's compute dispatches run before its draw,
/// so its rules apply after the draw. `final` writes the screen and flips nothing.
pub fn pass_flips(passes: &[FlipPass<'_>], rules: &[FlipRule]) -> Vec<u32> {
    passes
        .iter()
        .map(|pass| {
            if pass.group == ProgramGroup::Final {
                return 0;
            }
            let finishes_pass = pass.kind == ProgramKind::Render
                || !passes
                    .iter()
                    .any(|next| next.name == pass.name && next.kind == ProgramKind::Render);
            let mut flip = pass.writes;
            for rule in rules
                .iter()
                .filter(|r| finishes_pass && r.program == pass.name)
            {
                if let Some(BufferId::Color(b)) = BufferId::parse(&rule.buffer)
                    && b < MAX_COLOR_BUFFERS
                {
                    if rule.flip {
                        flip |= 1 << b;
                    } else {
                        flip &= !(1 << b);
                    }
                }
            }
            flip
        })
        .collect()
}

/// Per pass of `passes`, bit per colour buffer an earlier pass of the same stage flipped,
/// given `flips` from [`pass_flips`] (Iris's `flippedAtLeastOnce`). The buffer's
/// `texture.<stage>` binding no longer replaces `colortexN` or its legacy alias there, so
/// the pass samples the buffer. `prepare`, `deferred` and `composite` each start empty;
/// `final` sees the composite passes' flips.
fn released_overrides(passes: &[FlipPass<'_>], flips: &[u32]) -> Vec<u32> {
    let stage = |group| match group {
        ProgramGroup::Final => ProgramGroup::Composite,
        group => group,
    };
    let mut current = None;
    let mut flipped = 0;
    passes
        .iter()
        .zip(flips)
        .map(|(pass, &flip)| {
            if current != Some(stage(pass.group)) {
                current = Some(stage(pass.group));
                flipped = 0;
            }
            let released = flipped;
            flipped |= flip;
            released
        })
        .collect()
}

/// Whether sampler `name` (`colortexN` or a legacy alias) is a colour buffer in `released`.
pub(crate) fn sampler_released(released: u32, name: &str) -> bool {
    matches!(BufferId::parse(name), Some(BufferId::Color(b)) if b < MAX_COLOR_BUFFERS && released & (1 << b) != 0)
}

/// Whether `group`'s passes run each frame between geometry stages and can flip buffers.
fn full_screen(group: ProgramGroup) -> bool {
    matches!(
        group,
        ProgramGroup::Prepare
            | ProgramGroup::Deferred
            | ProgramGroup::Composite
            | ProgramGroup::Final
    )
}

/// [`released_overrides`] per program of a folder's translations in frame order; zero for
/// disabled or failed programs and outside the full-screen stages.
pub(crate) fn program_releases(programs: &[ProgramTranslation], rules: &[FlipRule]) -> Vec<u32> {
    let passes: Vec<(usize, FlipPass<'_>)> = programs
        .iter()
        .enumerate()
        .filter(|(_, p)| p.enabled && full_screen(p.program.group))
        .filter_map(|(index, p)| {
            let writes = p
                .reflection
                .as_ref()?
                .color_outputs
                .iter()
                .filter_map(|o| o.buffer)
                .filter(|&b| b < MAX_COLOR_BUFFERS)
                .fold(0, |mask, b| mask | (1 << b));
            Some((
                index,
                FlipPass {
                    name: &p.program.name,
                    group: p.program.group,
                    kind: p.program.kind,
                    writes,
                },
            ))
        })
        .collect();
    let list: Vec<FlipPass<'_>> = passes.iter().map(|(_, pass)| *pass).collect();
    let released = released_overrides(&list, &pass_flips(&list, rules));
    let mut out = vec![0; programs.len()];
    for ((index, _), released) in passes.iter().zip(released) {
        out[*index] = released;
    }
    out
}

/// Whether `program` samples a name whose stage binding `released` cancels, so its
/// translation must read that name through the buffer's sampler class instead.
pub(crate) fn samples_released_override(
    program: &ProgramTranslation,
    released: u32,
    properties: &ShaderProperties,
) -> bool {
    let stage = program.program.group.texture_stage();
    program.reflection.as_ref().is_some_and(|reflection| {
        reflection.textures.iter().any(|texture| {
            let name = canonical_sampler_name(&texture.name);
            properties.custom_textures.iter().any(|custom| {
                custom.stage == stage
                    && custom.released_by(released)
                    && canonical_sampler_name(custom.uniform()) == name
            })
        })
    })
}
