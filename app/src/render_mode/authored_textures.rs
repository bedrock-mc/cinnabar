//! Optional authored maps load once off the frame thread, after Enhanced is selected.

use std::{
    io,
    sync::{Arc, Mutex, mpsc},
};

use assets::{MaterialKeys, RuntimeAssets};
use bevy::prelude::*;
use bevy::render::renderer::RenderDevice;
use render::{ChunkTextureAssets, ChunkTextureReload, EnhancedTextureAssets};
use render_model::ENHANCED_RENDERING_ENABLED;
use ui::RenderMode;

use crate::settings_runtime::RuntimeSettings;

type AuthoredMaps = Option<Arc<EnhancedTextureAssets>>;
type Receiver = mpsc::Receiver<AuthoredMaps>;

enum Loading {
    Unrequested,
    Pending(Mutex<Receiver>),
    Complete(AuthoredMaps),
}

#[derive(Resource)]
pub(crate) struct AuthoredTextureLoading {
    base: Arc<RuntimeAssets>,
    keys: Option<MaterialKeys>,
    loading: Loading,
}

impl AuthoredTextureLoading {
    pub(crate) fn new(base: Arc<RuntimeAssets>, keys: Option<MaterialKeys>) -> Self {
        Self {
            base,
            keys,
            loading: Loading::Unrequested,
        }
    }

    fn begin(
        &mut self,
        mode: RenderMode,
        spawn: impl FnOnce(Arc<RuntimeAssets>, MaterialKeys) -> io::Result<Receiver>,
    ) {
        if !ENHANCED_RENDERING_ENABLED
            || mode != RenderMode::Enhanced
            || !matches!(self.loading, Loading::Unrequested)
        {
            return;
        }
        let Some(keys) = self.keys.take() else {
            self.loading = Loading::Complete(None);
            return;
        };
        self.loading = match spawn(Arc::clone(&self.base), keys) {
            Ok(receiver) => Loading::Pending(Mutex::new(receiver)),
            Err(error) => {
                warn!(?error, "authored Enhanced texture worker could not start");
                Loading::Complete(None)
            }
        };
    }

    fn poll(&mut self) {
        let Loading::Pending(receiver) = &self.loading else {
            return;
        };
        let result = receiver
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .try_recv();
        match result {
            Ok(maps) => self.loading = Loading::Complete(maps),
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                warn!("authored Enhanced texture worker stopped; retained carrier materials");
                self.loading = Loading::Complete(None);
            }
        }
    }

    fn selected(&self, mode: RenderMode) -> AuthoredMaps {
        if mode == RenderMode::Enhanced
            && let Loading::Complete(maps) = &self.loading
        {
            maps.clone()
        } else {
            None
        }
    }

    /// Drops unsupported cached maps so future server carriers cannot inherit them.
    fn discard_unsupported(&mut self, limits: &wgpu::Limits) {
        if let Loading::Complete(Some(maps)) = &self.loading
            && !maps.fits_device_limits(limits)
        {
            warn!("unsupported authored Enhanced maps; retained carrier materials");
            self.loading = Loading::Complete(None);
        }
    }
}

/// Loads and validates optional maps before publishing them to the frame thread.
fn spawn(
    base: Arc<RuntimeAssets>,
    keys: MaterialKeys,
    limits: Option<wgpu::Limits>,
) -> io::Result<Receiver> {
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("Enhanced terrain materials".to_owned())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let maps = crate::asset_startup::load_optional_enhanced_textures(&base, &keys).filter(
                |maps| {
                    let supported = limits
                        .as_ref()
                        .is_none_or(|limits| maps.fits_device_limits(limits));
                    if !supported {
                        warn!("unsupported authored Enhanced maps; retained carrier materials");
                    }
                    supported
                },
            );
            let _ = sender.send(maps);
        })?;
    Ok(receiver)
}

fn apply(textures: &mut ChunkTextureAssets, maps: AuthoredMaps) {
    let unchanged = match (textures.enhanced(), maps.as_ref()) {
        (Some(current), Some(next)) => Arc::ptr_eq(current, next),
        (None, None) => true,
        _ => false,
    };
    if !unchanged {
        *textures = textures.with_updated_enhanced(maps);
    }
}

