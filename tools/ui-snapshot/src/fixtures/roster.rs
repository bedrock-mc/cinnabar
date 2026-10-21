//! Anonymous player rosters with badges and original head artwork.

use super::Fixture;
use std::sync::Arc;
use ui_screens::player_list::RosterRow;

/// Badges two Cinnabar players, one of them sharing a name with an unbadged player.
pub(super) fn badges(visible: bool) -> Fixture {
    Fixture::TabList(
        [
            ("Alex", true),
            ("Alex", false),
            ("Riley", false),
            ("Steve", visible),
            ("Zoe", false),
        ]
        .map(|(name, badged)| RosterRow {
            name: Arc::from(name),
            badged,
            annotation: None,
            skin: None,
        })
        .into(),
    )
}

/// Distinct original head texels alongside missing and invalid skins that must show Steve.
pub(super) fn heads() -> Fixture {
    let Fixture::TabList(mut rows) = badges(true) else {
        unreachable!()
    };
    for (index, row) in rows
        .iter_mut()
        .enumerate()
        .filter(|(i, _)| matches!(i, 0 | 2))
    {
        let side = assets::starter_skins::STARTER_SKIN_SIDE;
        let mut pixels = vec![0; (side * side * 4) as usize];
        for y in 8..16 {
            for x in 8..16 {
                let color = if (y == 11 || y == 12) && (x == 9 || x == 10 || x == 13 || x == 14) {
                    [240, 240, 255, 255]
                } else if index == 0 {
                    [60, 155, 100, 255]
                } else {
                    [110, 80, 180, 255]
                };
                let offset = ((y * side + x) * 4) as usize;
                pixels[offset..offset + 4].copy_from_slice(&color);
                if y < 10 {
                    let hat = ((y * side + x + 32) * 4) as usize;
                    pixels[hat..hat + 4].copy_from_slice(&[35, 60, 110, 255]);
                }
            }
        }
        row.skin = Some(render_api::StandardSkin {
            width: side,
            height: side,
            rgba8: pixels.into(),
            cape: None,
            geometry: None,
        });
    }
    rows.last_mut().unwrap().skin = Some(render_api::StandardSkin {
        width: 1,
        height: 1,
        rgba8: vec![0; 4].into(),
        cape: None,
        geometry: None,
    });
    Fixture::TabList(rows)
}

/// Exercises every roster column at the maximum list size and a larger GUI scale.
pub(super) fn head_columns() -> Fixture {
    let Fixture::TabList(rows) = heads() else {
        unreachable!()
    };
    Fixture::TabList(
        (0..80)
            .map(|index| {
                let mut row = rows[index % rows.len()].clone();
                row.name = format!("Player{index:02}").into();
                row
            })
            .collect(),
    )
}
