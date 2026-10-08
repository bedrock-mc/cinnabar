use super::*;

fn decal() -> Decal {
    Decal {
        center: [0.0, 64.0, 0.0],
        radius: 3.0,
        color: [1.0, 0.2, 0.1, 0.8],
        progress: 0.5,
        style: DecalStyle::Telegraph,
    }
}

#[test]
fn appends_are_all_or_nothing_within_budgets() {
    let mut frame = Primitives::default();
    let full = Primitives {
        decals: vec![decal(); mod_api::MAX_RENDER_DECALS],
        ..Default::default()
    };
    frame.append_checked(full).unwrap();
    let one_more = Primitives {
        decals: vec![decal()],
        ..Default::default()
    };
    assert!(frame.append_checked(one_more).is_err());
    assert_eq!(frame.decals.len(), mod_api::MAX_RENDER_DECALS);
}

#[test]
fn non_finite_or_oversized_values_reject_the_whole_append() {
    let bad = [
        Decal {
            radius: f32::NAN,
            ..decal()
        },
        Decal {
            radius: mod_api::MAX_PRIMITIVE_EXTENT_BLOCKS + 1.0,
            ..decal()
        },
        Decal {
            center: [f32::INFINITY, 0.0, 0.0],
            ..decal()
        },
        Decal {
            color: [1.0, 1.0, 1.0, 2.0],
            ..decal()
        },
    ];
    for bad in bad {
        let mut frame = Primitives::default();
        let append = Primitives {
            decals: vec![decal(), bad],
            ..Default::default()
        };
        assert!(frame.append_checked(append).is_err(), "{bad:?}");
        assert!(frame.is_empty());
    }
    let short_ribbon = Primitives {
        ribbons: vec![Ribbon {
            points: vec![[0.0; 3]],
            width: 1.0,
            color: [1.0; 4],
        }],
        ..Default::default()
    };
    assert!(Primitives::default().append_checked(short_ribbon).is_err());
}

#[test]
fn pass_names_are_short_identifiers() {
    assert!(pass_name_valid("boss-aura_2"));
    for name in ["", "Upper", "white space", &"x".repeat(33)] {
        assert!(!pass_name_valid(name), "{name}");
    }
}

fn pass(name: &str, order: i32, revision: u64) -> Pass {
    Pass {
        name: name.into(),
        order,
        depth: false,
        source: Arc::from(""),
        shader: Arc::from(""),
        revision,
        enabled: true,
        params: [0.0; MAX_PASS_PARAMS],
    }
}

fn output(passes: Vec<Pass>, decals: usize) -> RenderOutput {
    RenderOutput {
        passes,
        primitives: Arc::new(Primitives {
            decals: vec![decal(); decals],
            ..Default::default()
        }),
    }
}

#[test]
fn merging_keeps_load_order_priority_within_the_single_mod_budgets() {
    let camera = output(vec![pass("shake", 5, 1), pass("grade", 0, 2)], 40);
    let effects = output(vec![pass("shake", -1, 3), pass("aura", 1, 4)], 40);
    let merged = RenderMerge::default().merge([&camera, &effects]);
    let passes: Vec<_> = merged
        .passes
        .iter()
        .map(|pass| (pass.name.as_str(), pass.revision))
        .collect();
    assert_eq!(passes, [("grade", 2), ("aura", 4), ("shake", 1)]);
    assert_eq!(merged.primitives.decals.len(), mod_api::MAX_RENDER_DECALS);

    let crowded: Vec<_> = (0..mod_api::MAX_RENDER_PASSES as u64 + 2)
        .map(|index| pass(&format!("p{index}"), 0, index))
        .collect();
    let late = output(vec![pass("late", -9, 99)], 0);
    let merged = RenderMerge::default().merge([&output(crowded, 0), &late]);
    assert_eq!(merged.passes.len(), mod_api::MAX_RENDER_PASSES);
    assert!(merged.passes.iter().all(|pass| pass.name != "late"));
}

#[test]
fn a_lone_drawing_mod_shares_its_primitives_so_nothing_rebuilds() {
    let drawing = output(Vec::new(), 3);
    let passes_only = output(vec![pass("grade", 0, 1)], 0);
    let merged = RenderMerge::default().merge([&passes_only, &drawing]);
    assert!(Arc::ptr_eq(&merged.primitives, &drawing.primitives));
}

#[test]
fn pass_changes_keep_the_merged_primitives_of_unchanged_mods() {
    let mut cache = RenderMerge::default();
    let mut camera = output(vec![pass("shake", 0, 1)], 2);
    let effects = output(Vec::new(), 3);
    let first = cache.merge([&camera, &effects]).primitives;
    camera.passes[0].params[0] = 1.0;
    let next = cache.merge([&camera, &effects]);
    assert_eq!(next.passes[0].params[0], 1.0);
    assert!(
        Arc::ptr_eq(&first, &next.primitives),
        "unchanged sets must not rebuild"
    );
    camera.primitives = Arc::new((*camera.primitives).clone());
    let equal = cache.merge([&camera, &effects]).primitives;
    assert!(
        Arc::ptr_eq(&first, &equal),
        "equal content keeps its identity too"
    );
    camera.primitives = Arc::new(Primitives::default());
    let changed = cache.merge([&camera, &effects]).primitives;
    assert_eq!(changed.decals.len(), 3);
}
