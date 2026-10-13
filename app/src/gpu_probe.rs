//! Minimal GPU helper modes run before lifecycle setup, assets and the Bevy app.

pub(crate) mod device;
#[cfg(not(target_os = "android"))]
mod window;

use anyhow::{Context, Result, bail};
use launcher::graphics_backend::{AdapterKey, Backend, INVENTORY_FLAG, Inventory, PROBE_FLAG};

/// A helper request either inventories one API or presents on one exact physical adapter.
#[derive(Debug)]
pub enum Request {
    Inventory(Backend),
    Probe(Backend, AdapterKey),
}

impl Request {
    /// Validates helper arguments without touching a graphics driver or user files.
    pub fn parse(args: &[String]) -> Result<Self> {
        match args {
            [flag, backend] if flag == INVENTORY_FLAG => Ok(Self::Inventory(
                Backend::parse(backend).context("unknown inventory backend")?,
            )),
            [flag, backend, key] if flag == PROBE_FLAG => Ok(Self::Probe(
                Backend::parse(backend).context("unknown probe backend")?,
                serde_json::from_str(key).context("invalid adapter key")?,
            )),
            _ => bail!("expected --gpu-inventory <backend> or --gpu-probe <backend> <adapter-key>"),
        }
    }

    /// The one API this helper may load.
    fn backend(&self) -> Backend {
        match self {
            Self::Inventory(backend) | Self::Probe(backend, _) => *backend,
        }
    }
}

/// Runs a helper process with a hidden window, returning zero only after completed GPU work.
#[cfg(not(target_os = "android"))]
pub fn dispatch(args: &[String]) -> i32 {
    match Request::parse(args).and_then(window::run) {
        Ok(output) => {
            println!("{output}");
            0
        }
        Err(error) => {
            eprintln!("GPU helper failed: {error:#}");
            1
        }
    }
}

/// Executes a request against the caller's native surface without loading game assets.
pub async fn execute(
    request: &Request,
    display: Option<Box<dyn wgpu::wgt::WgpuHasDisplayHandle>>,
    surface_target: Option<wgpu::SurfaceTarget<'static>>,
) -> Result<String> {
    let mut settings = bevy::render::settings::WgpuSettings::default();
    device::configure_render_settings(&mut settings);
    let instance = device::instance(request.backend(), &settings, display);
    let key = match request {
        Request::Probe(_, key) => Some(key),
        Request::Inventory(_) => None,
    };
    let adapter = device::select_adapter(&instance, &settings, key).await?;
    let info = adapter.get_info();
    device::log(format_args!("GPU helper selected adapter: {info:?}"));
    if matches!(request, Request::Inventory(_)) {
        let adapters = instance
            .enumerate_adapters(device::flags(request.backend()))
            .await
            .iter()
            .map(device::identity)
            .collect();
        return Ok(serde_json::to_string(&Inventory {
            selected: device::identity(&adapter),
            adapters,
        })?);
    }
    let (device, queue) = device::request_device(&adapter, &settings).await?;
    let surface =
        instance.create_surface(surface_target.context("GPU probe requires a native surface")?)?;
    present(&surface, &adapter, &device, &queue)?;
    Ok(String::new())
}

/// Clears and presents one small surface, then waits for the submitted GPU work to finish.
fn present(
    surface: &wgpu::Surface<'_>,
    adapter: &wgpu::Adapter,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<()> {
    let mut config = surface
        .get_default_config(adapter, 16, 16)
        .context("GPU probe surface incompatible with selected adapter")?;
    config.present_mode = wgpu::PresentMode::Fifo;
    surface.configure(device, &config);
    let frame = match surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(frame)
        | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
        other => bail!("GPU probe failed to acquire a frame: {other:?}"),
    };
    let view = frame.texture.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("GPU stability probe"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
    }
    queue.submit([encoder.finish()]);
    queue.present(frame);
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .context("GPU probe completion")?;
    Ok(())
}

/// Runs an Android probe on the service's owned native window; it never starts a Bevy app.
///
/// # Safety
/// The caller must retain a live ANativeWindow reference until this function returns.
#[cfg(target_os = "android")]
pub unsafe fn execute_android(
    args: &[String],
    window: std::ptr::NonNull<std::ffi::c_void>,
) -> Result<String> {
    let request = Request::parse(args)?;
    let mut settings = bevy::render::settings::WgpuSettings::default();
    device::configure_render_settings(&mut settings);
    let instance = device::instance(request.backend(), &settings, None);
    let key = match &request {
        Request::Probe(_, key) => Some(key),
        _ => None,
    };
    let adapter = bevy::tasks::block_on(device::select_adapter(&instance, &settings, key))?;
    if matches!(request, Request::Inventory(_)) {
        let adapters =
            bevy::tasks::block_on(instance.enumerate_adapters(device::flags(request.backend())))
                .iter()
                .map(device::identity)
                .collect();
        return Ok(serde_json::to_string(&Inventory {
            selected: device::identity(&adapter),
            adapters,
        })?);
    }
    let (device, queue) = bevy::tasks::block_on(device::request_device(&adapter, &settings))?;
    // SAFETY: The JNI caller retains the ANativeWindow reference through presentation and teardown.
    let surface = unsafe {
        instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: raw_window_handle::RawDisplayHandle::Android(
                raw_window_handle::AndroidDisplayHandle::new(),
            ),
            raw_window_handle: raw_window_handle::RawWindowHandle::AndroidNdk(
                raw_window_handle::AndroidNdkWindowHandle::new(window),
            ),
        })
    }?;
    present(&surface, &adapter, &device, &queue)?;
    Ok(String::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn helper_dispatch_rejects_missing_keys_and_extra_client_flags() {
        assert!(Request::parse(&[PROBE_FLAG.into(), "vulkan".into()]).is_err());
        assert!(
            Request::parse(&[INVENTORY_FLAG.into(), "vulkan".into(), "--assets".into()]).is_err()
        );
        assert!(Request::parse(&[INVENTORY_FLAG.into(), "unknown".into()]).is_err());
        assert!(matches!(
            Request::parse(&[INVENTORY_FLAG.into(), "vulkan".into()]).unwrap(),
            Request::Inventory(Backend::Vulkan)
        ));
    }
}
