use super::*;

#[test]
fn reflection_capture_draws_offscreen_opaque_and_blended_actor_spans() {
    use crate::actor::gpu::ActorDrawSpan;
    let all = [
        ActorDrawSpan {
            first: 0,
            count: 2,
            page: 0,
            material: 0,
            vertex_count: 36,
        },
        ActorDrawSpan {
            first: 2,
            count: 1,
            page: 0,
            material: assets::EntityRenderMaterial::Default.word(Some(
                assets::EntityRenderMaterialState {
                    blend: true,
                    ..Default::default()
                },
            )),
            vertex_count: 36,
        },
    ];
    let main = [draws::main_span(all[0], 1).unwrap()];
    let main_settings = crate::EnhancedRendering::default();
    let capture = crate::EnhancedRendering {
        reflection_capture: true,
        ..main_settings
    };
    assert_eq!(view_spans(&main, &all, Some(&main_settings)), main);
    assert_eq!(view_spans(&main, &all, None), main);
    let captured = view_spans(&main, &all, Some(&capture));
    assert_eq!(captured, all);
    assert_eq!(captured.iter().filter(|s| blended(s.material)).count(), 1);
}
