//! Fixtures for tests that inspect OreUI's drawn nodes, here and in the adapter crates.

use std::{collections::HashMap, sync::Arc};

use launcher::dressing_room::{
    DressingRoomCape, DressingRoomSkin, DressingRoomView, STARTER_SKIN_NAMES, SkinModel,
};
use launcher::menu::MenuView;
use ui::{UiNode, UiPoint, UiRect};

use crate::images::{
    OreUiImages, OreUiPage, OreUiSprite, SETTINGS_ICON_HIGHLIGHT_IMAGE, SETTINGS_ICONS,
};

/// Every solid fill as window bounds and colour, in draw order.
pub fn solids(nodes: &[UiNode]) -> Vec<([f32; 4], [u8; 4])> {
    let origin = |mut id: Option<ui::UiNodeId>| {
        let mut at = [0.0, 0.0];
        while let Some(node) = id.and_then(|id| nodes.iter().find(|node| node.id() == id)) {
            let min = node.bounds().min();
            at = [at[0] + min.x(), at[1] + min.y()];
            id = node.parent();
        }
        at
    };
    nodes
        .iter()
        .filter_map(|node| match node.visual() {
            ui::UiVisual::Solid { color, .. } => {
                let [x, y] = origin(node.parent());
                let (min, max) = (node.bounds().min(), node.bounds().max());
                Some(([min.x() + x, min.y() + y, max.x() + x, max.y() + y], *color))
            }
            _ => None,
        })
        .collect()
}

/// Every settings category icon as a 24px cell on page 0, and the highlight sweep's nine
/// frames on page 1.
pub fn settings_icon_images() -> OreUiImages {
    let mut sprites: HashMap<_, _> = SETTINGS_ICONS
        .iter()
        .map(|key| {
            (
                (*key).into(),
                OreUiSprite {
                    page: 0,
                    bounds: [0, 0, 24, 24],
                },
            )
        })
        .collect();
    sprites.insert(
        SETTINGS_ICON_HIGHLIGHT_IMAGE.into(),
        OreUiSprite {
            page: 1,
            bounds: [0, 0, 216, 24],
        },
    );
    OreUiImages {
        pages: vec![
            OreUiPage {
                dimensions: [24; 2],
                pixels: vec![255; 24 * 24 * 4].into(),
            },
            OreUiPage {
                dimensions: [216, 24],
                pixels: vec![255; 216 * 24 * 4].into(),
            },
        ],
        sprites: Arc::new(sprites),
        loading_frames: Default::default(),
        animations: Default::default(),
    }
}

/// The dressing room over `count` skins: the starters first, then imported ones.
pub fn dressing_room_view(count: usize) -> MenuView {
    let mut view = MenuView::new(true, "Fixture".into());
    view.screen = launcher::menu::MenuScreen::DressingRoom;
    view.dressing_room = Arc::new(DressingRoomView {
        skins: (0..count)
            .map(|index| DressingRoomSkin {
                engine_version: render_api::DEFAULT_SKIN_GEOMETRY_ENGINE_VERSION.into(),
                id: index.to_string(),
                name: STARTER_SKIN_NAMES
                    .get(index)
                    .map_or_else(|| format!("Skin {index}"), |name| (*name).into()),
                path: String::new(),
                imported: index >= STARTER_SKIN_NAMES.len(),
                model: if index == 1 {
                    SkinModel::Slim
                } else {
                    SkinModel::Classic
                },
                skin: render_api::StandardSkin {
                    width: render_api::CLASSIC_SKIN_SIDE as u32,
                    height: render_api::CLASSIC_SKIN_SIDE as u32,
                    rgba8: vec![255; render_api::CLASSIC_SKIN_SIDE.pow(2) * 4].into(),
                    cape: None,
                    geometry: None,
                },
            })
            .collect::<Vec<_>>()
            .into(),
        selected: Some(0),
        ..Default::default()
    });
    view
}

/// Adds a starter cape and an imported one.
pub fn add_capes(view: &mut MenuView) {
    let (width, height) = render_api::CAPE_DIMENSIONS[0];
    Arc::make_mut(&mut view.dressing_room).capes = (0..2)
        .map(|index| DressingRoomCape {
            id: format!("cape-{index}"),
            name: format!("Cape {index}"),
            path: String::new(),
            cape: render_api::CapeImage {
                width,
                height,
                rgba8: vec![255; (width * height * 4) as usize].into(),
            },
            imported: index > 0,
        })
        .collect::<Vec<_>>()
        .into();
}

/// A signed-in view with the account picker open over two saved accounts, the first active.
pub fn account_manager_view() -> MenuView {
    let mut view = MenuView::new(true, "First".into());
    view.auth_state = launcher::menu::auth::AuthState::Authenticated;
    view.dialog = Some(launcher::menu::MenuDialog::Accounts);
    view.feeds.account_active_id = Some("first".into());
    view.feeds.accounts = ["first", "second"]
        .map(|id| launcher::accounts::AccountProfile {
            id: id.into(),
            gamertag: id.into(),
            picture_path: None,
        })
        .into();
    view
}

/// The settings screen opened on `section`, a selector variable name.
pub fn settings_view(section: &str) -> MenuView {
    let mut view = MenuView::new(true, "Player".into());
    view.screen = launcher::menu::MenuScreen::Settings;
    view.settings.section = launcher::menu::settings_sections::SETTINGS_SECTIONS
        .iter()
        .find_map(|(name, index)| (*name == section).then_some(*index))
        .expect("known settings section");
    view
}

/// The centre of `bounds`.
pub fn centre(bounds: UiRect) -> UiPoint {
    let (min, max) = (bounds.min(), bounds.max());
    UiPoint::new((min.x() + max.x()) * 0.5, (min.y() + max.y()) * 0.5).unwrap()
}

/// The index of the registered setting `name`.
pub fn option_index(name: &str) -> usize {
    launcher::menu::settings_options::SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == name)
        .expect("registered option")
}
