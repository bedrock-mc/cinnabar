use super::*;
use launcher::graphics_backend::INTEL_PCI_VENDOR;

/// Builds a candidate with no native resources so selection policy can be tested offline.
fn candidate(backend: Backend, kind: wgpu::DeviceType, vendor: u32) -> Candidate<()> {
    Candidate {
        backend,
        identity: Adapter {
            name: "test GPU".into(),
            vendor,
            device: 1,
            driver: "test".into(),
            driver_info: String::new(),
            driver_version: String::new(),
            integrated: kind == wgpu::DeviceType::IntegratedGpu,
        },
        device_type: kind,
        native: (),
    }
}

#[test]
fn every_probe_attributes_a_native_interruption_to_only_its_own_backend() {
    for &backend in Backend::ALL {
        let root = tempfile::tempdir().unwrap();
        let mut guard = StartupGuard::open(root.path()).unwrap();
        for &previous in Backend::ALL {
            probe_backend(&mut guard, previous, false, || {
                let pending = StartupGuard::pending(root.path()).unwrap().unwrap();
                assert_eq!(pending.backend, previous);
                assert_eq!(pending.stage, Stage::EnumeratingAdapters);
                Ok(())
            })
            .unwrap();
            assert!(StartupGuard::pending(root.path()).unwrap().is_none());
        }
        let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _: Result<()> = probe_backend(&mut guard, backend, false, || {
                assert_eq!(
                    StartupGuard::pending(root.path()).unwrap().unwrap().backend,
                    backend
                );
                panic!("simulated native interruption");
            });
        }));
        assert!(interrupted.is_err());
        drop(guard);
        let reopened = StartupGuard::open(root.path()).unwrap();
        assert_eq!(reopened.recovery.failed_backends(), [backend]);
    }
}

#[test]
fn device_rejections_continue_across_adapters_and_apis_until_success_or_exhaustion() {
    for success in [None, Some(0), Some(2)] {
        let root = tempfile::tempdir().unwrap();
        let mut guard = StartupGuard::open(root.path()).unwrap();
        let order = [Backend::Vulkan, Backend::Vulkan, Backend::Dx12];
        let adapters = order
            .map(|api| candidate(api, wgpu::DeviceType::DiscreteGpu, 0))
            .into();
        let mut calls = Vec::new();
        let result = try_candidates(adapters, &mut guard, false, |candidate| {
            let pending = StartupGuard::pending(root.path()).unwrap().unwrap();
            assert_eq!(pending.backend, candidate.backend);
            assert_eq!(pending.stage, Stage::CreatingDevice);
            calls.push(candidate.backend);
            if Some(calls.len() - 1) == success {
                Ok(candidate.backend)
            } else {
                bail!("candidate rejected")
            }
        });
        assert_eq!(
            calls,
            order[..success.map_or(order.len(), |index| index + 1)]
        );
        if let Some(success) = success {
            assert_eq!(result.unwrap(), order[success]);
            assert_eq!(
                StartupGuard::pending(root.path()).unwrap().unwrap().stage,
                Stage::FirstPresentation
            );
            guard.presented().unwrap();
        } else {
            assert!(result.is_err());
            assert!(StartupGuard::pending(root.path()).unwrap().is_none());
            assert!(guard.recovery.failed_backends().is_empty());
        }
    }
}

#[test]
fn discrete_dx12_beats_integrated_or_software_vulkan_and_vendor_order_breaks_ties() {
    for kind in [wgpu::DeviceType::IntegratedGpu, wgpu::DeviceType::Cpu] {
        let mut adapters = vec![
            candidate(Backend::Vulkan, kind, 0),
            candidate(Backend::Dx12, wgpu::DeviceType::DiscreteGpu, 0),
        ];
        rank(
            &mut adapters,
            Platform::Windows,
            wgpu::PowerPreference::HighPerformance,
        );
        assert_eq!(adapters[0].backend, Backend::Dx12);
    }
    for (vendor, preferred) in [(INTEL_PCI_VENDOR, Backend::Dx12), (0, Backend::Vulkan)] {
        let mut adapters = vec![
            candidate(Backend::Vulkan, wgpu::DeviceType::DiscreteGpu, vendor),
            candidate(Backend::Dx12, wgpu::DeviceType::DiscreteGpu, vendor),
        ];
        rank(
            &mut adapters,
            Platform::Windows,
            wgpu::PowerPreference::HighPerformance,
        );
        assert_eq!(adapters[0].backend, preferred);
    }
    let mut adapters: Vec<_> = [
        wgpu::DeviceType::Cpu,
        wgpu::DeviceType::VirtualGpu,
        wgpu::DeviceType::IntegratedGpu,
        wgpu::DeviceType::DiscreteGpu,
    ]
    .into_iter()
    .map(|kind| candidate(Backend::Vulkan, kind, 0))
    .collect();
    rank(
        &mut adapters,
        Platform::Windows,
        wgpu::PowerPreference::HighPerformance,
    );
    assert_eq!(
        adapters.iter().map(|a| a.device_type).collect::<Vec<_>>(),
        [
            wgpu::DeviceType::DiscreteGpu,
            wgpu::DeviceType::IntegratedGpu,
            wgpu::DeviceType::VirtualGpu,
            wgpu::DeviceType::Cpu
        ]
    );
}

