use super::*;

/// Creates synthetic hardware with a version string, without loading a driver.
fn adapter(vendor: u32, integrated: bool) -> Adapter {
    Adapter {
        name: "synthetic GPU".into(),
        vendor,
        device: 42,
        driver: "synthetic".into(),
        driver_info: "1.2.3".into(),
        driver_version: String::new(),
        integrated,
    }
}

#[test]
fn vendor_defaults_and_hybrid_selection_follow_the_render_adapter() {
    let intel = adapter(INTEL_PCI_VENDOR, true);
    assert_eq!(
        default_order(Platform::Windows, &intel),
        [Backend::Dx12, Backend::Vulkan]
    );
    for vendor in [0x10de, 0x1002, INTEL_PCI_VENDOR] {
        let discrete = adapter(vendor, false);
        assert_eq!(
            default_order(Platform::Windows, &discrete),
            [Backend::Vulkan, Backend::Dx12]
        );
        let hybrid = Inventory {
            adapters: vec![intel.clone(), discrete.clone()],
            selected: discrete,
        };
        assert_eq!(
            default_order(Platform::Windows, &hybrid.selected)[0],
            Backend::Vulkan
        );
    }
    for platform in [Platform::Linux, Platform::Android] {
        assert_eq!(
            default_order(platform, &intel),
            [Backend::Vulkan, Backend::Gl]
        );
    }
    assert_eq!(default_order(Platform::Macos, &intel), [Backend::Metal]);
}

#[test]
fn precedence_respects_overrides_verdicts_defaults_and_sentinel() {
    let gpu = adapter(0x10de, false);
    let key = gpu.key();
    let defaults = default_order(Platform::Windows, &gpu);
    let mut cache = ProbeCache::default();
    let select = |cache: &ProbeCache, env: Option<&[Backend]>, video, failed: &[Backend]| {
        candidates(
            &defaults,
            &defaults,
            env,
            video,
            &defaults
                .iter()
                .filter_map(|backend| cache.get(&key, *backend).map(|verdict| (*backend, verdict)))
                .collect::<Vec<_>>(),
            &[],
            failed,
        )
    };
    assert_eq!(select(&cache, None, None, &[]), defaults);
    cache.record(key.clone(), Backend::Dx12, Verdict::Passed);
    assert_eq!(
        select(&cache, None, None, &[]),
        [Backend::Dx12, Backend::Vulkan]
    );
    cache.record(key.clone(), Backend::Vulkan, Verdict::Failed);
    assert_eq!(select(&cache, None, None, &[]), [Backend::Dx12]);
    assert_eq!(
        select(&cache, None, Some(Backend::Vulkan), &[Backend::Vulkan]),
        [Backend::Vulkan]
    );
    assert_eq!(
        select(
            &cache,
            Some(&[Backend::Dx12]),
            Some(Backend::Vulkan),
            &[Backend::Dx12]
        ),
        [Backend::Dx12]
    );
    assert!(select(&cache, None, None, &[Backend::Dx12]).is_empty());
    assert!(select(&cache, None, Some(Backend::Gl), &[]).is_empty());
    assert_eq!(
        candidates(
            &defaults,
            &defaults,
            Some(&[Backend::Vulkan]),
            None,
            &[(Backend::Vulkan, Verdict::Failed)],
            &[Backend::Vulkan],
            &[Backend::Vulkan]
        ),
        [Backend::Vulkan]
    );
    assert_eq!(
        candidates(
            &defaults,
            &defaults,
            None,
            None,
            &[],
            &[Backend::Vulkan],
            &[]
        ),
        [Backend::Dx12]
    );
}

#[test]
fn cache_invalidates_on_driver_or_adapter_change_but_not_enumeration_order() {
    let gpu = adapter(INTEL_PCI_VENDOR, true);
    let other = adapter(0x10de, false);
    let mut inventory = Inventory {
        selected: gpu.clone(),
        adapters: vec![gpu.clone(), other],
    };
    let mut cache = ProbeCache::default();
    assert!(cache.refresh(inventory.fingerprint()));
    for verdict in [Verdict::Passed, Verdict::Failed, Verdict::TimedOut] {
        cache.record(gpu.key(), Backend::Dx12, verdict);
        let mut loaded: ProbeCache =
            serde_json::from_slice(&serde_json::to_vec(&cache).unwrap()).unwrap();
        assert_eq!(loaded.get(&gpu.key(), Backend::Dx12), Some(verdict));
        inventory.adapters.reverse();
        assert!(!loaded.refresh(inventory.fingerprint()));
        assert_eq!(loaded.get(&gpu.key(), Backend::Dx12), Some(verdict));
    }
    inventory.selected.driver_info = "1.2.4".into();
    assert!(cache.refresh(inventory.fingerprint()));
    assert_eq!(cache.get(&gpu.key(), Backend::Dx12), None);
    cache.record(inventory.selected.key(), Backend::Dx12, Verdict::Passed);
    inventory.adapters.pop();
    assert!(cache.refresh(inventory.fingerprint()));
    assert_eq!(cache.get(&inventory.selected.key(), Backend::Dx12), None);
}

