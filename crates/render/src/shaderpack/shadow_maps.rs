//! The shadow pass's own targets, built once per pack.

use super::pipelines::GpuContext;
use super::plan::parse_clear_color;
use super::resources::{ConstantTexture, ShadowSlots};
use shaderpack::BufferId;
use wgpu::{TextureFormat, TextureUsages};

/// The shadow pass's targets: the depth attachment it draws into and the pack-wide shadow
/// maps programs sample, which it fills each frame.
pub(crate) struct ShadowMaps {
    pub slots: ShadowSlots,
    pub depth: wgpu::Texture,
    pub depth_view: wgpu::TextureView,
    /// The depth-compare `shadowtex` slot that is the attachment itself, which therefore
    /// needs no copy: `shadowtex0`, whose content is the attachment's after the pass.
    pub attachment_slot: Option<usize>,
    /// Clear colour per `shadowcolorN` in `slots.color`; `None` keeps the last frame's.
    pub color_clears: Vec<Option<[f64; 4]>>,
}

/// The shadow pass's depth attachment and the clear colour of each shadow colour buffer
/// (`shadowcolorNClear`, `shadowcolorNClearColor`; Iris clears them to white). With
/// `shadowtex0`, the depth-compare `shadowtex0` slot (absent when a shadow program samples
/// it), the pass draws into that texture instead of a texture of its own: `shadowtex0`
/// holds the depth with translucent casters, which is the attachment's final content.
pub(super) fn shadow_maps(
    gpu: &GpuContext,
    slots: ShadowSlots,
    directives: &shaderpack::BufferDirectives,
    shadowtex0: Option<(usize, &ConstantTexture)>,
) -> ShadowMaps {
    let side = slots.resolution.max(1);
    let (depth, depth_view) = match shadowtex0 {
        Some((_, constant)) => (constant.texture.clone(), constant.view.clone()),
        None => {
            let depth = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("shader pack shadow depth"),
                size: wgpu::Extent3d {
                    width: side,
                    height: side,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: TextureFormat::Depth32Float,
                usage: TextureUsages::RENDER_ATTACHMENT
                    | TextureUsages::TEXTURE_BINDING
                    | TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = depth.create_view(&Default::default());
            (depth, view)
        }
    };
    let color_clears = slots
        .color
        .iter()
        .map(|(index, ..)| {
            let settings = directives.buffers.get(&BufferId::ShadowColor(*index));
            match settings.and_then(|s| s.clear) {
                Some(false) => None,
                _ => Some(
                    settings
                        .and_then(|s| s.clear_color.as_deref())
                        .and_then(parse_clear_color)
                        .unwrap_or([1.0; 4]),
                ),
            }
        })
        .collect();
    ShadowMaps {
        slots,
        depth,
        depth_view,
        attachment_slot: shadowtex0.map(|(slot, _)| slot),
        color_clears,
    }
}
