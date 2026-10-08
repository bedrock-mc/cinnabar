//! One reusable target-compatible depth surface per live view, never world depth.
use super::*;
use bevy::render::render_resource::{
    Extent3d, Texture, TextureDescriptor, TextureDimension, TextureUsages, TextureView,
    TextureViewDescriptor,
};

pub(super) struct UiModelDepth {
    _texture: Texture,
    pub(super) view: TextureView,
    size: Extent3d,
    samples: u32,
}

#[derive(Default, Resource)]
pub(super) struct UiModelDepths {
    device: Option<wgpu::Device>,
    pub(super) views: std::collections::BTreeMap<Entity, UiModelDepth>,
}

impl UiModelDepths {
    pub(super) fn synchronize_device(&mut self, device: &RenderDevice) {
        if self.device.as_ref() != Some(device.wgpu_device()) {
            self.views.clear();
            self.device = Some(device.wgpu_device().clone());
        }
    }

    pub(super) fn ensure(
        &mut self,
        owner: Entity,
        layer: &super::composite::UiLayerTexture,
        device: &RenderDevice,
    ) {
        let color = &layer.texture;
        let size = color.size();
        let samples = color.sample_count();
        self.ensure_surface(owner, size, samples, device);
    }

    fn ensure_surface(
        &mut self,
        owner: Entity,
        size: Extent3d,
        samples: u32,
        device: &RenderDevice,
    ) {
        if self
            .views
            .get(&owner)
            .is_some_and(|depth| depth.size == size && depth.samples == samples)
        {
            return;
        }
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("reusable isolated JSON-UI model reverse-Z depth"),
            size,
            mip_level_count: 1,
            sample_count: samples,
            dimension: TextureDimension::D2,
            format: CORE_3D_DEPTH_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&TextureViewDescriptor::default());
        self.views.insert(
            owner,
            UiModelDepth {
                _texture: texture,
                view,
                size,
                samples,
            },
        );
    }

    pub(super) fn compatible(
        &self,
        owner: Entity,
        layer: &super::composite::UiLayerTexture,
    ) -> Option<&UiModelDepth> {
        let color = &layer.texture;
        self.views
            .get(&owner)
            .filter(|depth| depth.size == color.size() && depth.samples == color.sample_count())
    }
}

#[derive(Default)]
pub(super) struct ModelDepthLifetime {
    scope: Option<u32>,
    cleared: bool,
}

impl ModelDepthLifetime {
    pub(super) fn enter(&mut self, scope: Option<u32>) {
        if self.scope != scope {
            self.scope = scope;
            self.cleared = false;
        }
    }
    pub(super) fn cleared(&self) -> bool {
        self.cleared
    }
    pub(super) fn encoded(&mut self) {
        self.cleared = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_model_depth_lifetime_clears_once_per_control_not_per_material() {
        let mut lifetime = ModelDepthLifetime::default();
        lifetime.enter(Some(7));
        assert!(!lifetime.cleared());
        // A depth-disabled material does not consume the clear.
        lifetime.enter(Some(7));
        assert!(!lifetime.cleared());
        lifetime.encoded();
        lifetime.enter(Some(7));
        assert!(
            lifetime.cleared(),
            "translucent/read-only layer retains opaque depth"
        );
        lifetime.enter(None);
        lifetime.enter(Some(8));
        assert!(
            !lifetime.cleared(),
            "a later control cannot inherit an earlier model's depth"
        );
    }

    #[test]
    fn ui_model_depth_surface_reuses_one_allocation_and_tracks_extent_samples() {
        let world = super::super::ordered_command_tests::binding_world();
        let device = world.resource::<RenderDevice>();
        let owner = Entity::from_bits(7);
        let size = Extent3d {
            width: 64,
            height: 32,
            depth_or_array_layers: 1,
        };
        let mut depths = UiModelDepths::default();
        depths.synchronize_device(device);
        depths.ensure_surface(owner, size, 1, device);
        let texture = depths.views[&owner]._texture.id();
        for _ in 0..100 {
            depths.ensure_surface(owner, size, 1, device);
        }
        assert_eq!(depths.views.len(), 1);
        assert_eq!(depths.views[&owner]._texture.id(), texture);
        depths.ensure_surface(owner, size, 4, device);
        assert_eq!(depths.views[&owner].samples, 4);
        assert_ne!(depths.views[&owner]._texture.id(), texture);
        depths.ensure_surface(owner, Extent3d { width: 128, ..size }, 4, device);
        assert_eq!(depths.views[&owner].size.width, 128);
    }

    #[test]
    fn ui_model_depth_matches_gamma_layer_not_world_msaa() {
        let world = super::super::ordered_command_tests::binding_world();
        let device = world.resource::<RenderDevice>();
        let owner = Entity::from_bits(7);
        let layer = |width| {
            let texture = device.create_texture(&TextureDescriptor {
                label: Some("test gamma UI layer"),
                size: Extent3d {
                    width,
                    height: 32,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: super::super::composite::UI_LAYER_FORMAT,
                usage: TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let default_view = texture.create_view(&TextureViewDescriptor::default());
            super::super::composite::UiLayerTexture::detached(texture, default_view)
        };
        let original = layer(64);
        let resized = layer(128);
        let mut depths = UiModelDepths::default();
        depths.synchronize_device(device);
        depths.ensure(owner, &original, device);
        let first = depths.compatible(owner, &original).unwrap()._texture.id();
        assert_eq!(depths.compatible(owner, &original).unwrap().samples, 1);
        assert!(depths.compatible(owner, &resized).is_none());
        depths.ensure(owner, &resized, device);
        assert_ne!(
            depths.compatible(owner, &resized).unwrap()._texture.id(),
            first
        );
        assert_eq!(depths.compatible(owner, &resized).unwrap().samples, 1);
        assert!(depths.compatible(owner, &original).is_none());
    }
}
