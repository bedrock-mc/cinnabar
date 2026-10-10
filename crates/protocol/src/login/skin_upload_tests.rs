//! Signed login fixtures exercise the production client-data encoder.
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};

#[test]
fn signed_login_keeps_custom_geometry_patch_version_and_classic_flags() {
    let config = jolyne::stream::client::ClientHandshakeConfig::random(
        "127.0.0.1:19132".parse().unwrap(),
        "Fixture",
    );
    let geometry = crate::ClientSkinGeometry {
        resource_patch: r#"{"geometry":{"default":"geometry.fixture"}}"#.into(),
        geometry_data: r#"{"geometry.fixture":{"bones":[]}}"#.into(),
        engine_version: "1.12.0".into(),
    };
    let skin = crate::ClientSkin {
        rgba8: vec![42; crate::CLASSIC_SKIN_SIDE * crate::CLASSIC_SKIN_SIDE * 4],
        width: crate::CLASSIC_SKIN_SIDE as u32,
        height: crate::CLASSIC_SKIN_SIDE as u32,
        arm_size: "wide".into(),
        cape: None,
        geometry: Some(geometry.clone()),
    };
    let (chain, jwt) = jolyne::auth::client::generate_self_signed_chain(
        &config.identity_key,
        "Fixture",
        config.uuid,
        Some(&skin),
    )
    .unwrap();
    let (_, online_jwt) = jolyne::auth::client::encode_with_mojang_chain(
        &config.identity_key,
        "Fixture",
        config.uuid,
        &chain,
        Some(&skin),
    )
    .unwrap();
    for jwt in [jwt, online_jwt] {
        let claims: serde_json::Value = serde_json::from_slice(
            &URL_SAFE_NO_PAD
                .decode(jwt.split('.').nth(1).unwrap())
                .unwrap(),
        )
        .unwrap();
        for (key, expected) in [
            ("SkinResourcePatch", geometry.resource_patch.as_str()),
            ("SkinGeometryData", geometry.geometry_data.as_str()),
            (
                "SkinGeometryDataEngineVersion",
                geometry.engine_version.as_str(),
            ),
        ] {
            assert_eq!(
                STANDARD.decode(claims[key].as_str().unwrap()).unwrap(),
                expected.as_bytes()
            );
        }
        assert_eq!(
            STANDARD
                .decode(claims["SkinData"].as_str().unwrap())
                .unwrap(),
            skin.rgba8
        );
        assert_eq!(claims["PremiumSkin"], false);
        assert_eq!(claims["PersonaSkin"], false);
        assert_eq!(claims["SkinAnimationData"], "");
        assert_eq!(claims["AnimatedImageData"], serde_json::json!([]));
    }
}
