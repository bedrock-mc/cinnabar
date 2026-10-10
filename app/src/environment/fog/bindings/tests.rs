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
