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
fn windows_defaults_to_dx12_then_vulkan() {
    assert_eq!(
        Platform::Windows.backends(),
        [Backend::Dx12, Backend::Vulkan]
    );
    for platform in [Platform::Linux, Platform::Android] {
        assert_eq!(platform.backends(), [Backend::Vulkan]);
    }
    assert_eq!(Platform::Macos.backends(), [Backend::Metal]);
}

#[test]
fn precedence_respects_overrides_defaults_and_sentinel() {
    let defaults = Platform::Windows.backends();
    assert_eq!(candidates(defaults, defaults, None, None, &[]), defaults);
    assert_eq!(
        candidates(defaults, defaults, None, None, &[Backend::Dx12]),
        [Backend::Vulkan]
    );
    assert_eq!(
        candidates(
            defaults,
            defaults,
            None,
            Some(Backend::Vulkan),
            &[Backend::Vulkan]
        ),
        [Backend::Vulkan]
    );
    assert_eq!(
        candidates(
            defaults,
            defaults,
            Some(&[Backend::Dx12]),
            Some(Backend::Vulkan),
            &[Backend::Dx12]
        ),
        [Backend::Dx12]
    );
    assert!(candidates(defaults, defaults, None, None, defaults).is_empty());
    assert!(candidates(defaults, defaults, None, Some(Backend::Gl), &[]).is_empty());
    assert!(candidates(defaults, defaults, Some(&[]), None, &[]).is_empty());
}

#[test]
fn a_lone_supported_api_obeys_exclusions_before_the_bounded_retry() {
    for backend in Backend::ALL {
        assert_eq!(
            candidates(&[*backend], &[*backend], None, None, &[*backend]),
            []
        );
    }
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

#[test]
fn available_registry_matches_all_platform_candidates_without_gl() {
    assert!(!Backend::AVAILABLE.contains(&Backend::Gl));
    let platforms = [
        Platform::Windows,
        Platform::Linux,
        Platform::Android,
        Platform::Macos,
    ];
    for platform in platforms {
        let supported = platform.backends();
        assert!(supported.iter().all(|api| Backend::AVAILABLE.contains(api)));
        assert_eq!(candidates(supported, supported, None, None, &[]), supported);
        assert!(!supported.contains(&Backend::Gl));
    }
    for api in Backend::ALL {
        assert_eq!(Backend::AVAILABLE.contains(api), *api != Backend::Gl);
        assert_eq!(
            Backend::AVAILABLE.contains(api),
            platforms.iter().any(|p| p.backends().contains(api))
        );
    }
}
