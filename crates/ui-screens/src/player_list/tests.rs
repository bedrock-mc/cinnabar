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
    }
}

fn names(count: usize) -> Vec<RosterRow> {
    (0..count)
        .map(|index| row(&format!("Player{index:02}"), false))
        .collect()
}

/// Renders the list's current data at a fixed test geometry.
fn render(list: &mut PlayerList) -> Vec<json_ui::DrawNode> {
    let context = Context::default()
        .with_var("row_height", serde_json::json!(ROW_HEIGHT))
        .with_var("row_background_height", serde_json::json!(ROW_HEIGHT - 1.0))
        .with_var("grid_top", serde_json::json!(ROW_HEIGHT + 3.0))
        .with_var("cell_width", serde_json::json!(80))
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

/// Counts rendered badge sprites.
fn badges(nodes: &[json_ui::DrawNode]) -> usize {
    nodes
        .iter()
        .filter(
            |node| matches!(&node.draw, Draw::Sprite { texture, .. } if texture == CINNABAR_BADGE),
        )
        .count()
}
