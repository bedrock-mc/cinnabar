use super::*;

#[test]
fn admitted_samples_and_route_handles_allocate_nothing() {
    let profiles = crate::environment::tests::profiles();
    let fogs = crate::environment::tests::fog_profiles();
    let bindings = FogBindings::for_profiles(&[], &profiles, &fogs);
    let before = crate::tests::alloc_count::thread_allocations();
    let old_names: Vec<Option<Box<str>>> = [(); render_model::PRECIPITATION_SAMPLE_OFFSETS.len()]
        .map(|_| Some(Box::<str>::from("minecraft:the_end")))
        .into_iter()
        .collect();
    let old_allocations = crate::tests::alloc_count::thread_allocations() - before;
    assert_eq!(
        old_names.len(),
        render_model::PRECIPITATION_SAMPLE_OFFSETS.len()
    );
    let before = crate::tests::alloc_count::thread_allocations();
    let samples =
        [(); render_model::PRECIPITATION_SAMPLE_OFFSETS.len()].map(|_| bindings.fog(None, 2));
    let (profile, route, fog) = bindings.camera(None, 2);
    let context = crate::environment::EnvironmentContext {
        dimension: 2,
        fog_registry_generation: bindings.atmosphere_generation,
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
        bindings.atmosphere_generation.unwrap(),
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
    assert_eq!(
        samples,
        [fog; render_model::PRECIPITATION_SAMPLE_OFFSETS.len()]
    );
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

#[test]
fn atmosphere_rejects_indices_sampled_before_a_registry_reload() {
    use crate::environment::{EnvironmentContext, atmosphere::derive_profiled_atmosphere_frame};

    let mut profiles = crate::environment::tests::profiles();
    let mut fogs = crate::environment::tests::fog_profiles();
    let bindings = FogBindings::for_profiles(&[], &profiles, &fogs);
    let sampled = [0, 2].map(|dimension| {
        let (camera_profile, profile_route, camera_fog) = bindings.camera(None, dimension);
        EnvironmentContext {
            dimension,
            fog_registry_generation: bindings.atmosphere_generation,
            camera_biome_identifier: profile_route
                .as_ref()
                .and_then(|route| route.biome_identifier.clone()),
            camera_profile,
            camera_fog,
            profile_route,
            fog_biomes: Some(
                [bindings.fog(None, dimension); render_model::PRECIPITATION_SAMPLE_OFFSETS.len()],
            ),
            default_fog: bindings.default_fog,
            render_distance_blocks: Some(256.0),
            ..Default::default()
        }
    });

    // A pack inserts earlier entries after sampling and replaces the camera's route.
    let mut inserted_fog = fogs[0].clone();
    inserted_fog.identifier = "aaa:inserted_fog".into();
    for distance in &mut inserted_fog.distances {
        distance.rgb8 = 0xff0000;
        distance.end_bits = 4.0_f32.to_bits();
    }
    fogs.insert(0, inserted_fog);
    let mut inserted_profile = profiles[0].clone();
    inserted_profile.biome_identifier = "aaa:inserted_biome".into();
    profiles.insert(0, inserted_profile);
    for profile in &mut profiles {
        profile.atmosphere_identifier = "test:reloaded_atmosphere".into();
    }

    for mut context in sampled {
        let expected = EnvironmentContext {
            dimension: context.dimension,
            camera_biome_identifier: context.camera_biome_identifier.clone(),
            render_distance_blocks: context.render_distance_blocks,
            ..Default::default()
        };
        for lattice in [true, false] {
            if !lattice {
                context.fog_biomes = None;
            }
            for medium in [meshing::CameraMedium::Air, meshing::CameraMedium::Water] {
                let derive = |context: &EnvironmentContext| {
                    derive_profiled_atmosphere_frame(
                        Default::default(),
                        Default::default(),
                        0.0,
                        medium,
                        context,
                        &profiles,
                        &fogs,
                        [1; 32],
                        None,
                        0.0,
                    )
                };
                assert_eq!(derive(&context), derive(&expected));
            }
        }
    }
}
