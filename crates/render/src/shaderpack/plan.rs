//! The device-independent frame plan of a loaded pack: which colour buffers exist with which
//! format, size and clear, and for each full-screen pass which side of every ping-pong
//! buffer it reads and writes.
//!
//! OptiFine/Iris flip semantics: every buffer has a main and an alternate texture and starts
//! the frame unflipped (reads main). A deferred/composite pass reads each buffer's current
//! side and draws into the other side; afterwards each buffer it wrote flips, unless
//! `flip.<program>.<buffer>=false`, and `flip.<program>.<buffer>=true` also flips a buffer
//! the pass did not write. The plan is identical every frame. Buffers that end the frame
//! flipped either exchange their two textures (when the next frame never observes the
//! difference) or are copied back to main after `final` unless the next frame clears them
//! anyway, so the next frame starts unflipped.

use shaderpack::{BufferDirectives, BufferFormat, BufferId, FlipRule, ProgramGroup, SizeValue};

/// Colour buffers a pack may use (`colortex0`–`colortex15`).
pub(crate) const MAX_COLOR_BUFFERS: u32 = shaderpack::MAX_COLOR_BUFFERS;

/// A buffer's size relative to the screen or in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Extent {
    Relative([f32; 2]),
    Absolute([u32; 2]),
}

impl Extent {
    /// The size in pixels for a screen of `screen`, at least 1×1.
    pub(crate) fn resolve(self, screen: [u32; 2]) -> [u32; 2] {
        match self {
            Extent::Relative(scale) => {
                std::array::from_fn(|i| ((screen[i] as f32 * scale[i]) as u32).max(1))
            }
            Extent::Absolute(size) => size.map(|s| s.max(1)),
        }
    }
}

/// One colour buffer of the pack.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BufferPlan {
    pub index: u32,
    pub format: BufferFormat,
    pub extent: Extent,
    /// What both textures are cleared to at the start of each frame; `None` keeps the
    /// previous frame's contents (`colortexNClear = false`).
    pub clear: Option<BufferClear>,
    /// Some pass requests mipmaps, so the textures carry a full mip chain.
    pub mipmapped: bool,
    /// Some pass reads or writes the alternate texture.
    pub alternate: bool,
}

/// A colour buffer's per-frame clear value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum BufferClear {
    /// A fixed colour (`colortexNClearColor`, or Iris's default for the buffer).
    Color([f64; 4]),
    /// This frame's `fogColor` with alpha 1, Iris's default for `colortex0` (vanilla's
    /// clear colour).
    Fog,
}

impl BufferClear {
    /// The clear colour given this frame's fog colour.
    pub(crate) fn resolve(self, fog: [f32; 3]) -> [f64; 4] {
        match self {
            BufferClear::Color(colour) => colour,
            BufferClear::Fog => [f64::from(fog[0]), f64::from(fog[1]), f64::from(fog[2]), 1.0],
        }
    }
}

/// A program as the frame plan sees it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlannedProgram {
    pub name: String,
    pub group: ProgramGroup,
    pub kind: shaderpack::ProgramKind,
    /// Destination buffer per fragment output location; empty for compute programs.
    pub outputs: Vec<Option<u32>>,
    /// Buffers referenced through samplers or images.
    pub reads: Vec<u32>,
    /// Buffers whose `MipmapEnabled` directive this program declares.
    pub mipmaps: Vec<u32>,
    /// Bit per buffer: a draw of this program may leave pixels of that output unwritten
    /// (a blended output, or every output of a program that discards fragments or draws a
    /// scaled viewport).
    pub partial: u32,
}

impl PlannedProgram {
    /// Rejects unsupported buffer indices before a planner constructs bit masks.
    fn validate(&self) -> Result<(), String> {
        for &index in self
            .outputs
            .iter()
            .flatten()
            .chain(&self.reads)
            .chain(&self.mipmaps)
        {
            if index >= MAX_COLOR_BUFFERS {
                return Err(format!(
                    "{} references unsupported color buffer {index}; expected an index below {MAX_COLOR_BUFFERS}",
                    self.name
                ));
            }
        }
        Ok(())
    }
}

