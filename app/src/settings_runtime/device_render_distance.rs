//! Reads the selected graphics device once before publishing menu settings.

use bevy::{
    prelude::*,
    render::renderer::{RenderAdapter, RenderDevice},
};

#[cfg(target_os = "macos")]
mod macos;

/// Applies installed memory to untouched preferences after the renderer has selected its adapter.
pub(crate) fn initialize_render_distance(
    adapter: Option<Res<RenderAdapter>>,
    device: Option<Res<RenderDevice>>,
    mut menu: ResMut<crate::menu::MenuRuntime>,
) {
    let graphics_bytes = adapter
        .as_deref()
        .and_then(|adapter| dedicated_graphics_bytes(adapter, device.as_deref()));
    if graphics_bytes.is_none() {
        warn!("graphics memory unavailable; render distance uses the shared-memory recommendation");
    }
    menu.sync_render_distance_device(ui::RenderDistanceDevice {
        physical_memory_bytes: crate::global_resources::memory::physical_bytes(),
        dedicated_graphics_memory_bytes: graphics_bytes.unwrap_or(0),
        use_full_graphics_memory: false,
    });
}

/// Reads dedicated memory from the selected backend, never from a different enumerated GPU.
#[cfg(target_os = "windows")]
fn dedicated_graphics_bytes(
    adapter: &RenderAdapter,
    _device: Option<&RenderDevice>,
) -> Option<u64> {
    // SAFETY: the borrowed adapter stays alive and only a read-only DXGI description is queried.
    unsafe {
        let native = adapter.as_hal::<wgpu::hal::api::Dx12>()?;
        native
            .raw_adapter()
            .as_raw()
            .GetDesc1()
            .ok()
            .map(|desc| desc.DedicatedVideoMemory as u64)
    }
}

/// Uses device-local heaps only when they belong to a discrete Vulkan adapter.
#[cfg(target_os = "linux")]
fn dedicated_graphics_bytes(
    adapter: &RenderAdapter,
    _device: Option<&RenderDevice>,
) -> Option<u64> {
    if adapter.get_info().device_type != wgpu::DeviceType::DiscreteGpu {
        return Some(0);
    }
    // SAFETY: the borrowed adapter retains its instance and physical device for this read-only query.
    unsafe {
        let native = adapter.as_hal::<wgpu::hal::api::Vulkan>()?;
        let memory = native
            .shared_instance()
            .raw_instance()
            .get_physical_device_memory_properties(native.raw_physical_device());
        Some(
            memory.memory_heaps[..memory.memory_heap_count as usize]
                .iter()
                .filter(|heap| heap.flags.contains(ash::vk::MemoryHeapFlags::DEVICE_LOCAL))
                .map(|heap| heap.size)
                .sum(),
        )
    }
}

/// Unified Metal devices have no dedicated VRAM; discrete devices report it through their registry entry.
#[cfg(target_os = "macos")]
fn dedicated_graphics_bytes(
    _adapter: &RenderAdapter,
    device: Option<&RenderDevice>,
) -> Option<u64> {
    // SAFETY: the HAL guard retains the Metal device; both queries leave its state unchanged.
    let native = unsafe { device?.wgpu_device().as_hal::<wgpu::hal::api::Metal>()? };
    let raw = native.raw_device().lock();
    if raw.has_unified_memory() {
        Some(0)
    } else {
        macos::dedicated_bytes(raw.registry_id())
    }
}

/// Unsupported platforms use the shared-memory fallback until a backend probe is available.
#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn dedicated_graphics_bytes(
    _adapter: &RenderAdapter,
    _device: Option<&RenderDevice>,
) -> Option<u64> {
    None
}
