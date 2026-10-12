use bedrock_protocol_1_26_51::packets::StartGame;

use super::game_rules::{daylight_cycle_rule_update, weather_cycle_rule_update};

/// Initial clock and weather state retained from StartGame.
///
/// This is separate from [`super::WorldBootstrap`] so existing world-stream
/// construction remains independent of the later app-owned atmosphere state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldEnvironmentBootstrap {
    /// StartGame's elapsed simulation tick, not the named daylight clock.
    pub initial_time: i64,
    /// StartGame's cycle lock tick, used only when the daylight cycle is disabled.
    pub day_cycle_lock_time: i32,
    /// Whether the world clock advances between server-authored time updates.
    pub daylight_cycle_enabled: bool,
    /// Native seasonal accumulation freezes while this rule is disabled.
    pub weather_cycle_enabled: bool,
    /// Initial rain intensity clamped to the closed unit interval.
    pub rain_level: f32,
    /// Initial lightning intensity clamped to the closed unit interval.
    pub lightning_level: f32,
}

impl WorldEnvironmentBootstrap {
    pub(crate) fn from_start_game(start_game: &StartGame) -> Self {
        let settings = &start_game.settings;
        Self {
            // gophertunnel packet/start_game.go writes `Time int64`; the
            // generated field is u64 over the same eight little-endian bytes.
            initial_time: start_game.level_current_time.0 as i64,
            day_cycle_lock_time: settings.day_cycle_stop_time.0,
            // StartGame and GameRulesChanged carry the same tagged GameRule.
            daylight_cycle_enabled: daylight_cycle_rule_update(&settings.rule_data.rules_list)
                .unwrap_or(true),
            weather_cycle_enabled: weather_cycle_rule_update(&settings.rule_data.rules_list)
                .unwrap_or(true),
            rain_level: normalize_weather_level(settings.rain_level.0),
            lightning_level: normalize_weather_level(settings.lightning_level.0),
        }
    }
}

fn normalize_weather_level(level: f32) -> f32 {
    if level.is_finite() {
        level.clamp(0.0, 1.0)
    } else {
        0.0
    }
}