#[test]
fn selecting_another_gpu_resets_the_recovery_inventory() {
    let integrated = adapter(INTEL_PCI_VENDOR, true);
    let discrete = adapter(0x10de, false);
    let mut inventory = Inventory {
        selected: integrated.clone(),
        adapters: vec![integrated.clone(), discrete.clone()],
    };
    let mut cache = ProbeCache::default();
    cache.refresh(inventory.fingerprint());
    cache.record(integrated.key(), Backend::Dx12, Verdict::Failed);
    inventory.selected = discrete;
    assert!(cache.refresh(inventory.fingerprint()));
    assert_eq!(cache.get(&integrated.key(), Backend::Dx12), None);
}

#[test]
fn timeout_crash_and_nonzero_exit_are_durable_failures() {
    assert_eq!(
        probe_verdict(None, PROBE_TIMEOUT - Duration::from_nanos(1)),
        None
    );
    assert_eq!(probe_verdict(None, PROBE_TIMEOUT), Some(Verdict::TimedOut));
    assert_eq!(
        probe_verdict(Some(false), Duration::ZERO),
        Some(Verdict::Failed)
    );
    assert_eq!(
        probe_verdict(Some(true), PROBE_TIMEOUT),
        Some(Verdict::Passed)
    );
}

#[test]
fn driver_rules_match_numeric_inclusive_ranges_and_exact_hardware() {
    let rule: blocklist::Rule = serde_json::from_str(r#"{"platform":"windows","vendor":32902,"device":42,"driver_contains":"synthetic","min_version":[1,2,3],"max_version":[1,10],"backend":"vulkan","reason":"Synthetic regression fixture"}"#).unwrap();
    let mut gpu = adapter(INTEL_PCI_VENDOR, true);
    for version in ["1.2.3", "1.9.99", "1.10.0", "Mesa 1.3.0"] {
        gpu.driver_info = version.into();
        assert!(rule.matches(Platform::Windows, &gpu), "{version}");
    }
    for version in ["1.2.2", "1.10.1", "unknown"] {
        gpu.driver_info = version.into();
        assert!(!rule.matches(Platform::Windows, &gpu), "{version}");
    }
    gpu.driver_info = "1.3.0".into();
    assert!(!rule.matches(Platform::Android, &gpu));
    gpu.device += 1;
    assert!(!rule.matches(Platform::Windows, &gpu));
    gpu.device -= 1;
    gpu.vendor += 1;
    assert!(!rule.matches(Platform::Windows, &gpu));
}

#[test]
fn shipped_rules_are_valid_and_only_avoid_supported_apis() {
    for rule in blocklist::rules() {
        assert!(rule.platform.backends().contains(&rule.backend));
        assert!(!rule.reason.trim().is_empty());
        for version in [&rule.min_version, &rule.max_version].into_iter().flatten() {
            assert!(!version.is_empty());
        }
    }
    for backend in Backend::ALL {
        assert_eq!(Backend::parse(backend.name()), Some(*backend));
    }
    assert_eq!(Backend::parse("vulkan,dx12"), None);
}

#[test]
fn older_intel_windows_vulkan_rule_ends_at_the_known_working_driver() {
    let mut gpu = adapter(INTEL_PCI_VENDOR, true);
    gpu.driver_version = "101.2114".into();
    let matches = |gpu: &Adapter| {
        blocklist::rules()
            .iter()
            .any(|rule| rule.backend == Backend::Vulkan && rule.matches(Platform::Windows, gpu))
    };
    assert!(matches(&gpu));
    gpu.driver_version = "101.2115".into();
    assert!(!matches(&gpu));
    gpu.driver_version = "101.2114".into();
    gpu.integrated = false;
    assert!(!matches(&gpu));
}
