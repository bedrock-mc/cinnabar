//! Item foil keeps the base material's alpha coverage through the production shader.

use crate::{gpu_snapshot, shader_source};
use gpu_snapshot::Gpu;
use render as render_api;

#[path = "support/actor_raster.rs"]
mod actor_raster;

#[test]
fn item_glint_keeps_visible_low_alpha_texels_and_discards_empty_texels() {
    let Some(gpu) =
        Gpu::for_fixture("item_glint_keeps_visible_low_alpha_texels_and_discards_empty_texels")
    else {
        return;
    };
    let plane = actor_raster::cube([16, 0, 16], true, false);
    let plain = render_model::ActorMaterial::default();
    let foil = render_model::ActorMaterial {
        glint: render_model::ActorGlint::item(0.0, [1.0; 2]),
        ..plain
    };
    let background = gpu.render(&actor_raster::source(false), "actor_vertex", &[]);
    for alpha in [0, 64, 255] {
        let texels = [[128, 192, 64, alpha]; 2];
        let original = actor_raster::raster_material(&gpu, &plane, plain, false, texels);
        let glinting = actor_raster::raster_material(&gpu, &plane, foil, false, texels);
        let coverage = |pixels: &[u8]| {
            pixels
                .chunks_exact(4)
                .map(|pixel| pixel[3])
                .collect::<Vec<_>>()
        };
        assert_eq!(
            coverage(&glinting),
            coverage(&original),
            "alpha {alpha} coverage"
        );
        assert_eq!(original != background, alpha != 0);
        if alpha != 0 {
            assert_ne!(
                glinting, original,
                "item foil still adds colour at alpha {alpha}"
            );
        }
    }
}
