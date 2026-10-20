use super::*;
use crate::options::OptionValues;
use crate::pack::ShaderPack;
use crate::source::PackSource;

/// Options: GLOW (on), SHADOWS (off), QUALITY (2 of 1–3), TINT (0.5 of 0.0–1.0),
/// shadowMapResolution (2048 of 1024–4096) and generateShadowMipmap (const bool, on).
const SETTINGS: &str = "\
#define GLOW
//#define SHADOWS
#define QUALITY 2 // [1 2 3]
#define TINT 0.5 // Tint strength [0.0 0.5 1.0]
const int shadowMapResolution = 2048; // [1024 2048 4096]
const bool generateShadowMipmap = true;
";
const PROGRAM: &str = "#ifdef GLOW\n#endif\n#if defined SHADOWS\n#endif\nvoid main() {}\n";

fn pack(properties: &str) -> ShaderPack {
    ShaderPack::new(PackSource::from_files([
        ("settings.glsl", SETTINGS),
        ("final.fsh", PROGRAM),
        ("shaders.properties", properties),
    ]))
}

fn menu(properties: &str) -> OptionMenu {
    pack(properties).option_menu()
}

fn option(name: &str) -> MenuElement {
    MenuElement::Option {
        name: name.to_owned(),
        slider: false,
    }
}

fn slider(name: &str) -> MenuElement {
    MenuElement::Option {
        name: name.to_owned(),
        slider: true,
    }
}

fn values(pairs: &[(&str, &str)]) -> OptionValues {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

#[test]
fn screens_resolve_links_blanks_profiles_and_the_rest() {
    let menu = menu(
        "screen=<profile> <empty>  [LIGHT] GLOW NOPE *\n\
         screen.LIGHT = SHADOWS shadowMapResolution \\\n    *\n\
         screen.LIGHT.columns=1\n\
         sliders=QUALITY shadowMapResolution\n\
         profile.LOW=!SHADOWS\n",
    );
    assert_eq!(
        menu.main().elements,
        [
            MenuElement::Profile,
            MenuElement::Empty,
            MenuElement::Link("LIGHT".to_owned()),
            option("GLOW"),
            MenuElement::Empty,
            slider("QUALITY"),
            option("TINT"),
            option("generateShadowMipmap"),
        ],
        "`*` lists the options no screen uses, by name; unknown names leave a blank"
    );
    assert_eq!(menu.main().columns, 2);
    let light = menu.screen("LIGHT").expect("declared sub-screen");
    assert_eq!(
        light.elements,
        [option("SHADOWS"), slider("shadowMapResolution")],
        "only one `*` receives the unused options"
    );
    assert_eq!(light.columns, 1);
    assert_eq!(menu.screen("MISSING"), None);
}

#[test]
fn without_a_screen_key_every_option_is_on_the_main_screen() {
    let menu = menu("sliders=TINT\n");
    assert_eq!(
        menu.main().elements,
        [
            option("GLOW"),
            option("QUALITY"),
            option("SHADOWS"),
            slider("TINT"),
            option("generateShadowMipmap"),
            option("shadowMapResolution"),
        ]
    );
    assert!(menu.profiles().is_empty());
}

#[test]
fn layout_keys_are_read_raw_so_conditionals_do_not_hide_them() {
    let menu = menu("screen=GLOW\n#ifdef SHADOWS\nscreen=QUALITY <profile>\n#endif\n");
    assert_eq!(
        menu.main().elements,
        [option("QUALITY")],
        "the later key wins and <profile> needs profiles"
    );
}

#[test]
fn column_counts_follow_iris() {
    let many = vec!["<empty>"; 19].join(" ");
    let menu = menu(&format!(
        "screen={many}\nscreen.A=GLOW\nscreen.A.columns=0\nscreen.B=GLOW\n\
         screen.B.columns=wide\nscreen.C=\nscreen.columns=x\n"
    ));
    assert_eq!(menu.main().columns, 3, "more than 18 elements use three");
    assert_eq!(menu.screen("A").map(|s| s.columns), Some(1));
    assert_eq!(menu.screen("B").map(|s| s.columns), Some(2));
    assert_eq!(
        menu.screen("C").map(|s| s.elements.clone()),
        Some(vec![MenuElement::Empty]),
        "an empty list is one unknown element"
    );
}

#[test]
fn profiles_merge_includes_and_order_by_value_count() {
    let menu = menu(
        "profile.LOW=GLOW\n\
         profile.HIGH=profile.LOW SHADOWS shadowMapResolution=4096 QUALITY=3\n\
         profile.EMPTY=\n\
         profile.LOW=!SHADOWS shadowMapResolution=1024 QUALITY:1 !program.world0/final GLOW TINT\n",
    );
    let names: Vec<_> = menu.profiles().iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["LOW", "HIGH", "EMPTY"]);
    let high = &menu.profiles()[1];
    assert_eq!(
        high.values,
        values(&[
            ("GLOW", "true"),
            ("QUALITY", "3"),
            ("SHADOWS", "true"),
            ("shadowMapResolution", "4096"),
        ])
    );
    assert_eq!(high.disabled_programs, ["world0/final"]);
    assert!(
        !menu.profiles()[0].values.contains_key("TINT"),
        "a bare value option is ignored"
    );
}

#[test]
fn broken_profile_includes_leave_the_pack_without_profiles() {
    for broken in [
        "profile.A=profile.B\nprofile.B=profile.A\n",
        "profile.A=profile.MISSING\n",
        "profile.A=GLOW\nprofile.B=profile.A\nprofile.C=profile.B profile.A\n",
    ] {
        let menu = menu(&format!("screen=<profile> GLOW\n{broken}"));
        assert!(menu.profiles().is_empty(), "{broken}");
        assert_eq!(menu.main().elements, [option("GLOW")]);
    }
}

#[test]
fn profile_scan_finds_the_current_profile_and_its_neighbours() {
    let pack =
        pack("profile.LOW=!GLOW QUALITY=1\nprofile.MID=QUALITY=2\nprofile.HIGH=GLOW QUALITY=3\n");
    let menu = pack.option_menu();
    let scan = |pairs: &[(&str, &str)]| {
        menu.scan_profiles(pack.options(), &values(pairs))
            .expect("profiles")
    };
    let names: Vec<_> = menu.profiles().iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["LOW", "HIGH", "MID"]);
    let low = scan(&[("GLOW", "false"), ("QUALITY", "1")]);
    assert_eq!((low.current, low.next, low.previous), (Some(0), 1, 2));
    let high = scan(&[("QUALITY", "3")]);
    assert_eq!(high.current, Some(1));
    let mid = scan(&[]);
    assert_eq!((mid.current, mid.next, mid.previous), (Some(2), 0, 1));
    let custom = scan(&[("QUALITY", "1")]);
    assert_eq!((custom.current, custom.next, custom.previous), (None, 0, 2));
    assert_eq!(
        self::menu("").scan_profiles(pack.options(), &OptionValues::new()),
        None
    );
}

#[test]
fn the_final_invalid_column_count_restores_automatic_columns() {
    let many = vec!["<empty>"; 19].join(" ");
    let menu = menu(&format!(
        "screen={many}\nscreen.columns=4\nscreen.columns=wide\n\
         screen.A=GLOW\nscreen.A.columns=5\nscreen.A.columns=wide\n\
         screen.B=GLOW\nscreen.B.columns=wide\nscreen.B.columns=1\n"
    ));
    assert_eq!(menu.main().columns, 3);
    assert_eq!(menu.screen("A").unwrap().columns, 2);
    assert_eq!(menu.screen("B").unwrap().columns, 1);
}
