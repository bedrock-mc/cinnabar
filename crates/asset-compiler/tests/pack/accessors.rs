use super::support::*;

#[test]
fn exact_side_caps_and_static_terrain_accessors_fail_closed() {
    let valid = tempfile::tempdir().expect("valid cactus pack");
    write_pack(
        valid.path(),
        r#"{
            "format_version":[1,1,0],
            "cactus":{"textures":{"down":"cactus_bottom","side":"cactus_side","up":"cactus_top"}}
        }"#,
        r#"{"texture_data":{
            "cactus_bottom":{"textures":"textures/blocks/cactus_bottom"},
            "cactus_side":{"textures":"textures/blocks/cactus_side"},
            "cactus_top":{"textures":"textures/blocks/cactus_top"}
        }}"#,
        EMPTY_FLIPBOOKS,
    );
    let pack = read_pack(valid.path()).expect("read valid cactus pack");
    assert_eq!(
        pack.blocks.get_exact_side_caps("cactus"),
        Some(["cactus_side", "cactus_bottom", "cactus_top"])
    );
    assert_eq!(
        pack.terrain.get_exact_static_no_tint("cactus_bottom"),
        Some("textures/blocks/cactus_bottom")
    );

    for (name, route) in [
        ("scalar", r#""cactus_side""#),
        (
            "explicit horizontal",
            r#"{"down":"cactus_bottom","up":"cactus_top","side":"cactus_side","west":"cactus_side"}"#,
        ),
        (
            "missing side",
            r#"{"down":"cactus_bottom","up":"cactus_top"}"#,
        ),
        (
            "missing down",
            r#"{"side":"cactus_side","up":"cactus_top"}"#,
        ),
        (
            "missing up",
            r#"{"down":"cactus_bottom","side":"cactus_side"}"#,
        ),
        (
            "unknown typo key",
            r#"{"down":"cactus_bottom","side":"cactus_side","up":"cactus_top","sied":"cactus_side"}"#,
        ),
    ] {
        let directory = tempfile::tempdir().expect("invalid cactus pack");
        write_pack(
            directory.path(),
            &format!(r#"{{"format_version":[1,1,0],"cactus":{{"textures":{route}}}}}"#),
            r#"{"texture_data":{"cactus_side":{"textures":"textures/blocks/cactus_side"},"cactus_bottom":{"textures":"textures/blocks/cactus_bottom"},"cactus_top":{"textures":"textures/blocks/cactus_top"}}}"#,
            EMPTY_FLIPBOOKS,
        );
        let pack = read_pack(directory.path())
            .unwrap_or_else(|error| panic!("read {name} cactus fixture: {error}"));
        assert_eq!(pack.blocks.get_exact_side_caps("cactus"), None, "{name}");
    }

    for (name, terrain_value) in [
        (
            "array",
            r#"["textures/blocks/cactus_side","textures/blocks/cactus_side_2"]"#,
        ),
        (
            "tinted",
            r##"{"path":"textures/blocks/cactus_side","overlay_color":"#00ff00"}"##,
        ),
    ] {
        let directory = tempfile::tempdir().expect("invalid cactus terrain");
        write_pack(
            directory.path(),
            r#"{"format_version":[1,1,0],"cactus":{"textures":{"down":"cactus_bottom","side":"cactus_side","up":"cactus_top"}}}"#,
            &format!(
                r#"{{"texture_data":{{"cactus_side":{{"textures":{terrain_value}}},"cactus_bottom":{{"textures":"textures/blocks/cactus_bottom"}},"cactus_top":{{"textures":"textures/blocks/cactus_top"}}}}}}"#
            ),
            EMPTY_FLIPBOOKS,
        );
        let pack = read_pack(directory.path())
            .unwrap_or_else(|error| panic!("read {name} terrain fixture: {error}"));
        assert_eq!(
            pack.terrain.get_exact_static_no_tint("cactus_side"),
            None,
            "{name}"
        );
    }
}

#[test]
fn exact_cake_faces_and_untinted_pairs_fail_closed() {
    const TERRAIN: &str = r#"{"texture_data":{
        "cake_bottom":{"textures":["textures/blocks/cake_bottom","textures/blocks/cake_bottom"]},
        "cake_side":{"textures":["textures/blocks/cake_side","textures/blocks/cake_side"]},
        "cake_top":{"textures":["textures/blocks/cake_top","textures/blocks/cake_top"]},
        "cake_west":{"textures":["textures/blocks/cake_side","textures/blocks/cake_inner"]}
    }}"#;
    let valid = tempfile::tempdir().expect("valid cake pack");
    write_pack(
        valid.path(),
        r#"{"format_version":[1,1,0],"cake":{"textures":{"down":"cake_bottom","east":"cake_side","north":"cake_side","south":"cake_side","up":"cake_top","west":"cake_west"}}}"#,
        TERRAIN,
        EMPTY_FLIPBOOKS,
    );
    let pack = read_pack(valid.path()).expect("read valid cake pack");
    assert_eq!(
        pack.blocks.get_exact_cake_faces(),
        Some([
            "cake_west",
            "cake_side",
            "cake_bottom",
            "cake_top",
            "cake_side",
            "cake_side"
        ])
    );
    assert_eq!(
        pack.terrain.get_exact_pair_no_tint("cake_west"),
        Some(["textures/blocks/cake_side", "textures/blocks/cake_inner"])
    );

    for (name, route) in [
        ("scalar", r#""cake_side""#),
        (
            "side fallback",
            r#"{"down":"cake_bottom","side":"cake_side","up":"cake_top","west":"cake_west"}"#,
        ),
        (
            "missing face",
            r#"{"down":"cake_bottom","east":"cake_side","north":"cake_side","up":"cake_top","west":"cake_west"}"#,
        ),
        (
            "wrong route",
            r#"{"down":"cake_bottom","east":"cake_side","north":"cake_side","south":"cake_side","up":"cake_top","west":"cake_side"}"#,
        ),
        (
            "unknown key",
            r#"{"down":"cake_bottom","east":"cake_side","north":"cake_side","south":"cake_side","up":"cake_top","west":"cake_west","sied":"cake_side"}"#,
        ),
    ] {
        let directory = tempfile::tempdir().expect("invalid cake pack");
        write_pack(
            directory.path(),
            &format!(r#"{{"cake":{{"textures":{route}}}}}"#),
            TERRAIN,
            EMPTY_FLIPBOOKS,
        );
        let pack = read_pack(directory.path())
            .unwrap_or_else(|error| panic!("read {name} cake fixture: {error}"));
        assert_eq!(pack.blocks.get_exact_cake_faces(), None, "{name}");
    }

    for (name, value) in [
        ("static", r#""textures/blocks/cake_side""#),
        ("singleton", r#"["textures/blocks/cake_side"]"#),
        (
            "three",
            r#"["textures/blocks/cake_side","textures/blocks/cake_inner","textures/blocks/cake_inner"]"#,
        ),
        ("empty", r#"["","textures/blocks/cake_inner"]"#),
        (
            "tinted",
            r##"[{"path":"textures/blocks/cake_side","overlay_color":"#ffffff"},"textures/blocks/cake_inner"]"##,
        ),
    ] {
        let directory = tempfile::tempdir().expect("invalid cake terrain");
        write_pack(
            directory.path(),
            r#"{"cake":{"textures":{"down":"cake_bottom","east":"cake_side","north":"cake_side","south":"cake_side","up":"cake_top","west":"cake_west"}}}"#,
            &format!(r#"{{"texture_data":{{"cake_west":{{"textures":{value}}}}}}}"#),
            EMPTY_FLIPBOOKS,
        );
        if let Ok(pack) = read_pack(directory.path()) {
            assert_eq!(
                pack.terrain.get_exact_pair_no_tint("cake_west"),
                None,
                "{name}"
            );
        }
    }
}

#[test]
fn exact_farmland_routes_and_inverse_moisture_selector_fail_closed() {
    let valid = tempfile::tempdir().expect("valid farmland pack");
    write_pack(
        valid.path(),
        r#"{"farmland":{"textures":{"down":"farmland_side","side":"farmland_side","up":"farmland"}}}"#,
        r#"{"texture_data":{"farmland_side":{"textures":"textures/blocks/dirt"},"farmland":{"textures":["textures/blocks/farmland_wet","textures/blocks/farmland_dry"]}}}"#,
        EMPTY_FLIPBOOKS,
    );
    let pack = read_pack(valid.path()).expect("read valid farmland pack");
    assert_eq!(
        pack.blocks.get_exact_side_caps("farmland"),
        Some(["farmland_side", "farmland_side", "farmland"])
    );
    assert_eq!(
        pack.terrain.get_exact_static_no_tint("farmland_side"),
        Some("textures/blocks/dirt")
    );
    assert_eq!(
        pack.terrain.get_exact_farmland_side(),
        Some("textures/blocks/dirt")
    );
    assert_eq!(
        pack.terrain.get_exact_farmland_top(0),
        Some(("textures/blocks/farmland_dry", 1))
    );
    for amount in 1..=7 {
        assert_eq!(
            pack.terrain.get_exact_farmland_top(amount),
            Some(("textures/blocks/farmland_wet", 0)),
            "amount {amount}"
        );
    }
    assert_eq!(pack.terrain.get_exact_farmland_top(8), None);

    for (label, route) in [
        ("scalar", r#""farmland_side""#),
        (
            "override",
            r#"{"down":"farmland_side","side":"farmland_side","up":"farmland","north":"farmland_side"}"#,
        ),
        (
            "missing side",
            r#"{"down":"farmland_side","up":"farmland"}"#,
        ),
        (
            "wrong top key",
            r#"{"down":"farmland_side","side":"farmland_side","up":"farmland_wet"}"#,
        ),
    ] {
        let directory = tempfile::tempdir().expect("invalid farmland route");
        write_pack(
            directory.path(),
            &format!(r#"{{"farmland":{{"textures":{route}}}}}"#),
            r#"{"texture_data":{"farmland_side":{"textures":"textures/blocks/dirt"},"farmland":{"textures":["textures/blocks/farmland_wet","textures/blocks/farmland_dry"]}}}"#,
            EMPTY_FLIPBOOKS,
        );
        if let Ok(pack) = read_pack(directory.path()) {
            assert_ne!(
                pack.blocks.get_exact_side_caps("farmland"),
                Some(["farmland_side", "farmland_side", "farmland"]),
                "{label}"
            );
        }
    }

    for (label, value) in [
        ("static", r#""textures/blocks/farmland_wet""#),
        ("singleton", r#"["textures/blocks/farmland_wet"]"#),
        (
            "three",
            r#"["textures/blocks/farmland_wet","textures/blocks/farmland_dry","textures/blocks/farmland_wet"]"#,
        ),
        (
            "wrong order",
            r#"["textures/blocks/farmland_dry","textures/blocks/farmland_wet"]"#,
        ),
        ("empty", r#"["","textures/blocks/farmland_dry"]"#),
        (
            "tinted",
            r##"[{"path":"textures/blocks/farmland_wet","overlay_color":"#ffffff"},"textures/blocks/farmland_dry"]"##,
        ),
    ] {
        let directory = tempfile::tempdir().expect("invalid farmland terrain");
        write_pack(
            directory.path(),
            r#"{"farmland":{"textures":{"down":"farmland_side","side":"farmland_side","up":"farmland"}}}"#,
            &format!(
                r#"{{"texture_data":{{"farmland_side":{{"textures":"textures/blocks/dirt"}},"farmland":{{"textures":{value}}}}}}}"#
            ),
            EMPTY_FLIPBOOKS,
        );
        if let Ok(pack) = read_pack(directory.path()) {
            assert_eq!(pack.terrain.get_exact_farmland_top(0), None, "{label}");
        }
    }

    for (label, terrain) in [
        (
            "top carried metadata",
            r#"{"texture_data":{"farmland_side":{"textures":"textures/blocks/dirt"},"farmland":{"textures":["textures/blocks/farmland_wet","textures/blocks/farmland_dry"],"carried_textures":"textures/blocks/farmland_dry"}}}"#,
        ),
        (
            "variant alias metadata",
            r#"{"texture_data":{"farmland_side":{"textures":"textures/blocks/dirt"},"farmland":{"textures":[{"path":"textures/blocks/farmland_wet","alias":"wet"},"textures/blocks/farmland_dry"]}}}"#,
        ),
        (
            "side carried metadata",
            r#"{"texture_data":{"farmland_side":{"textures":"textures/blocks/dirt","carried_textures":"textures/blocks/dirt"},"farmland":{"textures":["textures/blocks/farmland_wet","textures/blocks/farmland_dry"]}}}"#,
        ),
    ] {
        let directory = tempfile::tempdir().expect("farmland metadata adversary");
        write_pack(
            directory.path(),
            r#"{"farmland":{"textures":{"down":"farmland_side","side":"farmland_side","up":"farmland"}}}"#,
            terrain,
            EMPTY_FLIPBOOKS,
        );
        let pack = read_pack(directory.path())
            .unwrap_or_else(|error| panic!("read {label} farmland pack: {error}"));
        assert!(
            pack.terrain.get_exact_farmland_top(0).is_none()
                || pack.terrain.get_exact_farmland_side().is_none(),
            "{label}"
        );
    }
}
