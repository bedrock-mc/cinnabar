//! Static home portraits from the shared player mesh and locally installed default skin.

use crate::carriers::{self, Error};
use jsonui_editor::raster::Page;
use std::{path::Path, sync::Arc};
use ui::{UiBlendMode, UiMesh, UiMeshBatch, UiMeshVertex, UiNode, UiNodeId, UiVisual};
use ui_engine::engine::screen::PaintFrame;

/// Loads the same default skin used by the client, without account or network services.
pub(super) fn texture(dir: &Path, pages: &mut Vec<Page>) -> Result<u16, Error> {
    carriers::install_default_skin(dir)?;
    let side = render_model::STANDARD_SKIN_SIDE as u32;
    let page = u16::try_from(pages.len()).map_err(|error| error.to_string())?;
    pages.push(Page {
        repeat_content: None,
        width: side,
        height: side,
        rgba: render_model::default_actor_skin_rgba8().pixels().clone(),
    });
    Ok(page)
}

/// Turns the shared biped toward the camera for the offline portrait only.
fn project([x, y, z]: [f32; 3]) -> [f32; 3] {
    let (sin, cos) = 25.0_f32.to_radians().sin_cos();
    let across = x * cos + z * sin;
    let depth = -x * sin + z * cos;
    let (sin, cos) = 8.0_f32.to_radians().sin_cos();
    [across, y * cos - depth * sin, y * sin + depth * cos]
}

/// Appends a static default-skin model in the production painter's character viewport.
pub(super) fn paint(bounds: [f32; 4], page: u16, frame: &mut PaintFrame<'_>) -> Result<(), Error> {
    let mut source = render_model::standard_biped_vertices();
    source.extend(render_model::standard_biped_overlay_vertices());
    let mut triangles: Vec<&[render_model::ActorVertex]> = source
        .as_chunks::<3>()
        .0
        .iter()
        .map(|triangle| triangle.as_slice())
        .collect();
    triangles.sort_by(|a, b| {
        let depth = |triangle: &[render_model::ActorVertex]| {
            triangle
                .iter()
                .map(|vertex| project(vertex.position)[2])
                .sum::<f32>()
        };
        depth(a).total_cmp(&depth(b))
    });
    let extent = source.iter().map(|vertex| project(vertex.position)).fold(
        [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ],
        |[left, top, right, bottom], p| {
            [
                left.min(p[0]),
                top.min(p[1]),
                right.max(p[0]),
                bottom.max(p[1]),
            ]
        },
    );
    let width = bounds[2] - bounds[0];
    let height = bounds[3] - bounds[1];
    if width <= 0.0 || height <= 0.0 {
        return Ok(());
    }
    let fit = (width / (extent[2] - extent[0])).min(height / (extent[3] - extent[1])) * 0.94;
    let center = [(extent[0] + extent[2]) * 0.5, (extent[1] + extent[3]) * 0.5];
    let side = render_model::STANDARD_SKIN_SIDE as f32;
    let vertices: Vec<_> = triangles
        .into_iter()
        .flat_map(|triangle| {
            let a = triangle[0].position;
            let b = triangle[1].position;
            let c = triangle[2].position;
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let normal = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let shade = if normal[1] > 0.0 {
                255
            } else if normal[0] != 0.0 {
                205
            } else {
                235
            };
            triangle.iter().map(move |vertex| {
                let p = project(vertex.position);
                UiMeshVertex {
                    position: [
                        0.5 + (p[0] - center[0]) * fit / width,
                        0.5 - (p[1] - center[1]) * fit / height,
                    ],
                    clip_z: 0.0,
                    clip_w: 1.0,
                    uv: vertex.uv.map(|uv| uv * side),
                    color: [shade, shade, shade, 255],
                    model_light: 1.0,
                    overlay_color: [0.0; 4],
                    style_flags: 0,
                    alpha_test: true,
                }
            })
        })
        .collect();
    let indices: Vec<_> = (0..vertices.len() as u32).collect();
    let batch = UiMeshBatch {
        texture_page: page,
        index_range: 0..indices.len() as u32,
        blend: UiBlendMode::Alpha,
        depth_test: false,
        depth_write: false,
        alpha_cutoff: Some(0.5),
    };
    let mesh = UiMesh::new(vertices.into(), indices.into(), vec![batch].into())
        .map_err(|error| error.to_string())?;
    let bounds = ui_engine::engine::rect(bounds[0], bounds[1], bounds[2], bounds[3])
        .map_err(|error| error.to_string())?;
    frame.nodes.push(
        UiNode::new(UiNodeId::new(*frame.next), None, bounds)
            .with_visual(UiVisual::Mesh(Arc::new(mesh))),
    );
    *frame.next += 1;
    Ok(())
}
