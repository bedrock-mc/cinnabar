use super::*;
use json_ui::{Draw, LayoutEnv, TextMeasure, TextureMeta, TextureSource};

struct Measure;
impl TextMeasure for Measure {
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.chars().count() as f64 * 6.0, 9.0]
    }
}
impl TextureSource for Measure {
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        None
    }
}

fn row(name: &str, badged: bool) -> RosterRow {
    RosterRow {
        name: Arc::from(name),
        badged,
        annotation: None,
        skin: None,
    }
}

fn names(count: usize) -> Vec<RosterRow> {
    (0..count)
        .map(|index| row(&format!("Player{index:02}"), false))
        .collect()
}

/// Renders the list's current data at a fixed test geometry.
fn render(list: &mut PlayerList) -> Vec<json_ui::DrawNode> {
    list.bind_rows(80., &Measure);
    let context = Context::default()
        .with_var("row_height", serde_json::json!(ROW_HEIGHT))
        .with_var("row_background_height", serde_json::json!(ROW_HEIGHT - 1.0))
        .with_var("grid_top", serde_json::json!(ROW_HEIGHT + 3.0))
        .with_var("cell_width", serde_json::json!(80))
        .with_var("head_size", serde_json::json!(HEAD_WIDTH - 1.0))
        .with_var("name_offset", serde_json::json!(2.0 + HEAD_WIDTH))
        .with_var("name_width", serde_json::json!("100% - 26px"))
        .with_var("panel_width", serde_json::json!(164))
        .with_var("panel_height", serde_json::json!(122))
        .with_var("cinnabar_badge_texture", serde_json::json!(CINNABAR_BADGE));
    let env = LayoutEnv {
        text: &Measure,
        textures: &Measure,
    };
    let rendered = list
        .screen
        .render_shared_with(
            SCREEN,
            &list.catalog,
            &context,
            Arc::clone(&list.data),
            ([320.0, 240.0], 1.0, [0; 3]),
            &env,
            &ViewState::default(),
        )
        .unwrap();
    rendered.nodes.to_vec()
}

#[test]
fn list_fills_down_columns_and_never_shows_padding_as_a_player() {
    let mut list = PlayerList::new();
    assert_eq!(list.refresh(&names(23), MAX_ROWS), [2, 12]);
    let nodes = render(&mut list);
    let labels: Vec<_> = nodes
        .iter()
        .filter_map(|node| match &node.draw {
            Draw::Text { text, .. } if text.starts_with("Player") => {
                Some((text.as_str(), node.dest))
            }
            _ => None,
        })
        .collect();
    assert_eq!(labels.len(), 23);
    let find = |name: &str| labels.iter().find(|(text, _)| *text == name).unwrap().1;
    assert_eq!(find("Player00").x, find("Player01").x);
    assert!(find("Player01").y > find("Player00").y);
    assert!(find("Player12").x > find("Player00").x);
    assert_eq!(find("Player12").y, find("Player00").y);
}

#[test]
fn bounded_roster_and_unchanged_controller_share_the_same_data() {
    let mut list = PlayerList::new();
    assert_eq!(list.refresh(&names(100), MAX_ROWS), [4, 20]);
    assert_eq!(list.rows.len(), MAX_PLAYERS);
    assert_eq!(list.total, 100);
    let before = Arc::clone(&list.data);
    list.refresh(&names(100), MAX_ROWS);
    assert!(Arc::ptr_eq(&before, &list.data));
    list.refresh(&names(1), MAX_ROWS);
    assert_eq!(list.dimensions, [1, 1]);
    list.refresh(&[], MAX_ROWS);
    assert_eq!(list.dimensions, [1, 1]);
    assert!(list.rows.is_empty());
}

/// Only rows of reported Cinnabar players draw the badge, and a new badge set rebinds the rows.
#[test]
fn cinnabar_players_alone_carry_the_badge() {
    let mut list = PlayerList::new();
    let mut roster = names(3);
    list.refresh(&roster, MAX_ROWS);
    let before = Arc::clone(&list.data);
    assert_eq!(badges(&render(&mut list)), 0);
    roster[1].badged = true;
    list.refresh(&roster, MAX_ROWS);
    assert!(!Arc::ptr_eq(&before, &list.data));
    assert_eq!(badges(&render(&mut list)), 1);
}

