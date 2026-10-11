use super::*;

#[test]
fn admitted_samples_and_route_handles_allocate_nothing() {
    let profiles = crate::environment::tests::profiles();
    let fogs = crate::environment::tests::fog_profiles();
    let bindings = FogBindings::for_profiles(&[], &profiles, &fogs);
    let before = crate::tests::alloc_count::thread_allocations();
    let old_names: Vec<Option<Box<str>>> = [(); render::PRECIPITATION_SAMPLE_OFFSETS.len()]
        .map(|_| Some(Box::<str>::from("minecraft:the_end")))
        .into_iter()
        .collect();
    let old_allocations = crate::tests::alloc_count::thread_allocations() - before;
    assert_eq!(old_names.len(), render::PRECIPITATION_SAMPLE_OFFSETS.len());
    let before = crate::tests::alloc_count::thread_allocations();
    let samples = [(); render::PRECIPITATION_SAMPLE_OFFSETS.len()].map(|_| bindings.fog(None, 2));
    let (profile, route, fog) = bindings.camera(None, 2);
    let context = crate::environment::EnvironmentContext {
        dimension: 2,
        fog_biomes: Some(samples),
        camera_profile: profile,
        camera_fog: fog,
        profile_route: route.clone(),
        default_fog: bindings.default_fog,
        render_distance_blocks: Some(256.0),
        ..Default::default()
    };
    let frame = crate::environment::atmosphere::derive_profiled_atmosphere_frame(
        Default::default(),
        Default::default(),
        0.0,
        meshing::CameraMedium::Air,
        &context,
        &profiles,
        &fogs,
        None,
        0.0,
    );
    let allocations = crate::tests::alloc_count::thread_allocations() - before;
    let expected = profiles
        .iter()
        .position(|profile| profile.biome_identifier.as_ref() == "minecraft:the_end")
        .unwrap();
    let expected_fog = fogs
        .iter()
        .position(|fog| fog.identifier == profiles[expected].fog_identifier)
        .unwrap();
    assert_eq!(profile, Some(BiomeProfileIndex(expected)));
    assert_eq!(fog, Some(FogProfileIndex(expected_fog)));
    assert_eq!(samples, [fog; render::PRECIPITATION_SAMPLE_OFFSETS.len()]);
    assert_eq!(
        route.unwrap().fog_identifier.as_deref(),
        Some(fogs[expected_fog].identifier.as_ref())
    );
    assert_eq!(
        frame.1.fog_identifier.as_deref(),
        Some(fogs[expected_fog].identifier.as_ref())
    );
    assert_eq!(allocations, 0);
    println!("27 fog samples: name publication={old_allocations}, admitted frame={allocations}");
}

/// Creates a registry entry whose tint fields do not affect fog binding.
fn rule(id: u32, name: &str) -> BiomeRule {
    let tint = assets::TintSource::direct(0);
    BiomeRule {
        id,
        name: name.into(),
        flags: 0,
        grass: tint,
        foliage: tint,
        dry_foliage: tint,
        water: tint,
        temperature_bits: 0,
        downfall_bits: 0,
    }
}

#[test]
fn known_missing_profiles_remain_distinct_from_unknown_ids_and_rebinding_refreshes_indices() {
    let profiles = crate::environment::tests::profiles();
    let mut fogs = crate::environment::tests::fog_profiles();
    let rules = [rule(42, "minecraft:the_end"), rule(43, "test:missing")];
    let mut bindings = FogBindings::for_profiles(&rules, &profiles, &fogs);
    let expected = fogs
        .iter()
        .position(|fog| fog.identifier.as_ref() == "minecraft:fog_the_end")
        .unwrap();
    assert_eq!(bindings.fog(Some(42), 0), Some(FogProfileIndex(expected)));
    assert_eq!(bindings.fog(Some(43), 0), None);
    assert_eq!(bindings.fog(Some(999), 0), bindings.fog(None, 0));
    assert_eq!(bindings.camera(Some(43), 0).0, bindings.camera(None, 0).0);
    assert_eq!(bindings.fog(None, -1), None);

    fogs.remove(expected);
    bindings.compile(&rules, &profiles, &fogs);
    assert_eq!(bindings.fog(Some(42), 0), None);
    let plains = fogs
        .iter()
        .position(|fog| fog.identifier.as_ref() == "minecraft:fog_plains")
        .unwrap();
    assert_eq!(bindings.fog(None, 0), Some(FogProfileIndex(plains)));
    bindings.compile(&[rule(42, "minecraft:plains")], &profiles, &fogs);
    assert_eq!(bindings.fog(Some(42), 2), Some(FogProfileIndex(plains)));
}
