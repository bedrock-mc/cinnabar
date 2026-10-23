//! Player skin arrays mirroring the main world's slot residency; only changed layers upload.
use std::sync::Arc;

use super::*;
use render_model::ActorSkinResidency;
use render_model::SKIN_CLASS_SIDES;

use crate::actor::gpu::ArtworkRawViews;

#[derive(Clone)]
struct GpuSkinClass {
    texture: Option<Texture>,
    view: TextureView,
    /// Admission held by each layer; 0 is never-written.
    admissions: Vec<u64>,
    /// The raw-view setting `texture` was created with.
    raw_views: ArtworkRawViews,
}

pub(super) struct GpuSkinArrays {
    classes: [GpuSkinClass; 4],
    /// Bound wherever a class has no layers, and by artwork pages, which never sample them.
    pub placeholder: TextureView,
    synced: Option<Arc<ActorSkinResidency>>,
    failed: Option<Arc<ActorSkinResidency>>,
    pub uploaded_bytes: u64,
    /// The raw-view setting arrays are created with; arrays created under another setting
    /// are replaced by GPU copies on the next sync.
    raw_views: ArtworkRawViews,
}

/// Allocates a class array before any view or layer copy can reference it.
fn skin_array(
    device: &RenderDevice,
    side: u32,
    layers: u32,
    raw_views: ArtworkRawViews,
) -> Option<Texture> {
    gpu_allocation::checked(
        device,
        "player skin class array",
        u64::from(side) * u64::from(side) * u64::from(layers) * 4,
        || {
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
                usage: TextureUsages::TEXTURE_BINDING
                    | TextureUsages::COPY_DST
                    | TextureUsages::COPY_SRC,
                view_formats: raw_views.view_formats(),
            })
        },
    )
}

/// Creates an array binding only for a successfully allocated texture.
fn array_view(texture: &Texture) -> TextureView {
    texture.create_view(&TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    })
}

impl GpuSkinArrays {
    /// Creates empty class bindings, or disables actors when even the placeholder fails.
    pub fn new(device: &RenderDevice) -> Option<Self> {
        let texture = skin_array(device, 1, 1, ArtworkRawViews::default())?;
        let placeholder =
            gpu_allocation::checked(device, "actor skin placeholder view", 0, || {
                array_view(&texture)
            })?;
        Some(Self {
            classes: std::array::from_fn(|_| GpuSkinClass {
                texture: None,
                view: placeholder.clone(),
                admissions: Vec::new(),
                raw_views: ArtworkRawViews::default(),
            }),
            placeholder,
            synced: None,
            failed: None,
            uploaded_bytes: 0,
            raw_views: ArtworkRawViews::default(),
        })
    }

    /// Schedules every array for recreation under `raw_views` when it changes.
    pub fn set_raw_views(&mut self, raw_views: ArtworkRawViews) {
        if self.raw_views != raw_views {
            self.raw_views = raw_views;
            self.synced = None;
            self.failed = None;
        }
    }

    pub fn view(&self, class: usize) -> &TextureView {
        &self.classes[class].view
    }

    #[cfg(feature = "shaderpacks")]
    /// The array texture of resolution class `class` for raw views, once allocated to allow them.
    pub fn raw_texture(&self, class: usize) -> Option<&Texture> {
        let class = self.classes.get(class)?;
        class.texture.as_ref().filter(|_| class.raw_views.0)
    }

    pub fn is_synced(&self, residency: &Arc<ActorSkinResidency>) -> bool {
        self.synced
            .as_ref()
            .is_some_and(|synced| Arc::ptr_eq(synced, residency))
    }

    /// Avoids retrying a failed immutable publication every rendered frame.
    pub fn failed(&self, residency: &Arc<ActorSkinResidency>) -> bool {
        self.failed
            .as_ref()
            .is_some_and(|failed| Arc::ptr_eq(failed, residency))
    }

    /// Matches every array to `residency`, copying kept layers on resize and writing only layers
    /// whose admission changed; returns whether any array view was replaced.
    pub fn sync(
        &mut self,
        residency: &Arc<ActorSkinResidency>,
        device: &RenderDevice,
        queue: &RenderQueue,
    ) -> bool {
        if self.failed(residency) {
            return false;
        }
        let previous = self.classes.clone();
        let result = gpu_allocation::checked(device, "actor skin transfers", 0, || {
            self.try_sync(residency, device, queue)
        })
        .flatten();
        match result {
            Some(changed) => {
                self.failed = None;
                changed
            }
            None => {
                self.classes = previous;
                for class in &mut self.classes {
                    class.admissions.fill(0);
                }
                self.synced = None;
                self.failed = Some(Arc::clone(residency));
                false
            }
        }
    }

    /// Publishes only valid arrays; an interrupted upload leaves the publication unready.
    fn try_sync(
        &mut self,
        residency: &Arc<ActorSkinResidency>,
        device: &RenderDevice,
        queue: &RenderQueue,
    ) -> Option<bool> {
        let mut views_changed = false;
        let mut copies: Option<bevy::render::render_resource::CommandEncoder> = None;
        for (class, gpu) in self.classes.iter_mut().enumerate() {
            let desired = &residency.classes[class];
            let side = SKIN_CLASS_SIDES[class] as u32;
            if desired.len() != gpu.admissions.len()
                || (gpu.texture.is_some() && gpu.raw_views != self.raw_views)
            {
                let texture = if desired.is_empty() {
                    None
                } else {
                    Some(skin_array(
                        device,
                        side,
                        desired.len() as u32,
                        self.raw_views,
                    )?)
                };
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
                gpu.raw_views = self.raw_views;
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
                gpu_allocation::checked(
                    device,
                    "actor skin upload",
                    resident.texels.len() as u64,
                    || {
                        queue.profiled_write_texture(
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
                        )
                    },
                )?;
                gpu.admissions[layer] = resident.admission;
                self.uploaded_bytes += resident.texels.len() as u64;
            }
        }
        if let Some(copies) = copies {
            // Staged layer writes target only layers this copy leaves untouched.
            let command = copies.finish();
            #[cfg(feature = "tracy")]
            let _span = bevy::log::info_span!("actor.skin_copy_submit").entered();
            gpu_allocation::checked(device, "actor skin copy", 0, || queue.submit([command]))?;
        }
        self.synced = Some(Arc::clone(residency));
        Some(views_changed)
    }
}