#[test]
fn blocklisted_vulkan_is_removed_before_trying_working_dx12() {
    let root = tempfile::tempdir().unwrap();
    let mut guard = StartupGuard::open(root.path()).unwrap();
    let mut adapters: Vec<_> = [Backend::Vulkan, Backend::Dx12]
        .into_iter()
        .map(|api| {
            let mut candidate = candidate(api, wgpu::DeviceType::IntegratedGpu, INTEL_PCI_VENDOR);
            candidate.identity.driver_version = "101.2114".into();
            candidate
        })
        .collect();
    assert!(
        adapters
            .iter()
            .all(|a| eligible(a, Platform::Windows, None, false, true))
    );
    adapters.retain(|a| eligible(a, Platform::Windows, None, false, false));
    assert_eq!(adapters.len(), 1);
    let selected = try_candidates(adapters, &mut guard, false, |a| Ok(a.backend)).unwrap();
    assert_eq!(selected, Backend::Dx12);
}

#[test]
fn a_name_restricts_every_api_without_unnamed_fallback() {
    for backend in [Backend::Vulkan, Backend::Dx12] {
        let a = candidate(backend, wgpu::DeviceType::DiscreteGpu, 0);
        assert!(eligible(
            &a,
            Platform::Windows,
            Some("TEST gpu"),
            false,
            false
        ));
        assert!(!eligible(
            &a,
            Platform::Windows,
            Some("missing"),
            false,
            false
        ));
        assert!(!eligible(&a, Platform::Windows, None, true, false));
    }
}

/// Checks that unused native objects are destroyed under their own API's sentinel.
struct DropCheck {
    root: std::path::PathBuf,
    backend: Backend,
}

impl Drop for DropCheck {
    /// Observes the active sentinel at the same point native resource destructors would run.
    fn drop(&mut self) {
        assert_eq!(
            StartupGuard::pending(&self.root).unwrap().unwrap().backend,
            self.backend
        );
    }
}

#[test]
fn unused_candidates_are_dropped_under_their_own_api_before_restoring_presentation() {
    let root = tempfile::tempdir().unwrap();
    let mut guard = StartupGuard::open(root.path()).unwrap();
    let adapters = [Backend::Dx12, Backend::Vulkan]
        .into_iter()
        .map(|backend| {
            let fixture = candidate(backend, wgpu::DeviceType::DiscreteGpu, 0);
            Candidate {
                backend,
                identity: fixture.identity,
                device_type: fixture.device_type,
                native: DropCheck {
                    root: root.path().to_owned(),
                    backend,
                },
            }
        })
        .collect();
    let selected = try_candidates(adapters, &mut guard, false, |candidate| {
        let backend = candidate.backend;
        drop(candidate);
        Ok(backend)
    })
    .unwrap();
    assert_eq!(selected, Backend::Dx12);
    let pending = StartupGuard::pending(root.path()).unwrap().unwrap();
    assert_eq!(pending.backend, Backend::Dx12);
    assert_eq!(pending.stage, Stage::FirstPresentation);
}

#[test]
fn unclassified_gl_hardware_ranks_ahead_of_software_vulkan() {
    for preference in [
        wgpu::PowerPreference::None,
        wgpu::PowerPreference::LowPower,
        wgpu::PowerPreference::HighPerformance,
    ] {
        let mut adapters = vec![
            candidate(Backend::Vulkan, wgpu::DeviceType::Cpu, 0),
            candidate(Backend::Gl, wgpu::DeviceType::Other, 0),
        ];
        rank(&mut adapters, Platform::Linux, preference);
        assert_eq!(adapters[0].backend, Backend::Gl);
    }
}

#[test]
fn low_power_prefers_integrated_hardware_over_discrete_hardware() {
    for (preference, preferred) in [
        (wgpu::PowerPreference::LowPower, Backend::Vulkan),
        (wgpu::PowerPreference::HighPerformance, Backend::Dx12),
        (wgpu::PowerPreference::None, Backend::Dx12),
    ] {
        let mut adapters = vec![
            candidate(Backend::Dx12, wgpu::DeviceType::DiscreteGpu, 0),
            candidate(Backend::Vulkan, wgpu::DeviceType::IntegratedGpu, 0),
        ];
        rank(&mut adapters, Platform::Windows, preference);
        assert_eq!(adapters[0].backend, preferred);
    }
}
