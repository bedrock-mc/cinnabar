use super::*;

#[test]
fn hud_card_preview_fit_cache_tracks_only_dimensions_and_fitting_parameters() {
    let key = FitKey {
        ink: [400., 100.],
        node: [200., 100.],
        ui_scale: 2.,
        fraction: 0.8,
        max_scale: 2.4,
    };
    let mut cache = FitCache::default();
    assert_eq!(cache.scale(key), 0.2);
    assert!(cache.scale(key) < hud_card::MIN_SCALE);
    assert_eq!(cache.computations, 1);
    let (_, allocation) = test_alloc::measure(|| assert_eq!(cache.scale(key), 0.2));
    assert_eq!(allocation.calls, 0);
    assert_eq!(cache.computations, 1);
    for changed in [
        FitKey {
            ink: [200., 100.],
            ..key
        },
        FitKey {
            ink: [400., 200.],
            ..key
        },
        FitKey {
            node: [100., 100.],
            ..key
        },
        FitKey {
            node: [200., 20.],
            ..key
        },
        FitKey {
            ui_scale: 1.,
            ..key
        },
        FitKey {
            fraction: 0.5,
            ..key
        },
        FitKey {
            max_scale: hud_card::MIN_SCALE,
            ..key
        },
    ] {
        cache.scale(key);
        let before = cache.computations;
        cache.scale(changed);
        assert_eq!(cache.computations, before + 1);
        cache.scale(changed);
        assert_eq!(cache.computations, before + 1);
    }
    assert_eq!(
        cache.scale(FitKey {
            ink: [1., 1.],
            ..key
        }),
        key.max_scale
    );
}
