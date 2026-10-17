//! RENDERTARGETS/DRAWBUFFERS routing and buffer `const` directives.

use crate::{open_fixture, program, reflection};
use shaderpack::{BufferFormat, BufferId, TranslateConfig};

#[test]
fn render_targets_route_outputs_and_comment_consts_set_formats() {
    let Some(pack) = open_fixture("drawbuffers") else {
        return;
    };
    let t = pack.translate(&TranslateConfig::default());
    let composite = program(&t, "composite");
    // The RENDERTARGETS inside the inactive #ifdef after the real one is ignored.
    assert_eq!(composite.draw_buffers, [3, 1]);
    let outputs: Vec<(u32, &str, Option<u32>, BufferFormat)> = reflection(composite)
        .color_outputs
        .iter()
        .map(|o| (o.location, o.name.as_str(), o.buffer, o.format))
        .collect();
    assert_eq!(
        outputs,
        [
            (0, "bloom", Some(3), BufferFormat::Rgba16f),
            (1, "history", Some(1), BufferFormat::DEFAULT),
        ]
    );
}

#[test]
fn drawbuffers_digits_route_frag_data_and_extra_locations_have_no_buffer() {
    let Some(pack) = open_fixture("drawbuffers") else {
        return;
    };
    let t = pack.translate(&TranslateConfig::default());
    let composite1 = program(&t, "composite1");
    assert_eq!(composite1.draw_buffers, [0, 2]);
    let outputs: Vec<(u32, Option<u32>, BufferFormat)> = reflection(composite1)
        .color_outputs
        .iter()
        .map(|o| (o.location, o.buffer, o.format))
        .collect();
    assert_eq!(
        outputs,
        [
            (0, Some(0), BufferFormat::DEFAULT),
            (1, Some(2), BufferFormat::DEFAULT),
            (2, None, BufferFormat::DEFAULT),
        ]
    );
}

#[test]
fn pack_buffer_settings_come_from_active_const_directives_only() {
    let Some(pack) = open_fixture("drawbuffers") else {
        return;
    };
    let t = pack.translate(&TranslateConfig::default());
    let buffers = &t.buffers;
    assert_eq!(
        buffers.buffers.keys().copied().collect::<Vec<_>>(),
        [BufferId::Color(1), BufferId::Color(3)]
    );
    assert_eq!(buffers.format(BufferId::Color(3)), BufferFormat::Rgba16f);
    // `colortex1Format = RG8` sits in an inactive #ifdef.
    assert_eq!(buffers.format(BufferId::Color(1)), BufferFormat::DEFAULT);
    assert_eq!(buffers.buffers[&BufferId::Color(1)].clear, Some(false));
    assert_eq!(buffers.format(BufferId::Color(0)), BufferFormat::DEFAULT);
    assert!(buffers.rejected.is_empty());
    assert!(
        program(&t, "composite")
            .consts
            .iter()
            .all(|c| c.name != "colortex1Format")
    );
}