/// Which side of every buffer a pass or draw samples, and which of those sides show their
/// mip levels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct BufferReads {
    /// Bit per buffer: samplers and images read the alternate texture.
    pub alt: u32,
    /// Bit per buffer: the side read had its mip chain generated earlier in the frame, so
    /// samplers filter between its levels. Iris enables mipmap filtering on a texture when it
    /// generates the texture's mips and resets it at the end of the frame; reads before that
    /// see the base level only.
    pub mipmapped: u32,
}

impl BufferReads {
    /// Reads of the sides `alt` picks, given per side the buffers whose mips were generated.
    pub(crate) fn new(alt: u32, generated: [u32; 2]) -> Self {
        Self {
            alt,
            mipmapped: (generated[0] & !alt) | (generated[1] & alt),
        }
    }
}

/// One pass of the frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PassStep {
    /// Index into the plan's programs.
    pub program: usize,
    /// Bit per buffer: samplers and images read the alternate texture.
    pub read_alt: u32,
    /// Per fragment output location: the destination buffer and whether the alternate
    /// texture is written. `final` writes the screen, so its entries are `None`.
    pub writes: Vec<Option<(u32, bool)>>,
    /// Buffers whose current side gets its mip chain regenerated before the pass.
    pub mipmaps: Vec<u32>,
    /// Bit per buffer: the side read shows its mip levels ([`BufferReads::mipmapped`]).
    pub mipmapped: u32,
}

impl PassStep {
    pub(crate) fn reads(&self) -> BufferReads {
        BufferReads {
            alt: self.read_alt,
            mipmapped: self.mipmapped,
        }
    }
}

/// The passes of one frame around the geometry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FramePlan {
    /// `prepare*` passes, before the scene.
    pub prepare: Vec<PassStep>,
    /// Bit per buffer: the side the sky, opaque geometry (or the scene copy) write (the
    /// current side after the prepare passes, as Iris's gbuffer framebuffers do).
    pub scene_alt: u32,
    /// Per side, bit per buffer: mip chains the prepare passes generated.
    pub scene_mips: [u32; 2],
    /// `deferred*`, `composite*` and `final`, in order.
    pub post: Vec<PassStep>,
    /// Index in `post` of the first pass after the deferred ones; translucent geometry draws
    /// there.
    pub composite_start: usize,
    /// Bit per buffer: the side translucent geometry writes (the current side after the
    /// deferred passes).
    pub translucent_alt: u32,
    /// Per side, bit per buffer: mip chains generated before translucent geometry.
    pub translucent_mips: [u32; 2],
    /// Buffers that end the frame flipped and whose alternate texture is copied to main.
    pub swap_back: Vec<u32>,
    /// Buffers that end the frame flipped and exchange their main and alternate textures
    /// instead ([`exchange_swap_back`]).
    pub exchange: Vec<u32>,
}

/// Iris's default clear: `colortex0` to the fog colour, `colortex1` (`gdepth`) to white, the
/// rest to zero.
fn default_clear(index: u32) -> BufferClear {
    match index {
        0 => BufferClear::Fog,
        1 => BufferClear::Color([1.0; 4]),
        _ => BufferClear::Color([0.0; 4]),
    }
}

/// Parses a `ClearColor` directive value: `vec4(r, g, b, a)`, `vec3(r, g, b)` (alpha 1) or
/// a scalar (broadcast). `None` when it is not a list of 1, 3 or 4 numbers.
pub(crate) fn parse_clear_color(text: &str) -> Option<[f64; 4]> {
    let inner = text
        .trim()
        .trim_start_matches(|c: char| c.is_ascii_alphanumeric())
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')');
    let values: Vec<f64> = inner
        .split(',')
        .map(|v| v.trim().trim_end_matches(['f', 'F']).parse::<f64>().ok())
        .collect::<Option<_>>()?;
    match values.as_slice() {
        [v] => Some([*v; 4]),
        [r, g, b] => Some([*r, *g, *b, 1.0]),
        [r, g, b, a] => Some([*r, *g, *b, *a]),
        _ => None,
    }
}

