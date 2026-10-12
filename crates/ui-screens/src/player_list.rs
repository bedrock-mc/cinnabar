//! Momentary Java-style roster, rendered independently of server HUD replacements.

use std::sync::Arc;

use json_ui::{Catalog, CollectionItem, Context, DataSource, Scalar, ViewState};
use ui::FONT_DESIGN_PIXEL_TEXELS;
use ui_engine::{
    cached_screen::CachedScreen,
    engine::{
        FormEngine, ScreenArt,
        screen::{PaintError, PaintFrame},
    },
};

const MAX_PLAYERS: usize = 80;
const MAX_ROWS: usize = 20;
const ROW_HEIGHT: f64 = 9.0;
const SCREEN: &str = "cinnabar_tab.screen";

/// The roster screen over its own bundled catalog, rebound only when the names change.
pub struct PlayerList {
    catalog: Arc<Catalog>,
    screen: CachedScreen,
    names: Vec<Arc<str>>,
    total: usize,
    data: Arc<DataSource>,
    dimensions: [usize; 2],
    geometry: Option<([f64; 2], f32, [usize; 3])>,
    context: Context,
}

impl PlayerList {
    fn new() -> Self {
        Self {
            catalog: Arc::new(
                Catalog::from_files([
                    ("ui/_global_variables.json", &b"{}"[..]),
                    (
                        "ui/_ui_defs.json",
                        &br#"{"ui_defs":["ui/player_list.json"]}"#[..],
                    ),
                    (
                        "ui/player_list.json",
                        &include_bytes!("../../../assets/java-hud/ui/player_list.json")[..],
                    ),
                ])
                .expect("bundled player-list JSON must be valid"),
            ),
            screen: CachedScreen::default(),
            names: Vec::new(),
            total: 0,
            data: Arc::default(),
            dimensions: [0, 0],
            geometry: None,
            context: Context::default(),
        }
    }

    fn refresh(&mut self, names: &[Arc<str>], rows: usize) -> [usize; 2] {
        let shown = &names[..names.len().min(MAX_PLAYERS)];
        let columns = shown.len().div_ceil(rows).max(1);
        let rows = shown.len().div_ceil(columns).max(1);
        if self.names != shown || self.total != names.len() || self.dimensions != [columns, rows] {
            // Grids consume row-major collections; Java presents names down each column.
            let cells = (0..rows).flat_map(|row| (0..columns).map(move |col| col * rows + row));
            let entries = cells
                .map(|index| {
                    let name = shown.get(index);
                    CollectionItem::default()
                        .with(
                            "#player_name",
                            Scalar::Text(name.map_or_else(String::new, |name| name.to_string())),
                        )
                        .with("#row_visible", Scalar::Bool(name.is_some()))
                })
                .collect();
            self.names = shown.to_vec();
            self.total = names.len();
            self.dimensions = [columns, rows];
            self.geometry = None;
            let mut data = DataSource::new();
            let title = if names.len() > MAX_PLAYERS {
                format!("Online players: {} (first {MAX_PLAYERS})", names.len())
            } else {
                format!("Online players: {}", names.len())
            };
            data.set_global("#online_title", Scalar::Text(title));
            data.set_collection("online_players", entries);
            data.set_grid_dimensions("#players_dimensions", [columns as u32, rows as u32]);
            self.data = Arc::new(data);
        }
        [columns, rows]
    }
}

impl Default for PlayerList {
    fn default() -> Self {
        Self::new()
    }
}

impl PlayerList {
    /// Paints the roster of `names` over the frame.
    pub fn paint(
        &mut self,
        engine: &FormEngine,
        names: &[Arc<str>],
        frame: &mut PaintFrame<'_>,
    ) -> Result<(), PaintError> {
        let px = frame.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let language = frame.language;
        engine.draw(ScreenArt::default(), frame, |env, root| {
            let max_rows = ((root[1] - 40.0) / ROW_HEIGHT)
                .floor()
                .clamp(1.0, MAX_ROWS as f64) as usize;
            let [columns, rows] = self.refresh(names, max_rows);
            let geometry = (root, px, language);
            if self.geometry != Some(geometry) {
                let widest = self
                    .names
                    .iter()
                    .map(|name| env.text.extent(name)[0])
                    .fold(60.0, f64::max);
                let width = (widest + 8.0).min((root[0] - 8.0).max(1.0) / columns as f64);
                let title = format!("Online players: {}", self.total);
                let panel_width = (width * columns as f64 + 4.0)
                    .max(env.text.extent(&title)[0] + 4.0)
                    .min(root[0]);
                self.context = Context::default()
                    .with_var("row_height", serde_json::json!(ROW_HEIGHT))
                    .with_var("row_background_height", serde_json::json!(ROW_HEIGHT - 1.0))
                    .with_var("grid_top", serde_json::json!(ROW_HEIGHT + 3.0))
                    .with_var("cell_width", serde_json::json!(width))
                    .with_var("panel_width", serde_json::json!(panel_width))
                    .with_var(
                        "panel_height",
                        serde_json::json!(ROW_HEIGHT + 5.0 + rows as f64 * ROW_HEIGHT),
                    );
                self.geometry = Some(geometry);
            }
            self.screen.render_shared_with(
                SCREEN,
                &self.catalog,
                &self.context,
                Arc::clone(&self.data),
                (root, px, language),
                env,
                &ViewState::default(),
            )
        })?;
        Ok(())
    }

    /// Bind+layout passes the roster screen ran.
    pub fn passes(&self) -> usize {
        self.screen.passes
    }
}

#[cfg(test)]
mod tests;
