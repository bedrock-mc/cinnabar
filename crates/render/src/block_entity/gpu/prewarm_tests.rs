use super::*;
use crate::pipeline_warmup::{PrewarmPipelines, WarmView};

#[test]
fn first_block_selection_reuses_a_prewarmed_pipeline() {
    let (app, _) = crate::queue_review_support::app();
    let cache = app.world().resource::<PipelineCache>();
    let mut pipeline = BlockEntityPipeline::from_world(&mut World::new());
    for msaa in [Msaa::Off, Msaa::Sample2, Msaa::Sample4, Msaa::Sample8] {
        for hdr in [false, true] {
            let mut ids = Vec::new();
            let view = WarmView {
                msaa,
                hdr,
                enhanced: false,
            };
            pipeline.prewarm(cache, view, &mut ids).unwrap();
            let key = BlockEntityPipelineKey {
                mode: PipelineMode::Outline,
                msaa,
                hdr,
            };
            let outline = pipeline.variants.specialize(cache, key).unwrap();
            assert!(
                ids.contains(&outline),
                "selection outline must not compile on first target"
            );
        }
    }
}