fn size_extent(width: SizeValue, height: SizeValue) -> Extent {
    match (width, height) {
        (SizeValue::Absolute(w), SizeValue::Absolute(h)) => Extent::Absolute([w, h]),
        (w, h) => {
            let relative = |v| match v {
                SizeValue::Relative(r) => r,
                // A mixed pair cannot be expressed; Iris treats both as relative then.
                SizeValue::Absolute(a) => a as f32,
            };
            Extent::Relative([relative(w), relative(h)])
        }
    }
}

/// The colour buffer behind a pack sampler name (`colortex3`, `gaux1`) or image name
/// (`colorimg3`).
pub(crate) fn color_buffer_of(name: &str) -> Option<u32> {
    let id = match name.strip_prefix("colorimg") {
        Some(rest) => BufferId::parse(&format!("colortex{rest}")),
        None => BufferId::parse(name),
    };
    match id {
        Some(BufferId::Color(i)) if i < MAX_COLOR_BUFFERS => Some(i),
        _ => None,
    }
}

/// Plans the passes of a frame; `programs` are the runnable prepare/deferred/composite/final
/// programs in frame order.
pub(crate) fn plan_frame(
    programs: &[PlannedProgram],
    flips: &[FlipRule],
) -> Result<FramePlan, String> {
    for program in programs {
        program.validate()?;
    }
    let passes: Vec<shaderpack::FlipPass<'_>> = programs
        .iter()
        .map(|program| shaderpack::FlipPass {
            name: &program.name,
            group: program.group,
            kind: program.kind,
            writes: program
                .outputs
                .iter()
                .flatten()
                .fold(0, |mask, b| mask | (1 << b)),
        })
        .collect();
    let flips = shaderpack::pass_flips(&passes, flips);
    // The side each buffer reads, and per side the mip chains generated so far.
    type State = (u32, [u32; 2]);
    let step = |index: usize, program: &PlannedProgram, (flipped, mips): &mut State| {
        let read_alt = *flipped;
        for &b in &program.mipmaps {
            mips[usize::from(read_alt & (1 << b) != 0)] |= 1 << b;
        }
        let is_final = program.group == ProgramGroup::Final;
        let writes: Vec<Option<(u32, bool)>> = program
            .outputs
            .iter()
            .map(|b| {
                b.filter(|_| !is_final)
                    .map(|b| (b, read_alt & (1 << b) == 0))
            })
            .collect();
        *flipped ^= flips[index];
        PassStep {
            program: index,
            read_alt,
            writes,
            mipmaps: program.mipmaps.clone(),
            mipmapped: BufferReads::new(read_alt, *mips).mipmapped,
        }
    };
    let mut state: State = (0, [0; 2]);
    let mut scene = state;
    let mut prepare = vec![];
    let mut post = vec![];
    let mut translucent = None;
    for (index, program) in programs.iter().enumerate() {
        let geometry_done = !matches!(
            program.group,
            ProgramGroup::Prepare | ProgramGroup::Deferred
        );
        if geometry_done && translucent.is_none() {
            translucent = Some((post.len(), state));
        }
        let s = step(index, program, &mut state);
        if program.group == ProgramGroup::Prepare {
            prepare.push(s);
            // Scene writes happen between prepare and deferred.
            scene = state;
        } else {
            post.push(s);
        }
    }
    let (composite_start, (translucent_alt, translucent_mips)) =
        translucent.unwrap_or((post.len(), state));
    Ok(FramePlan {
        prepare,
        scene_alt: scene.0,
        scene_mips: scene.1,
        post,
        composite_start,
        translucent_alt,
        translucent_mips,
        swap_back: (0..MAX_COLOR_BUFFERS)
            .filter(|b| state.0 & (1 << b) != 0)
            .collect(),
        exchange: vec![],
    })
}

/// Leaves out of `frame.swap_back` the buffers cleared at the start of every frame: nothing
/// reads their last side before the clear, as Iris skips their copy-back. `colortex0` stays
/// when no `final` program runs, since the screen is then presented from its main texture.
pub(crate) fn skip_cleared_swap_back(
    frame: &mut FramePlan,
    buffers: &[BufferPlan],
    programs: &[PlannedProgram],
) {
    let presents_colortex0 = !programs.iter().any(|p| p.group == ProgramGroup::Final);
    frame.swap_back.retain(|&index| {
        (presents_colortex0 && index == 0)
            || buffers
                .iter()
                .find(|buffer| buffer.index == index)
                .is_none_or(|buffer| buffer.clear.is_none())
    });
}

