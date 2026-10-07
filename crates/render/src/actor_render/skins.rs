//! Player skin arrays mirroring the main world's slot residency; only changed layers upload.
use std::sync::Arc;

use super::*;
use crate::actor::{ActorSkinResidency, SKIN_CLASS_SIDES};

struct GpuSkinClass {
    texture: Option<Texture>,
    view: TextureView,
    /// Admission held by each layer; 0 is never-written.
    admissions: Vec<u64>,
}

pub(super) struct GpuSkinArrays {
    classes: [GpuSkinClass; 4],
    /// Bound wherever a class has no layers, and by artwork pages, which never sample them.
    pub placeholder: TextureView,
    synced: Option<Arc<ActorSkinResidency>>,
    pub uploaded_bytes: u64,
}

fn skin_array(device: &RenderDevice, side: u32, layers: u32) -> Texture {
    device.create_texture(&TextureDescriptor {
        label: Some("player skin class array"),
        size: Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: layers,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8UnormSrgb,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn array_view(texture: &Texture) -> TextureView {
    texture.create_view(&TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    })
}

impl GpuSkinArrays {
    pub fn new(device: &RenderDevice) -> Self {
        let placeholder = array_view(&skin_array(device, 1, 1));
        Self {
            classes: std::array::from_fn(|_| GpuSkinClass {
                texture: None,
                view: placeholder.clone(),
                admissions: Vec::new(),
            }),
            placeholder,
            synced: None,
            uploaded_bytes: 0,
        }
    }

    pub fn view(&self, class: usize) -> &TextureView {
        &self.classes[class].view
    }

    pub fn is_synced(&self, residency: &Arc<ActorSkinResidency>) -> bool {
        self.synced
            .as_ref()
            .is_some_and(|synced| Arc::ptr_eq(synced, residency))
    }

    /// Matches every array to `residency`, copying kept layers on resize and writing only layers
    /// whose admission changed; returns whether any array view was replaced.
    pub fn sync(
        &mut self,
        residency: &Arc<ActorSkinResidency>,
        device: &RenderDevice,
        queue: &RenderQueue,
    ) -> bool {
        let mut views_changed = false;
        let mut copies: Option<bevy::render::render_resource::CommandEncoder> = None;
        for (class, gpu) in self.classes.iter_mut().enumerate() {
            let desired = &residency.classes[class];
            let side = SKIN_CLASS_SIDES[class] as u32;
            if desired.len() != gpu.admissions.len() {
                let texture =
                    (!desired.is_empty()).then(|| skin_array(device, side, desired.len() as u32));
                let mut admissions = vec![0; desired.len()];
                if let (Some(old), Some(new)) = (gpu.texture.as_ref(), texture.as_ref()) {
                    for (layer, admission) in admissions.iter_mut().enumerate() {
                        let kept = gpu.admissions.get(layer).copied().unwrap_or(0);
                        if kept == 0
                            || desired[layer].as_ref().map(|skin| skin.admission) != Some(kept)
                        {
                            continue;
                        }
                        let origin = bevy::render::render_resource::Origin3d {
                            x: 0,
                            y: 0,
                            z: layer as u32,
                        };
                        copies
                            .get_or_insert_with(|| {
                                device.create_command_encoder(&CommandEncoderDescriptor {
                                    label: Some("player skin array resize"),
                                })
                            })
                            .copy_texture_to_texture(
                                TexelCopyTextureInfo {
                                    texture: old,
                                    mip_level: 0,
                                    origin,
                                    aspect: default(),
                                },
                                TexelCopyTextureInfo {
                                    texture: new,
                                    mip_level: 0,
                                    origin,
                                    aspect: default(),
                                },
                                Extent3d {
                                    width: side,
                                    height: side,
                                    depth_or_array_layers: 1,
                                },
                            );
                        *admission = kept;
                    }
                }
                gpu.view = texture
                    .as_ref()
                    .map_or_else(|| self.placeholder.clone(), array_view);
                gpu.texture = texture;
                gpu.admissions = admissions;
                views_changed = true;
            }
            let Some(texture) = gpu.texture.as_ref() else {
                continue;
            };
            for (layer, resident) in desired.iter().enumerate() {
                let Some(resident) = resident else { continue };
                if gpu.admissions[layer] == resident.admission {
                    continue;
                }
                #[cfg(feature = "tracy")]
                let _span = bevy::log::info_span!(
                    "actor.skin_write",
                    class,
                    layer,
                    admission = resident.admission,
                    bytes = resident.texels.len(),
                )
                .entered();
                queue.write_texture(
                    TexelCopyTextureInfo {
                        texture,
                        mip_level: 0,
                        origin: bevy::render::render_resource::Origin3d {
                            x: 0,
                            y: 0,
                            z: layer as u32,
                        },
                        aspect: default(),
                    },
                    &resident.texels,
                    TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(side * 4),
                        rows_per_image: Some(side),
                    },
                    Extent3d {
                        width: side,
                        height: side,
                        depth_or_array_layers: 1,
                    },
                );
                gpu.admissions[layer] = resident.admission;
                self.uploaded_bytes += resident.texels.len() as u64;
            }
        }
        if let Some(copies) = copies {
            // Staged layer writes target only layers this copy leaves untouched.
            let command = copies.finish();
            #[cfg(feature = "tracy")]
            let _span = bevy::log::info_span!("actor.skin_copy_submit").entered();
            queue.submit([command]);
        }
        self.synced = Some(Arc::clone(residency));
        views_changed
    }
}
