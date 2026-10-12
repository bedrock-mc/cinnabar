//! Item foil keeps the base material's alpha coverage through the production shader.

use crate::{gpu_snapshot, shader_source};
use gpu_snapshot::{Draw, Gpu};
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

#[test]
fn item_glint_layers_keep_their_shape_across_rectangular_atlas_regions() {
    let Some(gpu) =
        Gpu::for_fixture("item_glint_layers_keep_their_shape_across_rectangular_atlas_regions")
    else {
        return;
    };
    let scroll = [0.125, -0.25];
    let square = glint_coordinates(&gpu, [1.0; 2], [0.0; 2], scroll);
    for span in [[0.5, 1.0], [1.0, 0.5], [0.25, 0.125]] {
        for origin in [[0.0; 2], [0.25, 0.5], [0.125, 0.0625]] {
            assert_eq!(
                glint_coordinates(&gpu, span, origin, scroll),
                square,
                "{span:?} at {origin:?}"
            );
        }
    }
    assert_ne!(
        glint_coordinates(&gpu, [1.0; 2], [0.0; 2], [0.0; 2]),
        square
    );
}

/// Rasters both production foil coordinate layers over one full sprite region.
fn glint_coordinates(gpu: &Gpu, span: [f32; 2], origin: [f32; 2], scroll: [f32; 2]) -> Vec<u8> {
    let source = shader_source::standalone(
        r#"
import render::glint::glint_layer_uvs;
struct Parameters { span: vec2<f32>, scale: vec2<f32>, scroll: vec2<f32>, origin: vec2<f32> }
@group(0) @binding(0) var<uniform> parameters: Parameters;
struct VertexOutput { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    var out: VertexOutput;
    out.uv = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    out.position = vec4(out.uv * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0);
    return out;
}
@fragment fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    let layers = glint_layer_uvs(parameters.origin + input.uv * parameters.span, parameters.origin, parameters.scale, parameters.scroll, true);
    return fract(vec4(layers[0], layers[1]));
}
"#,
        &[],
    );
    let parameters = gpu.buffer(
        &[
            span[0],
            span[1],
            render_model::ITEM_GLINT_UV_SCALE / span[0],
            render_model::ITEM_GLINT_UV_SCALE / span[1],
            scroll[0],
            scroll[1],
            origin[0],
            origin[1],
        ],
        wgpu::BufferUsages::UNIFORM,
    );
    gpu.render(
        &source,
        "vertex",
        &[Draw {
            fragment: "fragment",
            vertices: 0..3,
            bindings: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: parameters.as_entire_binding(),
            }],
            blend: None,
            write_depth: false,
        }],
    )
}