pub(super) fn update_authored_textures(
    settings: Res<RuntimeSettings>,
    loading: Option<ResMut<AuthoredTextureLoading>>,
    textures: Option<ResMut<ChunkTextureAssets>>,
    reload: Option<Res<ChunkTextureReload>>,
    device: Option<Res<RenderDevice>>,
) {
    let (Some(mut loading), Some(mut textures)) = (loading, textures) else {
        return;
    };
    let mode = settings.user_settings_update().1.video.render_mode;
    let limits = device.as_ref().map(|device| device.limits());
    loading.begin(mode, |base, keys| spawn(base, keys, limits.clone()));
    loading.poll();
    if let Some(limits) = &limits {
        loading.discard_unsupported(limits);
    }
    if reload.is_some_and(|reload| reload.geometry_pending()) {
        return;
    }
    apply(&mut textures, loading.selected(mode));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn maps() -> Arc<EnhancedTextureAssets> {
        let array = || assets::TextureArray {
            layers: 1,
            mips: vec![assets::TextureMip {
                size: 1,
                rgba8: vec![128, 128, 255, 255].into_boxed_slice(),
            }]
            .into_boxed_slice(),
        };
        Arc::new(
            EnhancedTextureAssets::new(
                [array(), array()],
                [array(), array()],
                [array(), array()],
                vec![u32::MAX; assets::MAX_TEXTURE_PAGES * assets::MAX_TEXTURE_LAYERS]
                    .into_boxed_slice(),
            )
            .unwrap(),
        )
    }

    #[test]
    fn vanilla_selection_never_starts_optional_texture_work() {
        let base = Arc::new(RuntimeAssets::diagnostic());
        let mut loading = AuthoredTextureLoading::new(base, Some(MaterialKeys::default()));
        loading.begin(RenderMode::Vanilla, |_, _| {
            panic!("vanilla must not inspect or load optional authored packs")
        });
        assert!(matches!(loading.loading, Loading::Unrequested));
        assert!(loading.selected(RenderMode::Vanilla).is_none());
    }

    #[cfg(feature = "enhanced")]
    #[test]
    fn enhanced_selection_loads_once_and_runtime_mode_toggles_reuse_maps() {
        let base = Arc::new(RuntimeAssets::diagnostic());
        let mut loading = AuthoredTextureLoading::new(base.clone(), Some(MaterialKeys::default()));
        let authored = maps();
        loading.begin(RenderMode::Enhanced, |worker_base, _| {
            assert!(Arc::ptr_eq(&base, &worker_base));
            let (sender, receiver) = mpsc::channel();
            sender.send(Some(authored.clone())).unwrap();
            Ok(receiver)
        });
        loading.poll();
        assert!(Arc::ptr_eq(
            &loading.selected(RenderMode::Enhanced).unwrap(),
            &authored
        ));
        assert!(loading.selected(RenderMode::Vanilla).is_none());
        loading.begin(RenderMode::Enhanced, |_, _| {
            panic!("completed authored maps must not decode again")
        });
        assert!(loading.selected(RenderMode::Enhanced).is_some());
    }

    #[test]
    fn attaching_maps_preserves_current_server_assets_and_revision() {
        let server_runtime = Arc::new(RuntimeAssets::diagnostic());
        let mut textures = ChunkTextureAssets::with_revision(server_runtime.clone(), 17);
        let original_identity = textures.identity();
        let authored = maps();
        apply(&mut textures, Some(authored.clone()));
        assert!(Arc::ptr_eq(textures.assets(), &server_runtime));
        assert!(Arc::ptr_eq(textures.enhanced().unwrap(), &authored));
        let enhanced_identity = textures.identity();
        apply(&mut textures, Some(authored));
        assert_eq!(textures.identity(), enhanced_identity);
        apply(&mut textures, None);
        assert_eq!(textures.identity(), original_identity);
    }

    #[cfg(feature = "enhanced")]
    #[test]
    fn unsupported_maps_are_discarded_before_server_assets_can_inherit_them() {
        let base = Arc::new(RuntimeAssets::diagnostic());
        let array = assets::TextureArray {
            layers: 2,
            mips: Box::new([assets::TextureMip {
                size: 1,
                rgba8: vec![128; 8].into_boxed_slice(),
            }]),
        };
        let authored = Arc::new(
            EnhancedTextureAssets::new(
                [array.clone(), array.clone()],
                [array.clone(), array.clone()],
                [array.clone(), array],
                vec![u32::MAX; assets::MAX_TEXTURE_PAGES * assets::MAX_TEXTURE_LAYERS]
                    .into_boxed_slice(),
            )
            .unwrap(),
        );
        let mut loading = AuthoredTextureLoading::new(base.clone(), None);
        loading.loading = Loading::Complete(Some(authored));
        let (device, _) = wgpu::Device::noop(&wgpu::DeviceDescriptor {
            required_limits: wgpu::Limits {
                max_texture_array_layers: 1,
                ..Default::default()
            },
            ..Default::default()
        });
        let mut settings = RuntimeSettings::default();
        crate::render_mode::set_render_mode(&mut settings, RenderMode::Enhanced);
        let mut app = App::new();
        app.insert_resource(settings)
            .insert_resource(loading)
            .insert_resource(bevy::render::renderer::RenderDevice::from(device))
            .insert_resource(ChunkTextureAssets::new(base))
            .add_systems(Update, update_authored_textures);
        app.update();
        assert!(
            app.world()
                .resource::<ChunkTextureAssets>()
                .enhanced()
                .is_none()
        );
        assert!(matches!(
            app.world().resource::<AuthoredTextureLoading>().loading,
            Loading::Complete(None),
        ));
        app.update();
        assert!(
            app.world()
                .resource::<ChunkTextureAssets>()
                .enhanced()
                .is_none()
        );
    }
}
