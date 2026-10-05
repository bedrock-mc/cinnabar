use super::*;
use std::cell::Cell;

fn stack(text: &[u8]) -> Arc<ValidatedPackStack> {
    crate::runtime::network::pack_reload_tests::stack(&[("texts/en_US.lang", text)])
}

fn context() -> Context {
    Context {
        generation: 0,
        locale: "en_US".into(),
        vanilla: None,
        material_keys_ready: false,
    }
}

fn artwork_pack() -> Arc<assets::SessionEntityPack> {
    let entity = br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"test:artwork_cache","materials":{"default":"entity_alphatest"},"textures":{"default":"textures/entity/cache"},"geometry":{"default":"geometry.cache"},"render_controllers":["controller.render.cache"]}}}"#;
    let geometry = br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.cache","texture_width":16,"texture_height":16},"bones":[{"name":"root","cubes":[{"origin":[0,0,0],"size":[1,1,1],"uv":[0,0]}]}]}]}"#;
    let render = br#"{"format_version":"1.8.0","render_controllers":{"controller.render.cache":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#;
    let mut image = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(16, 16, image::Rgba([17, 23, 31, 255]))
        .write_to(&mut image, image::ImageFormat::Png)
        .unwrap();
    let compiled = pack_compiler::compile_actor_pack(vec![
        ("entity/cache.json".into(), entity.to_vec()),
        ("models/entity/cache.geo.json".into(), geometry.to_vec()),
        ("render_controllers/cache.json".into(), render.to_vec()),
        ("textures/entity/cache.png".into(), image.into_inner()),
    ])
    .unwrap()
    .unwrap();
    Arc::new(assets::SessionEntityPack {
        assets: Arc::new(assets::RuntimeEntityAssets::from_compiled(compiled.entities).unwrap()),
        textures: compiled.textures.into(),
        bindings: compiled.bindings.into(),
        equipment: None,
    })
}

#[test]
fn artwork_reuses_only_the_same_base_and_entity_snapshots() {
    let cache = ArtworkCache::default();
    let base = render::ActorArtworkPages::default();
    let pack = artwork_pack();
    let first = cache.prepare(&base, &pack);
    let repeat = cache.prepare(&base.clone(), &pack.clone());
    assert!(Arc::ptr_eq(&first, &repeat));
    let changed = base
        .clone()
        .with_pack_artwork(&pack.textures, &pack.bindings);
    let next = cache.prepare(&changed, &pack);
    assert!(!Arc::ptr_eq(&first, &next));
    assert!(next.pages_for(&changed, &pack).is_some());
    assert!(next.pages_for(&base, &pack).is_none());
    let replacement = artwork_pack();
    let next_pack = cache.prepare(&changed, &replacement);
    assert!(!Arc::ptr_eq(&next, &next_pack));
    assert!(next_pack.pages_for(&changed, &replacement).is_some());
    assert!(next_pack.pages_for(&changed, &pack).is_none());
}

#[test]
fn unchanged_admission_reuses_every_compiled_subscriber() {
    let cache = PresentationCache::default();
    let compiles = Cell::new(0);
    let prepare = |stack| {
        cache.prepare(
            stack,
            Arc::default(),
            context(),
            |_| true,
            |stack, inputs| {
                compiles.set(compiles.get() + 1);
                prepare_changed_application(stack, inputs, None)
            },
        )
    };
    let (first, hit) = prepare(stack(b"key=cache-value"));
    assert!(!hit);
    let next_stack = stack(b"key=cache-value");
    let (next, hit) = prepare(next_stack.clone());
    assert!(hit);
    assert_eq!(compiles.get(), 1);
    assert!(Arc::ptr_eq(
        first.server_lang.as_ref().unwrap(),
        next.server_lang.as_ref().unwrap()
    ));
    let PackAdmission::Validated(admitted) = next.admission else {
        panic!("fresh admission");
    };
    assert!(Arc::ptr_eq(&admitted, &next_stack));
}

#[test]
fn compilation_inputs_and_context_changes_invalidate_reuse() {
    let cached_stack = stack(b"key=value");
    let cache = PresentationCache::default();
    let compiles = Cell::new(0);
    let prepare = |stack, inputs, context| {
        cache
            .prepare(
                stack,
                Arc::new(inputs),
                context,
                |_| true,
                |_, inputs| {
                    compiles.set(compiles.get() + 1);
                    PackApplication {
                        inputs,
                        ..Default::default()
                    }
                },
            )
            .1
    };
    assert!(!prepare(
        cached_stack.clone(),
        Default::default(),
        context()
    ));
    assert!(prepare(cached_stack.clone(), Default::default(), context()));
    let mut inputs = client_session::PackInputs {
        hashed: true,
        ..Default::default()
    };
    assert!(!prepare(cached_stack.clone(), inputs.clone(), context()));
    inputs.icons.push(("a:item".into(), "a_icon".into()));
    assert!(!prepare(cached_stack.clone(), inputs.clone(), context()));
    inputs.block_items.push(("a:item".into(), "a:block".into()));
    assert!(!prepare(cached_stack.clone(), inputs.clone(), context()));
    inputs.blocks.vanilla_blocks = vec![Arc::from("minecraft:test")].into();
    assert!(!prepare(cached_stack.clone(), inputs.clone(), context()));
    let mut changed_context = context();
    changed_context.locale = "fr_FR".into();
    assert!(!prepare(
        cached_stack.clone(),
        inputs.clone(),
        changed_context.clone()
    ));
    changed_context.vanilla = Some(Arc::new(assets::VanillaEntityRefs::new()));
    assert!(!prepare(
        cached_stack.clone(),
        inputs.clone(),
        changed_context.clone()
    ));
    changed_context.generation += 1;
    assert!(!prepare(
        cached_stack.clone(),
        inputs.clone(),
        changed_context.clone()
    ));
    changed_context.material_keys_ready = true;
    assert!(!prepare(
        cached_stack,
        inputs.clone(),
        changed_context.clone()
    ));
    assert!(!prepare(stack(b"key=changed"), inputs, changed_context));
    assert_eq!(compiles.get(), 10);
}

