//! Reads the selected graphics device once before publishing menu settings.

use bevy::{
    prelude::{Res, ResMut, warn},
    render::renderer::{RenderAdapter, RenderDevice},
};

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
use objc2_metal::MTLDevice;

/// Applies device defaults to untouched preferences after the renderer has selected its adapter.
pub fn initialize_render_distance(
    adapter: Option<Res<RenderAdapter>>,
    device: Option<Res<RenderDevice>>,
    mut menu: ResMut<crate::menu::MenuRuntime>,
) {
    menu.sync_render_resolution_default(render_resolution_default());
    let graphics_bytes = adapter
        .as_deref()
        .and_then(|adapter| dedicated_graphics_bytes(adapter, device.as_deref()));
    if graphics_bytes.is_none() {
        warn!("graphics memory unavailable; render distance uses the shared-memory recommendation");
    }
    menu.sync_render_distance_device(ui::RenderDistanceDevice {
        physical_memory_bytes: launcher_host::memory::physical_bytes(),
        dedicated_graphics_memory_bytes: graphics_bytes.unwrap_or(0),
        use_full_graphics_memory: false,
    });
}

/// Reads Linux's board identity once; other hosts keep the native-resolution default.
fn render_resolution_default() -> u8 {
    #[cfg(target_os = "linux")]
    if [
        "/sys/firmware/devicetree/base/model",
        "/proc/device-tree/model",
    ]
    .into_iter()
    .filter_map(|path| std::fs::read(path).ok())
    .any(|model| raspberry_pi_model(&model))
    {
        return launcher::menu::settings_options::render_resolution::RASPBERRY_PI_DEFAULT_PERCENT;
    }
    ui::DEFAULT_RENDER_SCALE_PERCENT
}

/// Recognizes the board-model property without classifying unrelated ARM Linux devices as Pis.
#[cfg(any(target_os = "linux", test))]
fn raspberry_pi_model(model: &[u8]) -> bool {
    model.starts_with(b"Raspberry Pi ")
}

/// Reads dedicated memory from the selected backend, never from a different enumerated GPU.
#[cfg(target_os = "windows")]
fn dedicated_graphics_bytes(
    adapter: &RenderAdapter,
    _device: Option<&RenderDevice>,
) -> Option<u64> {
    if adapter.get_info().backend == wgpu::Backend::Vulkan {
        return vulkan_graphics_bytes(adapter);
    }
    // SAFETY: the borrowed adapter stays alive and only a read-only DXGI description is queried.
    unsafe {
        let native = adapter.as_hal::<wgpu::hal::api::Dx12>()?;
        native
            .raw_adapter()
            .GetDesc1()
            .ok()
            .map(|desc| desc.DedicatedVideoMemory as u64)
    }
}

/// Uses the selected Vulkan adapter's dedicated memory on Linux.
#[cfg(target_os = "linux")]
fn dedicated_graphics_bytes(
    adapter: &RenderAdapter,
    _device: Option<&RenderDevice>,
) -> Option<u64> {
    vulkan_graphics_bytes(adapter)
}

/// Reads the selected Vulkan device's heaps on either supported desktop platform.
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn vulkan_graphics_bytes(adapter: &RenderAdapter) -> Option<u64> {
    // SAFETY: the borrowed adapter retains its instance and physical device for this read-only query.
    unsafe {
        let native = adapter.as_hal::<wgpu::hal::api::Vulkan>()?;
        let memory = native
            .shared_instance()
            .raw_instance()
            .get_physical_device_memory_properties(native.raw_physical_device());
        Some(dedicated_vulkan_heap_bytes(
            adapter.get_info().device_type,
            &memory.memory_heaps[..memory.memory_heap_count as usize],
        ))
    }
}

/// Counts device-local heaps as dedicated memory only for a discrete adapter.
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn dedicated_vulkan_heap_bytes(
    device_type: wgpu::DeviceType,
    heaps: &[ash::vk::MemoryHeap],
) -> u64 {
    if device_type != wgpu::DeviceType::DiscreteGpu {
        return 0;
    }
    heaps
        .iter()
        .filter(|heap| heap.flags.contains(ash::vk::MemoryHeapFlags::DEVICE_LOCAL))
        .map(|heap| heap.size)
        .sum()
}

/// Unified Metal devices have no dedicated VRAM; discrete devices report it through their registry entry.
#[cfg(target_os = "macos")]
fn dedicated_graphics_bytes(
    _adapter: &RenderAdapter,
    device: Option<&RenderDevice>,
) -> Option<u64> {
    // SAFETY: the HAL guard retains the Metal device; both queries leave its state unchanged.
    let native = unsafe { device?.wgpu_device().as_hal::<wgpu::hal::api::Metal>()? };
    let raw = native.raw_device();
    if raw.hasUnifiedMemory() {
        Some(0)
    } else {
        macos::dedicated_bytes(raw.registryID())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raspberry_pi_identity_accepts_device_tree_models_only() {
        for model in [
            b"Raspberry Pi 5 Model B Rev 1.0\0".as_slice(),
            b"Raspberry Pi 4 Model B Rev 1.5\0",
            b"Raspberry Pi Compute Module 4 Rev 1.1\0",
        ] {
            assert!(raspberry_pi_model(model));
        }
        for model in [
            b"".as_slice(),
            b"NVIDIA Jetson Orin Nano Developer Kit\0",
            b"linux aarch64",
            b"Not Raspberry Pi 5 Model B\0",
            b"Raspberry Pine\0",
        ] {
            assert!(!raspberry_pi_model(model));
        }
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    #[test]
    fn vulkan_dedicated_heaps_choose_the_discrete_recommendation() {
        let heaps = [
            ash::vk::MemoryHeap::default()
                .size(2 << 30)
                .flags(ash::vk::MemoryHeapFlags::DEVICE_LOCAL),
            ash::vk::MemoryHeap::default()
                .size(2 << 30)
                .flags(ash::vk::MemoryHeapFlags::DEVICE_LOCAL),
            ash::vk::MemoryHeap::default().size(16 << 30),
        ];
        let dedicated = dedicated_vulkan_heap_bytes(wgpu::DeviceType::DiscreteGpu, &heaps);
        assert_eq!(dedicated, 4 << 30);
        assert_eq!(
            ui::RenderDistanceDevice {
                physical_memory_bytes: 16 << 30,
                dedicated_graphics_memory_bytes: dedicated,
                use_full_graphics_memory: false,
            }
            .defaults()
            .recommended,
            35
        );
        assert_eq!(
            dedicated_vulkan_heap_bytes(wgpu::DeviceType::IntegratedGpu, &heaps),
            0
        );
    }
}