/// Moves into `frame.exchange` the buffers of `frame.swap_back` whose copy-back an exchange
/// of their two textures can replace. After the exchange main holds what the copy would
/// give it, while alternate holds an older content than the copy's; that is invisible when
/// every frame overwrites the whole alternate side before reading it. So a buffer stays
/// copied when geometry draws into it (draws cover part of the target and read the side
/// they do not write), or when some pass or draw reads its alternate side, or the frame
/// ends on that side, while only partial writes (`PlannedProgram::partial`) or none have
/// replaced the older content.
pub(crate) fn exchange_swap_back(
    frame: &mut FramePlan,
    programs: &[PlannedProgram],
    geometry: &[PlannedProgram],
) -> Result<(), String> {
    for program in programs.iter().chain(geometry) {
        program.validate()?;
    }
    let mut geometry_reads = 0u32;
    let mut copied = 0u32;
    for program in geometry {
        for b in program.outputs.iter().flatten() {
            copied |= 1 << b;
        }
        for b in &program.reads {
            geometry_reads |= 1 << b;
        }
    }
    // Per side, bit per buffer: the side may differ from what the copy-back gives it.
    let mut stale = [0u32, u32::MAX];
    let mut read = |buffers: u32, read_alt: u32, stale: &[u32; 2]| {
        copied |= buffers & ((stale[0] & !read_alt) | (stale[1] & read_alt));
    };
    let mut run = |steps: &[PassStep], stale: &mut [u32; 2]| {
        for step in steps {
            let program = &programs[step.program];
            let reads = program
                .reads
                .iter()
                .chain(&step.mipmaps)
                .fold(0u32, |mask, b| mask | (1 << b));
            read(reads, step.read_alt, stale);
            for &(b, alt) in step.writes.iter().flatten() {
                if program.partial & (1 << b) == 0 {
                    stale[usize::from(alt)] &= !(1 << b);
                }
            }
        }
    };
    run(&frame.prepare, &mut stale);
    let scene = stale;
    run(&frame.post[..frame.composite_start], &mut stale);
    let translucent = stale;
    run(&frame.post[frame.composite_start..], &mut stale);
    copied |= geometry_reads & ((scene[0] & !frame.scene_alt) | (scene[1] & frame.scene_alt));
    copied |= geometry_reads
        & ((translucent[0] & !frame.translucent_alt) | (translucent[1] & frame.translucent_alt));
    copied |= stale[1];
    let (copy, exchange) = frame
        .swap_back
        .iter()
        .partition(|&&b| copied & (1 << b) != 0);
    frame.swap_back = copy;
    frame.exchange = exchange;
    Ok(())
}

