//! The distant terrain Video settings persist, map to the user settings and reset with Video.

use super::{
    DISTANT_TERRAIN_DISTANCE_OPTION, DISTANT_TERRAIN_OPTION, DISTANT_TERRAIN_QUALITY_OPTION,
    DISTANT_TERRAIN_STORAGE_OPTION, SETTINGS_OPTIONS, SettingsGroup, SettingsOptions,
    video_presets::VideoPreset,
};
use render_api::{DistantTerrainQuality, DistantTerrainTier, distant_terrain};

fn index(name: &str) -> usize {
    SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == name)
        .expect("registered option")
}

#[test]
fn distant_terrain_defaults_off_and_round_trips_through_storage() {
    let defaults = SettingsOptions::default().user_settings().video;
    assert!(!defaults.distant_terrain);
    assert_eq!(
        defaults.distant_terrain_chunks,
        distant_terrain::DEFAULT_DISTANT_TERRAIN_CHUNKS
    );
    assert_eq!(
        defaults.distant_terrain_quality,
        DistantTerrainQuality::Medium
    );
    assert_eq!(defaults.distant_terrain_storage_bytes, 1 << 30);

    for quality in DistantTerrainQuality::ALL {
        let mut settings = SettingsOptions::default();
        assert!(settings.set(index(DISTANT_TERRAIN_OPTION.name), 1));
        settings.set(index(DISTANT_TERRAIN_DISTANCE_OPTION.name), 100);
        settings.set(index(DISTANT_TERRAIN_QUALITY_OPTION.name), quality.index());
        settings.set(index(DISTANT_TERRAIN_STORAGE_OPTION.name), 2048);
        let loaded = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
        let video = loaded.user_settings().video;
        assert!(video.distant_terrain);
        assert_eq!(video.distant_terrain_chunks, 100);
        assert_eq!(video.distant_terrain_quality, quality);
        assert_eq!(video.distant_terrain_storage_bytes, 2048 << 20);
    }
}

#[test]
fn distant_terrain_values_snap_to_the_tier_and_storage_steps() {
    let mut settings = SettingsOptions::default();
    settings.set(index(DISTANT_TERRAIN_DISTANCE_OPTION.name), i32::MAX);
    settings.set(index(DISTANT_TERRAIN_STORAGE_OPTION.name), 1000);
    let video = settings.user_settings().video;
    assert_eq!(
        video.distant_terrain_chunks,
        DistantTerrainTier::CURRENT.max_distance_chunks
    );
    assert_eq!(video.distant_terrain_storage_bytes, 768 << 20);
    settings.set(index(DISTANT_TERRAIN_STORAGE_OPTION.name), 0);
    assert_eq!(
        settings.user_settings().video.distant_terrain_storage_bytes,
        u64::from(distant_terrain::MIN_DISTANT_TERRAIN_STORAGE_MIB) << 20
    );
}

#[test]
fn distant_terrain_resets_with_video_and_follows_the_presets() {
    let mut settings = SettingsOptions::default();
    settings.set(index(DISTANT_TERRAIN_OPTION.name), 1);
    settings.set(index(DISTANT_TERRAIN_STORAGE_OPTION.name), 4096);
    assert!(settings.reset_group(SettingsGroup::Video));
    let video = settings.user_settings().video;
    assert!(!video.distant_terrain);
    assert_eq!(video.distant_terrain_storage_bytes, 1 << 30);

    let mut quality = SettingsOptions::default();
    quality.apply_video_preset(VideoPreset::Quality);
    let video = quality.user_settings().video;
    assert!(video.distant_terrain);
    assert_eq!(
        video.distant_terrain_chunks,
        DistantTerrainTier::CURRENT.max_distance_chunks
    );
    assert_eq!(video.distant_terrain_quality, DistantTerrainQuality::Medium);
    quality.apply_video_preset(VideoPreset::Performance);
    assert!(!quality.user_settings().video.distant_terrain);
    quality.apply_video_preset(VideoPreset::Balanced);
    assert!(!quality.user_settings().video.distant_terrain);
    assert_eq!(quality.video_preset(), Some(VideoPreset::Balanced));
}

#[test]
fn distant_terrain_sliders_caption_chunks_and_mebibytes() {
    let translate = |key: &str| {
        if key == "options.renderDistanceFormat" {
            "%s chunks".to_owned()
        } else {
            key.to_owned()
        }
    };
    assert_eq!(
        super::distant_terrain_slider_label(DISTANT_TERRAIN_DISTANCE_OPTION.name, 256, &translate),
        Some("256 chunks".to_owned())
    );
    assert_eq!(
        super::distant_terrain_slider_label(DISTANT_TERRAIN_STORAGE_OPTION.name, 1024, &translate),
        Some("1024 MiB".to_owned())
    );
    assert_eq!(
        super::distant_terrain_slider_label("gamma", 50, &translate),
        None
    );
}
