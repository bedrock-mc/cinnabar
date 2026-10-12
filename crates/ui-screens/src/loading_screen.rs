//! World entry and dimension changes share a dimension-specific progress screen.

use std::sync::Arc;

use std::collections::{BTreeMap, HashMap};

use json_ui::{DataSource, Scalar, ViewState};
use ui::{FONT_DESIGN_PIXEL_TEXELS, IconRef, LoadingStage};
use ui_engine::cached_screen::CachedScreen;
use ui_engine::engine::{
    FormEngine, ScreenArt,
    screen::{PaintError, PaintFrame},
};

/// The world-loading screens by dimension: overworld, nether, the end.
pub const LOADING_SCREENS: [&str; 3] = [
    "progress.overworld_loading_progress_screen",
    "progress.nether_loading_progress_screen",
    "progress.theend_loading_progress_screen",
];
pub const LOADING_SCREEN: &str = LOADING_SCREENS[0];

const DIMENSION_TITLES: [(&str, &str); 3] = [
    (
        "cinnabar.progress.enteringOverworld",
        "Entering the Overworld",
    ),
    ("cinnabar.progress.enteringNether", "Entering the Nether"),
    ("cinnabar.progress.enteringEnd", "Entering the End"),
];

/// The shipped brand layout stays below server UI files, as the other menus do.
pub fn install_brand_layout(catalog: &mut json_ui::Catalog) {
    catalog.overlay_text("ui/cinnabar_loading_brand.json", BRAND_LAYOUT);
}

// Keep the pack's title and backdrop layout; native components replace the modal.
const BRAND_LAYOUT: &str = r##"{
  "namespace": "progress",
  "cinnabar_loading_group": {
    "type": "stack_panel", "orientation": "vertical", "use_child_anchors": true,
    "size": ["100%", "100%c"],
    "anchor_from": "top_middle", "anchor_to": "top_middle",
    "offset": [0, "15%"],
    "controls": [
      {"cinnabar_loading_title@common_art.title_image": {
        "size": ["55%", 64], "keep_ratio": true,
        "anchor_from": "top_middle", "anchor_to": "top_middle"
      }},
      {"cinnabar_loading_title_padding": {"type": "panel", "size": [0, 16]}},
      {"world_modal_progress_panel@progress.world_modal_progress_panel": {
        "$modal_button_panel_type": "progress.modal_button_panel"
      }}
    ]
  },
  "world_convert_modal_progress_screen_content": {
    "modifications": [
      {"array_name": "controls", "operation": "replace", "control_name": "title_panel_content",
       "value": {"cinnabar_loading_group@progress.cinnabar_loading_group": {}}},
      {"array_name": "controls", "operation": "remove", "control_name": "world_modal_progress_panel"}
    ]
  }
}"##;

/// What one loading paint reads.
pub struct LoadingInput<'a> {
    pub stage: LoadingStage,
    /// The stream's current dimension, which picks the backdrop.
    pub dimension: Option<usize>,
    pub clocks: &'a BTreeMap<String, f64>,
    /// Images drawn by key ahead of the atlas.
    pub images: &'a HashMap<String, IconRef>,
}

/// Paints the world-loading screen for the input's stage over its cached `screen`; when it
/// drew, returns the title and detail the native progress components show.
pub fn paint(
    screen: &mut CachedScreen,
    engine: &FormEngine,
    input: LoadingInput<'_>,
    frame: &mut PaintFrame<'_>,
) -> Result<Option<[String; 2]>, PaintError> {
    let LoadingInput {
        stage,
        dimension,
        clocks,
        images,
    } = input;
    let reference = dimension
        .and_then(|dimension| LOADING_SCREENS.get(dimension))
        .copied()
        .unwrap_or(LOADING_SCREEN);
    let translate = frame.translate;
    let text = |key: &str, fallback: &str| {
        translate(key).map_or_else(|| fallback.to_owned(), |text| text.to_string())
    };
    let mut data = DataSource::new();
    data.set_strict(true);
    // Vanilla's world generation handler titles the terrain wait.
    let title = match stage {
        LoadingStage::Connecting => text(
            "progressScreen.title.connectingExternal",
            "Connecting to external server",
        ),
        LoadingStage::BuildingTerrain | LoadingStage::ChangingDimension => {
            text("progressScreen.generating", "Generating World")
        }
    };
    let destination_title = (stage == LoadingStage::ChangingDimension).then(|| {
        let (key, fallback) = dimension
            .and_then(|dimension| DIMENSION_TITLES.get(dimension))
            .copied()
            .unwrap_or(("cinnabar.progress.changingDimension", "Changing dimension"));
        text(key, fallback)
    });
    let detail = match stage {
        LoadingStage::Connecting => text("progressScreen.message.locating", "Locating server"),
        LoadingStage::BuildingTerrain | LoadingStage::ChangingDimension => {
            text("progressScreen.message.building", "Building terrain")
        }
    };
    data.set_global("#title_text", Scalar::Text(title.clone()));
    data.set_global("#progress_text", Scalar::Text(detail.clone()));
    data.set_global(
        "#bar_animation_visible",
        Scalar::Bool(stage != LoadingStage::ChangingDimension),
    );
    data.set_global("#loading_bar_visible", Scalar::Bool(false));
    let context = engine.context().clone();
    let catalog = Arc::clone(engine.catalog());
    let px = frame.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
    let language = frame.language;
    let view = ViewState::default();
    let drawn = engine.draw(
        ScreenArt {
            omit_controls: &[
                "world_modal_progress_panel",
                "modal_progress_panel",
                "title_panel_content",
                "cinnabar_loading_title",
                "title_panel",
                "realms_title_image",
            ],
            clocks: Some(clocks),
            images: Some(images),
            ..ScreenArt::default()
        },
        frame,
        |env, root| {
            screen.render_with(
                reference,
                &catalog,
                &context,
                data,
                (root, px, language),
                env,
                &view,
            )
        },
    )?;
    Ok(drawn.map(|_| [destination_title.unwrap_or(title), detail]))
}
