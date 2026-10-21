use super::*;

#[test]
fn world_options_defaults_and_bounds_cover_the_catalog() {
    let options = WorldOptions::default();
    for &id in WorldOption::ALL {
        let definition = id.definition();
        assert!(id.accepts(definition.default), "{id:?}");
        assert_eq!(options.get(id), definition.default);
        assert!(!id.accepts(-1), "{id:?}");
        if let Number { min, max, .. } = definition.control {
            assert!(id.accepts(min));
            assert!(id.accepts(max));
            assert!(!id.accepts(min - 1));
        }
    }
    assert!(!options.enabled(WorldOption::ActivateCheats));
    assert_eq!(options.get(WorldOption::SimulationDistance), 4);
    assert_eq!(options.get(WorldOption::SleepPercentage), 100);
    assert_eq!(options.get(WorldOption::RandomTickSpeed), 1);
}

#[test]
fn world_options_gated_changes_enable_cheats_and_disabling_restores_defaults() {
    for &id in WorldOption::ALL {
        let definition = id.definition();
        let mut options = WorldOptions::default();
        let changed = match definition.control {
            Toggle => 1 - definition.default,
            Number { min, .. } => min,
        };
        assert!(options.set(id, changed));
        if definition.cheats && changed != definition.default {
            assert!(options.enabled(WorldOption::ActivateCheats), "{id:?}");
            options.set(WorldOption::ActivateCheats, 0);
            assert_eq!(options.get(id), definition.default);
        }
    }
}

#[test]
fn world_options_daylight_modes_and_lan_compose() {
    let mut options = WorldOptions::default();
    options.set(WorldOption::AlwaysDay, 1);
    assert!(!options.enabled(WorldOption::DaylightCycle));
    options.set(WorldOption::DaylightCycle, 1);
    assert!(!options.enabled(WorldOption::AlwaysDay));
    assert!(options.lan_visible());
    options.set(WorldOption::Multiplayer, 0);
    assert!(!options.lan_visible());
    options.set(WorldOption::Multiplayer, 1);
    assert!(options.lan_visible());
    options.set(WorldOption::LanVisible, 0);
    assert!(!options.lan_visible());
}