#[test]
fn session_preparation_reuses_compiled_output_on_a_fresh_equivalent_stack() {
    let first = super::super::prepare_validated_application(
        stack(b"cache.integration=value"),
        Arc::default(),
    );
    let next = super::super::prepare_validated_application(
        stack(b"cache.integration=value"),
        Arc::default(),
    );
    assert!(Arc::ptr_eq(
        first.server_lang.as_ref().unwrap(),
        next.server_lang.as_ref().unwrap()
    ));
}

#[test]
fn cache_never_carries_session_item_state_or_stale_rejections() {
    let cache = PresentationCache::default();
    let first = stack(b"key=value");
    let admitted_first = first.clone();
    cache.prepare(
        first.clone(),
        Arc::default(),
        context(),
        |_| true,
        |_, inputs| PackApplication {
            inputs,
            item_components: Some(Arc::default()),
            ..Default::default()
        },
    );
    let (next, hit) = cache.prepare(
        first,
        Arc::default(),
        context(),
        |_| true,
        |_, _| panic!("compile"),
    );
    assert!(hit);
    assert!(next.item_components.is_none());
    assert!(next.prepared_actor_artwork.is_none());
    let rejected =
        resource_pack::validate_handoff(protocol::ResourcePackHandoff::from_archives(vec![
            protocol::ResourcePackArchive::unencrypted(
                "00000000-0000-0000-0000-00000000000a".parse().unwrap(),
                "1.0.0".into(),
                String::new(),
                vec![0; 32],
            ),
        ]));
    let rejected = Arc::new(ValidatedPackStack::compose(&admitted_first, &rejected).unwrap());
    for _ in 0..2 {
        let (application, hit) = cache.prepare(
            rejected.clone(),
            Arc::default(),
            context(),
            |_| true,
            |stack, inputs| PackApplication {
                inputs,
                admission: PackAdmission::Validated(stack),
                ..Default::default()
            },
        );
        assert!(!hit);
        assert!(client_session::required_packs_applied(true, &application.admission).is_err());
    }
}

#[test]
fn stack_precedence_is_part_of_the_cache_identity() {
    let left = stack(b"key=left");
    let right = stack(b"key=right");
    let cache = PresentationCache::default();
    let prepare = |lower: &ValidatedPackStack, higher: &ValidatedPackStack| {
        cache.prepare(
            Arc::new(ValidatedPackStack::compose(lower, higher).unwrap()),
            Arc::default(),
            context(),
            |_| true,
            |stack, inputs| prepare_changed_application(stack, inputs, None),
        )
    };
    let (first, hit) = prepare(&left, &right);
    assert!(!hit);
    assert_eq!(first.server_lang.unwrap().lookup("key"), Some("right"));
    let (reordered, hit) = prepare(&right, &left);
    assert!(!hit);
    assert_eq!(reordered.server_lang.unwrap().lookup("key"), Some("left"));
    assert!(prepare(&right, &left).1);
    assert!(!prepare(&left, &right).1);
}

#[test]
fn compilation_runs_without_cache_lock_and_changed_context_is_not_published() {
    let cache = PresentationCache::default();
    let current = stack(b"key=value");
    for _ in 0..2 {
        let (_, hit) = cache.prepare(
            current.clone(),
            Arc::default(),
            context(),
            |_| false,
            |_, inputs| {
                assert!(cache.0.try_lock().is_ok());
                PackApplication {
                    inputs,
                    ..Default::default()
                }
            },
        );
        assert!(!hit);
    }
    assert!(cache.0.lock().unwrap().is_none());
}

#[test]
fn warm_session_presentation_timing() {
    if std::env::var_os("CINNABAR_JOIN_CACHE_BENCH").is_none() {
        eprintln!(
            "missing fixture CINNABAR_JOIN_CACHE_BENCH; skipping session presentation timing"
        );
        return;
    }
    use std::{hint::black_box, time::Instant};
    let mut image = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(1024, 1024, image::Rgba([17, 23, 31, 255]))
        .write_to(&mut image, image::ImageFormat::Png)
        .unwrap();
    let fixture = crate::runtime::network::pack_reload_tests::stack(&[
        ("texts/en_US.lang", b"cache.benchmark=value"),
        ("font/glyph_00.png", image.get_ref()),
    ]);
    let cache = PresentationCache::default();
    let mut cold = Vec::new();
    let mut warm = Vec::new();
    for _ in 0..11 {
        let started = Instant::now();
        let application = prepare_changed_application(fixture.clone(), Arc::default(), None);
        assert!(application.glyph_sheets.is_some());
        black_box(application);
        cold.push(started.elapsed().as_secs_f64() * 1000.0);
        let started = Instant::now();
        black_box(cache.prepare(
            fixture.clone(),
            Arc::default(),
            context(),
            |_| true,
            |stack, inputs| prepare_changed_application(stack, inputs, None),
        ));
        warm.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    cold.remove(0);
    warm.remove(0);
    cold.sort_by(f64::total_cmp);
    warm.sort_by(f64::total_cmp);
    println!(
        "session_presentation_fixture uncached_p50_ms={:.3} uncached_max_ms={:.3} cached_p50_ms={:.3} cached_max_ms={:.3}",
        cold[5], cold[9], warm[5], warm[9]
    );
}
