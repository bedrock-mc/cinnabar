#[path = "../src/environment_settings.rs"]
mod environment_settings;

use environment_settings::{CloudQuality, EnvironmentQualitySettings, PrecipitationQuality};

#[test]
fn environment_quality_rejects_numeric_surrogates() {
    assert_eq!(CloudQuality::ALL.len(), 4);
    assert_eq!(PrecipitationQuality::ALL.len(), 3);
    let settings = EnvironmentQualitySettings {
        clouds: CloudQuality::High,
        precipitation: PrecipitationQuality::Low,
    };
    assert_eq!(settings.clouds, CloudQuality::High);
    assert_eq!(settings.precipitation, PrecipitationQuality::Low);
    assert!(
        serde_json::from_str::<EnvironmentQualitySettings>(r#"{"clouds":2,"precipitation":1}"#,)
            .is_err()
    );
}
