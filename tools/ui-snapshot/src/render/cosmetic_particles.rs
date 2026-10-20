//! Original paper-doll geometry through the production UI preview and offline painter.
use std::{collections::HashMap, sync::Arc};

use client_ui::ui_runtime::presentation::{
    gui_models::test_support::cosmetic_particle_preview, player_preview::PreviewView,
};
use image::ImageEncoder;
use jsonui_editor::raster::{Canvas, Page};
use ui::{SafeArea, UiMesh, UiMeshBatch, UiNode, UiNodeId, UiScale, UiTree, UiVisual};

use crate::carriers::Error;

/// Uses the offline harness's painter ordering for this fixed, anonymous model scene.
pub(super) fn png(
    size: [u32; 2],
    view: PreviewView,
    seconds: f64,
    emitting: bool,
) -> Result<Vec<u8>, Error> {
    let (mesh, sources) = cosmetic_particle_preview(view, seconds, emitting);
    let mut pages: HashMap<_, _> = sources
        .into_iter()
        .map(|(index, page)| {
            let [width, height] = page.dimensions();
            (
                index,
                Page {
                    repeat_content: None,
                    width,
                    height,
                    rgba: Arc::from(page.pixels()),
                },
            )
        })
        .collect();
    pages.insert(
        0,
        Page {
            repeat_content: None,
            width: 1,
            height: 1,
            rgba: Arc::from([255; 4]),
        },
    );
    let bounds = ui_engine::engine::rect(0.0, 0.0, size[0] as f32, size[1] as f32)
        .map_err(|e| e.to_string())?;
    let mut nodes =
        vec![
            UiNode::new(UiNodeId::new(1), None, bounds).with_visual(UiVisual::Solid {
                texture_page: 0,
                color: [26, 30, 38, 255],
            }),
        ];
    let mut triangles = Vec::new();
    for batch in mesh.batches() {
        for triangle in mesh.indices()
            [batch.index_range.start as usize..batch.index_range.end as usize]
            .chunks_exact(3)
        {
            let vertices = triangle
                .iter()
                .map(|&index| mesh.vertices()[index as usize])
                .collect::<Vec<_>>();
            let depth = vertices.iter().map(|vertex| vertex.clip_z).sum::<f32>();
            triangles.push((depth, batch.clone(), vertices));
        }
    }
    triangles.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, batch, mut vertices) in triangles {
        for vertex in &mut vertices {
            for channel in 0..3 {
                vertex.color[channel] =
                    (f32::from(vertex.color[channel]) * vertex.model_light).round() as u8;
            }
        }
        let batch = UiMeshBatch {
            index_range: 0..3,
            depth_test: false,
            depth_write: false,
            ..batch
        };
        let triangle = UiMesh::new(vertices.into(), Arc::from([0, 1, 2]), Arc::from([batch]))
            .map_err(|e| e.to_string())?;
        let id = u32::try_from(nodes.len() + 1).map_err(|e| e.to_string())?;
        nodes.push(
            UiNode::new(UiNodeId::new(id), None, bounds)
                .with_visual(UiVisual::Mesh(Arc::new(triangle))),
        );
    }
    let mut tree = UiTree::new(nodes).map_err(|e| e.to_string())?;
    tree.layout(bounds, UiScale::default(), SafeArea::ZERO)
        .map_err(|e| e.to_string())?;
    let list = tree.build_draw_list().map_err(|e| e.to_string())?;
    let mut canvas = Canvas::new(size[0], size[1]);
    canvas.draw(&list, |page| pages.get(&page));
    let pixels = canvas.into_rgba();
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&pixels, size[0], size[1], image::ExtendedColorType::Rgba8)
        .map_err(|e| e.to_string())?;
    Ok(png)
}