/// Plans the colour buffers the passes and the `gbuffers_*` programs in `geometry` touch
/// (plus buffer 0, which receives the scene). A geometry program that samples a buffer it
/// also draws into reads the buffer's other side, so such buffers get an alternate texture.
pub(crate) fn plan_buffers(
    programs: &[PlannedProgram],
    geometry: &[PlannedProgram],
    frame: &FramePlan,
    directives: &BufferDirectives,
    sizes: &[shaderpack::BufferSize],
) -> Result<Vec<BufferPlan>, String> {
    for program in programs.iter().chain(geometry) {
        program.validate()?;
    }
    let final_alternate = frame
        .swap_back
        .iter()
        .chain(&frame.exchange)
        .fold(0, |mask, index| mask | (1 << index));
    let mut alternate = frame.scene_alt | final_alternate;
    let mut used = 1 | alternate;
    let mut mipmapped = 0u32;
    for program in programs.iter().chain(geometry) {
        for b in program.outputs.iter().flatten().chain(&program.reads) {
            used |= 1 << b;
        }
        for b in &program.mipmaps {
            mipmapped |= 1 << b;
        }
    }
    if !geometry.is_empty() {
        alternate |= frame.translucent_alt;
    }
    for program in geometry {
        for b in program.outputs.iter().flatten() {
            if program.reads.contains(b) {
                alternate |= 1 << b;
            }
        }
    }
    for step in frame.prepare.iter().chain(&frame.post) {
        alternate |= step.read_alt;
        for (b, alt) in step.writes.iter().flatten() {
            if *alt {
                alternate |= 1 << b;
            }
        }
    }
    Ok((0..MAX_COLOR_BUFFERS)
        .filter(|b| used & (1 << b) != 0)
        .map(|index| {
            let settings = directives.buffers.get(&BufferId::Color(index));
            let extent = sizes
                .iter()
                .rev()
                .find(|s| color_buffer_of(&s.buffer) == Some(index))
                .map_or(Extent::Relative([1.0, 1.0]), |s| {
                    size_extent(s.width, s.height)
                });
            let clear = match settings.and_then(|s| s.clear) {
                Some(false) => None,
                _ => Some(
                    settings
                        .and_then(|s| s.clear_color.as_deref())
                        .and_then(parse_clear_color)
                        .map_or_else(|| default_clear(index), BufferClear::Color),
                ),
            };
            BufferPlan {
                index,
                format: directives.format(BufferId::Color(index)),
                extent,
                clear,
                mipmapped: mipmapped & (1 << index) != 0,
                alternate: alternate & (1 << index) != 0,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(name: &str, group: ProgramGroup, outputs: &[u32]) -> PlannedProgram {
        PlannedProgram {
            name: name.to_string(),
            group,
            kind: shaderpack::ProgramKind::Render,
            outputs: outputs.iter().map(|b| Some(*b)).collect(),
            reads: vec![],
            mipmaps: vec![],
            partial: 0,
        }
    }

    fn flip(program: &str, buffer: &str, flip: bool) -> FlipRule {
        FlipRule {
            program: program.to_string(),
            buffer: buffer.to_string(),
            flip,
        }
    }

    #[test]
    fn unsupported_buffer_indices_fail_before_planning_masks() {
        let directives = BufferDirectives::default();
        let frame = plan_frame(&[], &[]).unwrap();
        for index in [MAX_COLOR_BUFFERS - 1, MAX_COLOR_BUFFERS, 32, u32::MAX] {
            for field in ["output", "read", "mipmap"] {
                let mut pass = program("composite", ProgramGroup::Composite, &[]);
                match field {
                    "output" => pass.outputs = vec![Some(index)],
                    "read" => pass.reads = vec![index],
                    _ => pass.mipmaps = vec![index],
                }
                let programs = [pass];
                let supported = index < MAX_COLOR_BUFFERS;
                assert_eq!(
                    plan_frame(&programs, &[]).is_ok(),
                    supported,
                    "{field} {index}"
                );
                assert_eq!(
                    plan_buffers(&[], &programs, &frame, &directives, &[]).is_ok(),
                    supported,
                    "geometry {field} {index}"
                );
                assert_eq!(
                    exchange_swap_back(&mut frame.clone(), &[], &programs).is_ok(),
                    supported,
                    "exchange {field} {index}"
                );
            }
        }
    }

    #[test]
    fn relative_extents_truncate_odd_screen_dimensions() {
        assert_eq!(Extent::Relative([0.5; 2]).resolve([1921, 1081]), [960, 540]);
        assert_eq!(Extent::Relative([0.25; 2]).resolve([7, 3]), [1, 1]);
    }

    #[test]
    fn written_buffers_ping_pong_and_end_flipped_buffers_swap_back() {
        let programs = [
            program("deferred", ProgramGroup::Deferred, &[0, 4]),
            program("composite", ProgramGroup::Composite, &[0]),
            program("composite1", ProgramGroup::Composite, &[4]),
            program("final", ProgramGroup::Final, &[0]),
        ];
        let plan = plan_frame(&programs, &[]).unwrap();
        let deferred = &plan.post[0];
        assert_eq!(deferred.read_alt, 0);
        assert_eq!(deferred.writes, [Some((0, true)), Some((4, true))]);
        let composite = &plan.post[1];
        assert_eq!(composite.read_alt, (1 << 0) | (1 << 4));
        assert_eq!(composite.writes, [Some((0, false))]);
        let composite1 = &plan.post[2];
        assert_eq!(composite1.read_alt, 1 << 4);
        assert_eq!(composite1.writes, [Some((4, false))]);
        let fin = &plan.post[3];
        assert_eq!(fin.read_alt, 0);
        assert_eq!(fin.writes, [None]);
        assert!(plan.swap_back.is_empty());
    }

    #[test]
    fn flip_rules_suppress_and_force_flips() {
        let programs = [
            program("composite", ProgramGroup::Composite, &[3]),
            program("composite1", ProgramGroup::Composite, &[2]),
            program("final", ProgramGroup::Final, &[0]),
        ];
        let rules = [
            flip("composite", "colortex3", false),
            flip("composite1", "gaux2", true),
        ];
        let plan = plan_frame(&programs, &rules).unwrap();
        assert_eq!(plan.post[0].writes, [Some((3, true))]);
        assert_eq!(plan.post[1].read_alt, 0, "colortex3 did not flip");
        assert_eq!(plan.post[2].read_alt, (1 << 2) | (1 << 5));
        assert_eq!(plan.swap_back, [2, 5]);
    }

    #[test]
    fn scene_writes_the_side_current_after_prepare() {
        let programs = [
            program("prepare", ProgramGroup::Prepare, &[0]),
            program("deferred", ProgramGroup::Deferred, &[1]),
        ];
        let plan = plan_frame(&programs, &[]).unwrap();
        assert_eq!(plan.prepare.len(), 1);
        assert_eq!(plan.scene_alt, 1);
        assert_eq!(plan.post[0].read_alt, 1);
        assert_eq!(plan.swap_back, [0, 1]);
    }

    #[test]
    fn buffers_cleared_every_frame_are_not_copied_back() {
        let buffer = |index, clear| BufferPlan {
            index,
            format: BufferFormat::Rgba8,
            extent: Extent::Relative([1.0; 2]),
            clear,
            mipmapped: false,
            alternate: true,
        };
        let cleared = Some(BufferClear::Color([0.0; 4]));
        let mut programs = vec![
            program("prepare", ProgramGroup::Prepare, &[0]),
            program("deferred", ProgramGroup::Deferred, &[1]),
        ];
        let buffers = [buffer(0, cleared), buffer(1, cleared)];
        let mut plan = plan_frame(&programs, &[]).unwrap();
        skip_cleared_swap_back(&mut plan, &buffers, &programs);
        assert_eq!(plan.swap_back, [0], "without final, colortex0 is presented");

        programs.push(program("final", ProgramGroup::Final, &[0]));
        let buffers = [buffer(0, cleared), buffer(1, None)];
        let mut plan = plan_frame(&programs, &[]).unwrap();
        skip_cleared_swap_back(&mut plan, &buffers, &programs);
        assert_eq!(plan.swap_back, [1], "only the buffer kept across frames");
    }

    #[test]
    fn buffers_rewritten_whole_every_frame_exchange_instead_of_copying() {
        let reads = |mut p: PlannedProgram, reads: &[u32]| {
            p.reads = reads.to_vec();
            p
        };
        let mut blended = program("composite1", ProgramGroup::Composite, &[4]);
        blended.partial = 1 << 4;
        let programs = [
            // Temporal buffers: last frame's result in main, this frame's into alternate.
            reads(
                program("deferred", ProgramGroup::Deferred, &[5, 6]),
                &[5, 6],
            ),
            program("composite", ProgramGroup::Composite, &[2, 4]),
            blended,
            // colortex7 flips unwritten, so its alternate side is read and ends current.
            reads(program("composite2", ProgramGroup::Composite, &[]), &[7]),
            program("final", ProgramGroup::Final, &[0]),
        ];
        let rules = [flip("composite1", "colortex7", true)];
        let mut water = program("gbuffers_water", ProgramGroup::Gbuffers, &[6]);
        water.reads = vec![2];
        let mut plan = plan_frame(&programs, &rules).unwrap();
        assert_eq!(plan.swap_back, [2, 5, 6, 7]);
        exchange_swap_back(&mut plan, &programs, &[water]).unwrap();
        assert_eq!(
            plan.exchange,
            [2, 5],
            "written whole before any read of alternate"
        );
        assert_eq!(
            plan.swap_back,
            [6, 7],
            "geometry draws into colortex6; colortex7's alternate is never written"
        );
        let plan_without = |programs: &[PlannedProgram]| {
            let mut plan = plan_frame(programs, &rules).unwrap();
            exchange_swap_back(&mut plan, programs, &[]).unwrap();
            plan
        };
        let plan = plan_without(&programs[1..3]);
        assert!(
            !plan.exchange.contains(&4) && !plan.swap_back.contains(&4),
            "colortex4 is written twice and ends unflipped"
        );
        let mut programs = programs[..2].to_vec();
        programs[1].partial = 1 << 2;
        assert_eq!(
            plan_without(&programs).swap_back,
            [2],
            "a partial write leaves alternate's older content visible"
        );
        let programs = [
            program("deferred", ProgramGroup::Deferred, &[5]),
            reads(program("composite", ProgramGroup::Composite, &[2]), &[2]),
            program("composite1", ProgramGroup::Composite, &[2]),
        ];
        let mut plan = plan_frame(&programs, &[flip("deferred", "colortex2", true)]).unwrap();
        exchange_swap_back(&mut plan, &programs, &[]).unwrap();
        assert_eq!(
            plan.swap_back,
            [2],
            "composite reads colortex2's alternate side before composite1 rewrites it"
        );
        assert_eq!(plan.exchange, [5]);
    }

    #[test]
    fn translucent_geometry_writes_the_side_current_after_deferred() {
        let programs = [
            program("deferred", ProgramGroup::Deferred, &[0, 2]),
            program("deferred1", ProgramGroup::Deferred, &[2]),
            program("composite", ProgramGroup::Composite, &[0]),
            program("final", ProgramGroup::Final, &[0]),
        ];
        let plan = plan_frame(&programs, &[]).unwrap();
        assert_eq!(plan.composite_start, 2);
        assert_eq!(
            plan.translucent_alt, 1,
            "colortex0 flipped once, colortex2 twice"
        );
        assert_eq!(plan.post[2].read_alt, plan.translucent_alt);
        let mut water = program("gbuffers_water", ProgramGroup::Gbuffers, &[0, 5]);
        water.reads = vec![5];
        let buffers = plan_buffers(
            &programs,
            &[water],
            &plan,
            &BufferDirectives::default(),
            &[],
        )
        .unwrap();
        let indices: Vec<u32> = buffers.iter().map(|b| b.index).collect();
        assert_eq!(indices, [0, 2, 5]);
        assert!(
            buffers[2].alternate,
            "a buffer a geometry program reads and writes gets a second side"
        );
        let plan = plan_frame(&programs[..2], &[]).unwrap();
        assert_eq!(
            plan.composite_start, 2,
            "no composite: translucents draw last"
        );
    }

    #[test]
    fn mip_levels_show_only_on_the_side_generated_earlier_in_the_frame() {
        let mut programs = [
            program("deferred", ProgramGroup::Deferred, &[3]),
            program("deferred1", ProgramGroup::Deferred, &[3]),
            program("composite", ProgramGroup::Composite, &[0]),
            program("composite1", ProgramGroup::Composite, &[3]),
        ];
        programs[1].mipmaps = vec![0];
        let plan = plan_frame(&programs, &[]).unwrap();
        let mipmapped: Vec<u32> = plan.post.iter().map(|s| s.mipmapped & 1).collect();
        assert_eq!(
            mipmapped,
            [0, 1, 1, 0],
            "before generation, then main generated, then composite flips to alternate"
        );
        assert_eq!(plan.scene_mips, [0, 0]);
        let translucent = BufferReads::new(plan.translucent_alt, plan.translucent_mips);
        assert_eq!(translucent.mipmapped & 1, 1);
    }

    #[test]
    fn buffer_plan_applies_formats_sizes_clears_and_mipmaps() {
        let mut directives = BufferDirectives::default();
        for (name, value) in [
            ("colortex2Format", "RGBA16F"),
            ("colortex2Clear", "false"),
            ("colortex3ClearColor", "vec4(0.25, 0.5, 0.75, 1.0)"),
        ] {
            directives.accept(&shaderpack::ConstDirective {
                ty: "int".into(),
                name: name.into(),
                value: value.into(),
            });
        }
        let mut programs = vec![
            program("composite", ProgramGroup::Composite, &[2, 3]),
            program("final", ProgramGroup::Final, &[0]),
        ];
        programs[1].reads = vec![1, 6];
        programs[1].mipmaps = vec![2];
        let sizes = [
            shaderpack::BufferSize {
                buffer: "colortex6".into(),
                width: SizeValue::Relative(0.5),
                height: SizeValue::Relative(0.25),
            },
            shaderpack::BufferSize {
                buffer: "colortex1".into(),
                width: SizeValue::Absolute(64),
                height: SizeValue::Absolute(32),
            },
        ];
        let frame = plan_frame(&programs, &[]).unwrap();
        let buffers = plan_buffers(&programs, &[], &frame, &directives, &sizes).unwrap();
        let indices: Vec<u32> = buffers.iter().map(|b| b.index).collect();
        assert_eq!(indices, [0, 1, 2, 3, 6]);
        let get = |i| buffers.iter().find(|b| b.index == i).unwrap();
        assert_eq!(get(2).format, BufferFormat::Rgba16f);
        assert_eq!(get(2).clear, None);
        assert!(get(2).mipmapped && get(2).alternate);
        assert_eq!(
            get(3).clear,
            Some(BufferClear::Color([0.25, 0.5, 0.75, 1.0]))
        );
        assert_eq!(get(1).clear, Some(BufferClear::Color([1.0; 4])));
        assert_eq!(get(0).clear, Some(BufferClear::Fog));
        assert_eq!(get(0).format, BufferFormat::DEFAULT);
        assert!(!get(0).alternate);
        assert_eq!(get(6).extent.resolve([1920, 1080]), [960, 270]);
        assert_eq!(get(1).extent.resolve([1920, 1080]), [64, 32]);
    }

    #[test]
    fn clear_colours_and_buffer_names_parse() {
        assert_eq!(
            parse_clear_color("vec3(1.0, 0.5, 0)"),
            Some([1.0, 0.5, 0.0, 1.0])
        );
        assert_eq!(parse_clear_color("0.5"), Some([0.5; 4]));
        assert_eq!(parse_clear_color("vec2(1.0, 2.0)"), None);
        assert_eq!(color_buffer_of("gaux4"), Some(7));
        assert_eq!(color_buffer_of("colorimg12"), Some(12));
        assert_eq!(color_buffer_of("colortex16"), None);
        assert_eq!(color_buffer_of("shadowcolor0"), None);
    }
    #[test]
    fn forced_flip_after_a_compute_and_render_pair_happens_once() {
        let mut compute = program("composite", ProgramGroup::Composite, &[]);
        compute.kind = shaderpack::ProgramKind::Compute;
        let render = program("composite", ProgramGroup::Composite, &[0]);
        let programs = [compute.clone(), render];
        let rules = [flip("composite", "colortex5", true)];
        let frame = plan_frame(&programs, &rules).unwrap();
        assert_eq!(frame.post[1].read_alt, 0);
        assert!(frame.swap_back.contains(&5));
        let alone = plan_frame(&[compute], &rules).unwrap();
        assert!(alone.swap_back.contains(&5));
    }

    #[test]
    fn final_forced_flips_allocate_alternates_even_without_an_output() {
        let mut pass = program("composite", ProgramGroup::Composite, &[0]);
        pass.reads = vec![5];
        let mut programs = [pass];
        let mut directives = BufferDirectives::default();
        directives
            .buffers
            .entry(BufferId::Color(5))
            .or_default()
            .clear = Some(false);
        for reads_buffer in [true, false] {
            if !reads_buffer {
                programs[0].reads.clear();
            }
            let mut frame = plan_frame(&programs, &[flip("composite", "colortex5", true)]).unwrap();
            let buffers = plan_buffers(&programs, &[], &frame, &directives, &[]).unwrap();
            assert!(
                buffers
                    .iter()
                    .find(|buffer| buffer.index == 5)
                    .unwrap()
                    .alternate
            );
            skip_cleared_swap_back(&mut frame, &buffers, &programs);
            assert!(frame.swap_back.contains(&5));
            exchange_swap_back(&mut frame, &programs, &[]).unwrap();
            let exchanged = plan_buffers(&programs, &[], &frame, &directives, &[]).unwrap();
            assert!(
                exchanged
                    .iter()
                    .find(|buffer| buffer.index == 5)
                    .unwrap()
                    .alternate
            );
        }
    }
}
