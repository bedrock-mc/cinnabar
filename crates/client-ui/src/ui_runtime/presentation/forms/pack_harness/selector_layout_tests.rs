use json_ui::{
    ActionElement, ActionForm, Catalog, Context, FormButton, FormModel, LayoutEnv, TextMeasure,
    TextureMeta, TextureSource, render_form,
};

struct Text;

impl TextMeasure for Text {
    fn extent(&self, text: &str) -> [f64; 2] {
        [
            text.chars().count() as f64 * 6.0,
            if text.is_empty() { 0.0 } else { 8.0 },
        ]
    }
}

struct NoArt;

impl TextureSource for NoArt {
    fn texture(&self, _path: &str) -> Option<TextureMeta> {
        None
    }
}

#[test]
fn server_pack_selector_keeps_reserved_controls_out_of_game_grid() {
    let Some(carrier) = super::carrier() else {
        eprintln!(
            "skipping server_pack_selector_keeps_reserved_controls_out_of_game_grid: missing installed UI carrier; make assets"
        );
        return;
    };
    let Some(pack) = super::env_pack() else {
        eprintln!(
            "skipping server_pack_selector_keeps_reserved_controls_out_of_game_grid: missing CINNABAR_FORM_PACK_DIR fixture"
        );
        return;
    };
    let base = Catalog::from_files(
        carrier
            .ui_files()
            .iter()
            .map(|file| (&*file.path, &*file.bytes)),
    )
    .unwrap();
    let catalog = super::super::engine::layer_pack_catalog(&base, &pack.ui_layers);
    if catalog.lookup("lb_form_33", "form_33").is_none() {
        eprintln!(
            "skipping server_pack_selector_keeps_reserved_controls_out_of_game_grid: fixture has no ui/lifeboat/lb_form_33.json"
        );
        return;
    }
    let labels: Vec<_> = (0..8)
        .map(|index| format!("Game {index}"))
        .chain([
            "§t§rMini Games".to_owned(),
            "§t§rWorld Games".to_owned(),
            "§u§rBack".to_owned(),
            "§v§rClose".to_owned(),
        ])
        .chain(std::iter::repeat_n(String::new(), 11))
        .collect();
    let model = FormModel::Action(ActionForm {
        title: "form33_Mini Game Selector".to_owned(),
        body: String::new(),
        elements: labels
            .into_iter()
            .map(|text| ActionElement::Button(FormButton { text, image: None }))
            .collect(),
    });
    let render = render_form(
        &model,
        &catalog,
        &Context::retail(cfg!(target_os = "macos")),
        [480.0, 270.0],
        &LayoutEnv {
            text: &Text,
            textures: &NoArt,
        },
    )
    .expect("server form resolves");
    let buttons: Vec<_> = render
        .hits
        .iter()
        .filter(|hit| hit.pressed.as_deref() == Some("button.form_button_click"))
        .collect();
    let games: Vec<_> = buttons
        .iter()
        .copied()
        .filter(|hit| hit.collection_index.is_some_and(|index| index < 8))
        .collect();
    assert_eq!(games.len(), 8, "every game has exactly one click target");
    let unique_positions = |axis: usize| {
        let mut positions: Vec<_> = games
            .iter()
            .map(|hit| if axis == 0 { hit.rect.x } else { hit.rect.y })
            .collect();
        positions.sort_by(f64::total_cmp);
        positions.dedup_by(|a, b| (*a - *b).abs() < 0.01);
        positions.len()
    };
    assert_eq!([unique_positions(0), unique_positions(1)], [3, 3]);
    let game_left = games
        .iter()
        .map(|hit| hit.rect.x)
        .fold(f64::INFINITY, f64::min);
    for index in [8, 9] {
        let tab = buttons
            .iter()
            .find(|hit| hit.collection_index == Some(index))
            .unwrap_or_else(|| panic!("sidebar item {index} is visible and clickable"));
        assert!(tab.rect.x + tab.rect.w <= game_left);
    }
    for index in [10, 11] {
        assert_eq!(
            buttons
                .iter()
                .filter(|hit| hit.collection_index == Some(index))
                .count(),
            1,
            "header item {index} appears only in the header"
        );
    }
    assert_eq!(
        buttons.len(),
        12,
        "reserved blank entries create no game buttons"
    );
    let mut game_scrolls = std::collections::BTreeMap::new();
    for game in games {
        let (key, scroll) = render
            .report
            .scrolls
            .iter()
            .filter(|(key, _)| {
                game.key
                    .strip_prefix(key.as_str())
                    .is_some_and(|suffix| suffix.starts_with('/'))
            })
            .max_by_key(|(key, _)| key.len())
            .expect("each game belongs to a reported scroll viewport");
        game_scrolls.insert(key, scroll);
    }
    assert!(
        game_scrolls
            .values()
            .all(|scroll| scroll.max_offset() < 0.01),
        "eight games fit without scrolling: {:?}",
        game_scrolls
    );
}