/// Two players sharing a display name draw only the badge their own row carries.
#[test]
fn same_name_rows_keep_their_own_badge() {
    let mut list = PlayerList::new();
    list.refresh(&[row("Alex", true), row("Alex", false)], MAX_ROWS);
    assert_eq!(badges(&render(&mut list)), 1);
}

/// Counter runs preserve their RGBA colors, reserve space in narrow columns, and honor prefix placement.
#[test]
fn totem_counts_render_separate_tints_before_or_after_a_clipped_name() {
    for before in [false, true] {
        let mut list = PlayerList::new();
        let style = ui::mod_hud::TotemCounterStyle {
            render_first: before,
            show_prefix: true,
            prefix_color: [0.2, 0.4, 0.6, 0.5],
            ..Default::default()
        };
        let mut player = row("VeryLongPlayerName", true);
        player.annotation = style.annotation(2).map(Arc::new);
        list.refresh(&[player], MAX_ROWS);
        let nodes = render(&mut list);
        let find = |wanted: &str| {
            nodes
                .iter()
                .find_map(|node| match &node.draw {
                    Draw::Text { text, color, .. } if text == wanted => Some((node.dest, *color)),
                    _ => None,
                })
                .unwrap()
        };
        let (prefix, color) = find("| ");
        assert_eq!(color, [51, 102, 153, 128]);
        let (value, color) = find("-2");
        assert_eq!(color, [85, 255, 85, 255]);
        let (name, _) = find("VeryLongPlayerName");
        let head = nodes
            .iter()
            .find(|node| {
                matches!(&node.draw, Draw::Sprite { texture, .. }
            if texture.starts_with("client-raster:tab-head/"))
            })
            .unwrap();
        assert!(
            head.dest.x + head.dest.w <= name.x.min(prefix.x),
            "skin head remains before both the player name and its counter"
        );
        assert!(prefix.x + prefix.w <= value.x + 0.001);
        let badge = nodes
            .iter()
            .find(|node| matches!(&node.draw, Draw::Sprite { texture, .. } if texture == CINNABAR_BADGE))
            .unwrap();
        assert!(
            value.x + value.w <= badge.dest.x,
            "counter remains before its column's badge"
        );
        assert!(if before {
            value.x + value.w <= name.x
        } else {
            name.x + name.w <= prefix.x
        });
    }
}

/// Counts rendered badge sprites.
fn badges(nodes: &[json_ui::DrawNode]) -> usize {
    nodes
        .iter()
        .filter(
            |node| matches!(&node.draw, Draw::Sprite { texture, .. } if texture == CINNABAR_BADGE),
        )
        .count()
}

#[test]
fn heads_precede_names_and_padding_has_no_head() {
    let mut list = PlayerList::new();
    list.refresh(&names(23), MAX_ROWS);
    let nodes = render(&mut list);
    let heads: Vec<_> = nodes
        .iter()
        .filter_map(|node| match &node.draw {
            Draw::Sprite { texture, .. } if texture.starts_with("client-raster:tab-head/") => {
                Some(node.dest)
            }
            _ => None,
        })
        .collect();
    assert_eq!(heads.len(), 23);
    for node in &nodes {
        if let Draw::Text { text, .. } = &node.draw
            && text.starts_with("Player")
        {
            assert!(
                heads
                    .iter()
                    .any(|head| head.y == node.dest.y && head.x + head.w <= node.dest.x)
            );
        }
    }
}

#[test]
fn skin_changes_refresh_only_their_portrait_and_invalid_skins_use_default() {
    let mut list = PlayerList::new();
    let mut rows = vec![row("Alex", false), row("Alex", true)];
    list.refresh(&rows, MAX_ROWS);
    let fallback = list.portraits[&list.heads[0]].identity();
    assert_eq!(list.portraits.len(), 1);
    let skin = render_api::StandardSkin {
        width: 64,
        height: 64,
        rgba8: [40, 100, 200, 255].repeat(64 * 64).into(),
        cape: None,
        geometry: None,
    };
    rows[0].skin = Some(skin);
    list.refresh(&rows, MAX_ROWS);
    assert_ne!(list.portraits[&list.heads[0]].identity(), fallback);
    assert_eq!(list.portraits[&list.heads[1]].identity(), fallback);
    let before = Arc::clone(&list.data);
    list.refresh(&rows, MAX_ROWS);
    assert!(Arc::ptr_eq(&before, &list.data));
    rows[0].skin.as_mut().unwrap().width = 1;
    list.refresh(&rows, MAX_ROWS);
    assert_eq!(list.portraits[&list.heads[0]].identity(), fallback);
}
