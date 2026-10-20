use super::*;
use bedrock_services::{
    minecraft::profile::Gallery,
    xbox::{achievements::Collection, statistics::Batch},
};
use serde::Deserialize;

/// The actual Go fallback and Rust projections consume one shared response corpus.
#[test]
fn retail_profile_projections_match_shared_go_vectors() {
    #[derive(Deserialize)]
    struct StatisticsCase {
        name: String,
        response: serde_json::Value,
        expected: bridge::ProfileStatistics,
        error: bool,
    }
    #[derive(Deserialize)]
    struct AchievementsCase {
        name: String,
        response: serde_json::Value,
        expected: bridge::ProfileAchievements,
    }
    #[derive(Deserialize)]
    struct Vectors {
        statistics: Vec<StatisticsCase>,
        achievements: Vec<AchievementsCase>,
    }
    let vectors: Vectors = serde_json::from_str(include_str!(
        "../../../../../core/catalog/native_profile_conformance.json"
    ))
    .unwrap();
    for vector in vectors.statistics {
        let users = vector.response["users"]
            .as_array()
            .unwrap()
            .iter()
            .map(|user| serde_json::from_str(&user.to_string()).unwrap())
            .collect();
        let batch = Batch {
            users,
            malformed_users: 0,
        };
        let got = statistics::project(&batch, "123");
        if vector.error {
            assert!(got.is_none(), "{}", vector.name);
        } else {
            assert_eq!(got, Some(vector.expected), "{}", vector.name);
        }
    }
    for vector in vectors.achievements {
        let values = serde_json::from_str(&vector.response["achievements"].to_string()).unwrap();
        let collection = Collection {
            owner_xuid: "123".into(),
            achievements: values,
            malformed_items: 0,
        };
        assert_eq!(
            achievements::project(collection, "123").unwrap(),
            Some(vector.expected),
            "{}",
            vector.name
        );
    }
    assert!(
        policy::get()
            .statistics
            .configuration_ids
            .contains(&bedrock_services::minecraft::SERVICE_CONFIG_ID)
    );
}

/// Only explicit featured metadata can select the screenshot, and neither counts nor fallback URLs are invented.
#[test]
fn gallery_keeps_featured_only_selection_and_unknown_state() {
    let image = |raw: serde_json::Value| serde_json::from_value(raw).unwrap();
    let gallery = Gallery {
        images: vec![
            image(
                serde_json::json!({"url":"https://images.example.test/unfeatured.png","isFeatured":false}),
            ),
            image(serde_json::json!({"url":"https://images.example.test/unknown.png"})),
            image(serde_json::json!({"url":"file:///private/featured.png","isFeatured":true})),
            image(
                serde_json::json!({"url":"https://images.example.test/featured.png","isFeatured":true}),
            ),
        ],
        malformed_items: 1,
    };
    let (selected, partial) = facets::featured(gallery);
    assert_eq!(selected.url, "https://images.example.test/featured.png");
    assert!(selected.path.is_empty());
    assert!(partial);
    let (selected, partial) = facets::featured(Gallery {
        images: Vec::new(),
        malformed_items: 0,
    });
    assert_eq!(selected, bridge::Artwork::default());
    assert!(!partial);
    assert!(
        achievements::project(
            Collection {
                owner_xuid: "456".into(),
                achievements: Vec::new(),
                malformed_items: 0,
            },
            "123"
        )
        .is_err()
    );
    assert!(
        achievements::project(
            Collection {
                owner_xuid: "123".into(),
                achievements: Vec::new(),
                malformed_items: 1,
            },
            "123"
        )
        .unwrap()
        .is_none(),
        "an incomplete collection cannot claim zero achievements"
    );
}
