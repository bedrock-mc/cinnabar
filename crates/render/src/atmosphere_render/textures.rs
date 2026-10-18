use super::*;
use render_scene::{AtmosphereFrame, AtmosphereTextureAssets, WeatherTextureAssets};

/// Numbered sky images filter their encoded channels before the fragment transfer.
pub(crate) const SKYBOX_TEXTURE_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;

/// Uploads the active dimension and invalidates bindings only when its assets change.
pub(super) fn prepare_atmosphere_textures(
    requested: Res<AtmosphereTextureAssets>,
    java: Res<render_scene::JavaWorldVisuals>,
    frame: Option<Res<AtmosphereFrame>>,
    weather_textures: Option<Res<WeatherTextureAssets>>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut gpu: ResMut<AtmosphereGpu>,
) {
    let Some(runtime) = requested.runtime() else {
        gpu.prepared = None;
        gpu.bind_group = None;
        gpu.bound_asset_identity = None;
        return;
    };
    let end_sky = weather_textures.as_deref().and_then(|assets| {
        assets
            .end_sky()
            .map(|textures| (assets.identity(), textures))
    });
    let end_sky_identity = end_sky.map(|(identity, _)| identity);
    let dimension = crate::pack_skybox::dimension(frame.as_deref().copied().unwrap_or_default());
    let identity = java.texture_identity(requested.identity());
    if gpu.prepared.as_ref().is_some_and(|prepared| {
        prepared.identity == identity
            && prepared.end_sky_identity == end_sky_identity
            && prepared.dimension == dimension
    }) {
        return;
    }

    let sun = runtime
        .texture(AtmosphereRole::Sun)
        .expect("validated MCBEATM2 always contains the sun texture");
    let moon_phases = runtime
        .texture(AtmosphereRole::MoonPhases)
        .expect("validated MCBEATM2 always contains the moon atlas");
    let sun = java.sky_texture(sun);
    let moon_phases = java.sky_texture(moon_phases);
    let (sun_texture, sun_view) =
        upload_atmosphere_texture(&render_device, &render_queue, sun, "pinned vanilla sun");
    let (moon_texture, moon_view) = upload_atmosphere_texture(
        &render_device,
        &render_queue,
        moon_phases,
        "pinned vanilla moon phases",
    );
    let (end_sky_texture, end_sky_view) = match end_sky {
        Some((_, image)) => upload_rgba(
            &render_device,
            &render_queue,
            image.width,
            image.height,
            &image.rgba8,
            "vanilla End sky",
        ),
        None => upload_rgba(
            &render_device,
            &render_queue,
            1,
            1,
            &[0, 0, 0, 255],
            "absent End sky fallback",
        ),
    };
    let sampler = render_device.create_sampler(&SamplerDescriptor {
        label: Some("pinned vanilla atmosphere repeat sampler"),
        address_mode_u: AddressMode::Repeat,
        address_mode_v: AddressMode::Repeat,
        address_mode_w: AddressMode::Repeat,
        mag_filter: FilterMode::Nearest,
        min_filter: FilterMode::Nearest,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..default()
    });
    let skybox = requested.skybox(dimension);
    let uploaded: [(Texture, TextureView); assets::SKYBOX_FACE_COUNT] =
        std::array::from_fn(|face| {
            upload_skybox_face(
                &render_device,
                &render_queue,
                skybox.and_then(|skybox| skybox.faces[face].as_ref()),
            )
        });
    let (_skybox_textures, skybox_views) = uploaded.into_iter().unzip::<_, _, Vec<_>, Vec<_>>();
    let skybox_sampler = render_device.create_sampler(&skybox_sampler_descriptor());
    gpu.prepared = Some(PreparedAtmosphereAssets {
        identity,
        end_sky_identity,
        dimension,
        _skybox_textures: _skybox_textures.try_into().unwrap(),
        skybox_views: skybox_views.try_into().unwrap(),
        skybox_sampler,
        _textures: [sun_texture, moon_texture, end_sky_texture],
        views: [sun_view, moon_view, end_sky_view],
        sampler,
    });
    gpu.bind_group = None;
    gpu.view_buffer_id = None;
    gpu.bound_asset_identity = None;
    #[cfg(test)]
    {
        gpu.upload_count += 1;
    }
}

/// Uploads an authored face or a transparent placeholder with the cube's encoded format.
fn upload_skybox_face(
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
    image: Option<&assets::SkyboxFace>,
) -> (Texture, TextureView) {
    let (size, pixels, label): ([u32; 2], &[u8], _) = match image {
        Some(image) => (
            [image.width, image.height],
            &image.rgba8,
            "pack skybox face",
        ),
        None => ([1, 1], &[0; 4], "absent pack skybox face"),
    };
    upload_pixels(
        render_device,
        render_queue,
        size,
        pixels,
        label,
        SKYBOX_TEXTURE_FORMAT,
    )
}

/// Samples each independently authored face linearly while retaining its edge texels.
pub(crate) fn skybox_sampler_descriptor() -> SamplerDescriptor<'static> {
    SamplerDescriptor {
        label: Some("pack skybox clamp sampler"),
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        address_mode_w: AddressMode::ClampToEdge,
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..default()
    }
}

/// Uploads a validated carrier image using its decoded dimensions.
fn upload_atmosphere_texture(
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
    texture: &AtmosphereTexture,
    label: &'static str,
) -> (Texture, TextureView) {
    upload_rgba(
        render_device,
        render_queue,
        texture.width,
        texture.height,
        &texture.rgba8,
        label,
    )
}

/// Uploads straight-alpha sRGB pixels as a single, uncompressed texture level.
pub(crate) fn upload_rgba(
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
    width: u32,
    height: u32,
    rgba8: &[u8],
    label: &'static str,
) -> (Texture, TextureView) {
    upload_pixels(
        render_device,
        render_queue,
        [width, height],
        rgba8,
        label,
        TextureFormat::Rgba8UnormSrgb,
    )
}

/// Uploads one bounded image using the caller's colour-space contract.
fn upload_pixels(
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
    [width, height]: [u32; 2],
    rgba8: &[u8],
    label: &'static str,
    format: TextureFormat,
) -> (Texture, TextureView) {
    let gpu_texture = render_device.create_texture_with_data(
        render_queue,
        &TextureDescriptor {
            label: Some(label),
            size: Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        },
        TextureDataOrder::LayerMajor,
        rgba8,
    );
    let view = gpu_texture.create_view(&TextureViewDescriptor {
        label: Some(label),
        dimension: Some(TextureViewDimension::D2),
        ..default()
    });
    (gpu_texture, view)
}
