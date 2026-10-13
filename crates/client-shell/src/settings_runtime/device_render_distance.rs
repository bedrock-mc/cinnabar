//! Reads the selected graphics device once before publishing menu settings.

use bevy::{
    prelude::{Commands, Res, ResMut, warn},
    render::renderer::{RenderAdapter, RenderDevice},
};

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
use objc2_metal::MTLDevice;

/// Samples hardware once and shares its memory facts with the existing distance defaults.
pub fn initialize_hardware(
    mut commands: Commands,
    adapter: Option<Res<RenderAdapter>>,
    device: Option<Res<RenderDevice>>,
    mut menu: ResMut<crate::menu::MenuRuntime>,
) {
    use crate::menu::settings_options::graphics_detection::GraphicsHardware;
    use launcher::menu::settings_options::graphics_detection::{
        AdapterIdentity, DeviceType, HardwareFacts,
    };

    let graphics_bytes = adapter
        .as_deref()
        .and_then(|adapter| graphics_memory_bytes(adapter, device.as_deref()));
    if graphics_bytes.is_none() {
        warn!("graphics memory unavailable; automatic graphics uses the Balanced fallback");
    }
    let adapter = adapter
        .as_deref()
        .map(|adapter| {
            let info = adapter.get_info();
            AdapterIdentity {
                vendor: info.vendor,
                device: info.device,
                name: info.name,
                device_type: match info.device_type {
                    wgpu::DeviceType::IntegratedGpu => DeviceType::Integrated,
                    wgpu::DeviceType::DiscreteGpu => DeviceType::Discrete,
                    wgpu::DeviceType::VirtualGpu => DeviceType::Virtual,
                    wgpu::DeviceType::Cpu => DeviceType::Cpu,
                    wgpu::DeviceType::Other => DeviceType::Unknown,
                },
                backend: format!("{:?}", info.backend),
                driver: info.driver,
                driver_info: info.driver_info,
                driver_build: graphics_driver_build(),
            }
        })
        .unwrap_or_default();
    let memory = launcher_host::memory::physical_bytes();
    menu.sync_render_distance_device(ui::RenderDistanceDevice {
        physical_memory_bytes: memory,
        dedicated_graphics_memory_bytes: if adapter.device_type == DeviceType::Discrete {
            graphics_bytes.unwrap_or(0)
        } else {
            0
        },
        use_full_graphics_memory: false,
    });
    let cores = launcher_host::cpu::cores();
    commands.insert_resource(GraphicsHardware(HardwareFacts {
        adapter,
        graphics_memory_bytes: graphics_bytes,
        physical_memory_bytes: memory,
        logical_cores: cores.logical,
        physical_cores: cores.physical,
    }));
}

/// Includes the macOS build because Metal drivers ship with the operating system.
fn graphics_driver_build() -> String {
    #[cfg(target_os = "macos")]
    {
        return std::process::Command::new("/usr/bin/sw_vers")
            .arg("-buildVersion")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
            .unwrap_or_default();
    }
    #[cfg(not(target_os = "macos"))]
    String::new()
}

/// Reads dedicated VRAM or the integrated share from the selected backend.
#[cfg(target_os = "windows")]
fn graphics_memory_bytes(adapter: &RenderAdapter, _device: Option<&RenderDevice>) -> Option<u64> {
    if adapter.get_info().backend == wgpu::Backend::Vulkan {
        return vulkan_graphics_bytes(adapter);
    }
    // SAFETY: the borrowed adapter stays alive and only a read-only DXGI description is queried.
    unsafe {
        let native = adapter.as_hal::<wgpu::hal::api::Dx12>()?;
        native.raw_adapter().GetDesc1().ok().map(|desc| {
            if adapter.get_info().device_type == wgpu::DeviceType::IntegratedGpu {
                desc.SharedSystemMemory as u64
            } else {
                desc.DedicatedVideoMemory as u64
            }
        })
    }
}

/// Uses the selected Vulkan adapter's device-local memory on Linux.
#[cfg(target_os = "linux")]
fn graphics_memory_bytes(adapter: &RenderAdapter, _device: Option<&RenderDevice>) -> Option<u64> {
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
        Some(vulkan_heap_bytes(
            adapter.get_info().device_type,
            &memory.memory_heaps[..memory.memory_heap_count as usize],
        ))
    }
}

/// Counts device-local heaps for hardware GPUs, including integrated shared heaps.
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn vulkan_heap_bytes(device_type: wgpu::DeviceType, heaps: &[ash::vk::MemoryHeap]) -> u64 {
    if !matches!(
        device_type,
        wgpu::DeviceType::DiscreteGpu | wgpu::DeviceType::IntegratedGpu
    ) {
        return 0;
    }
    heaps
        .iter()
        .filter(|heap| heap.flags.contains(ash::vk::MemoryHeapFlags::DEVICE_LOCAL))
        .map(|heap| heap.size)
        .sum()
}

/// Reads Metal's recommended unified working set or the discrete device's registry VRAM.
#[cfg(target_os = "macos")]
fn graphics_memory_bytes(_adapter: &RenderAdapter, device: Option<&RenderDevice>) -> Option<u64> {
    // SAFETY: the HAL guard retains the Metal device; both queries leave its state unchanged.
    let native = unsafe { device?.wgpu_device().as_hal::<wgpu::hal::api::Metal>()? };
    let raw = native.raw_device();
    if raw.hasUnifiedMemory() {
        Some(raw.recommendedMaxWorkingSetSize())
    } else {
        macos::dedicated_bytes(raw.registryID())
    }
}

/// Unsupported backends leave graphics memory unknown.
#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn graphics_memory_bytes(_adapter: &RenderAdapter, _device: Option<&RenderDevice>) -> Option<u64> {
    None
}

#[cfg(all(test, any(target_os = "windows", target_os = "linux")))]
mod tests {
    use super::*;

    #[test]
    fn vulkan_local_heaps_supply_the_selected_gpu_memory() {
        let heaps = [
            ash::vk::MemoryHeap::default()
                .size(2 << 30)
                .flags(ash::vk::MemoryHeapFlags::DEVICE_LOCAL),
            ash::vk::MemoryHeap::default()
                .size(2 << 30)
                .flags(ash::vk::MemoryHeapFlags::DEVICE_LOCAL),
            ash::vk::MemoryHeap::default().size(16 << 30),
        ];
        let dedicated = vulkan_heap_bytes(wgpu::DeviceType::DiscreteGpu, &heaps);
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
            vulkan_heap_bytes(wgpu::DeviceType::IntegratedGpu, &heaps),
            dedicated
        );
    }
}
