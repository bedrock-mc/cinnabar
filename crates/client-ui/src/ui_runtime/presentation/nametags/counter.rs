//! Counter runs share the eligible name's font atlas and native depth passes.

use super::super::nametag_atlas::AtlasLine;
use super::*;
use std::sync::LazyLock;
use ui::mod_hud::CounterAnnotation;

static SPACE: LazyLock<Arc<str>> = LazyLock::new(|| Arc::from(" "));

/// Pins counter glyphs before this frame captures any atlas UVs.
pub(super) fn texts(counter: &CounterAnnotation) -> [&Arc<str>; 3] {
    [&counter.prefix, &counter.value, &SPACE]
}

/// Reserves colored text and an optional separate backing plate.
pub(super) fn record_budget(
    counter: &CounterAnnotation,
    style: Option<&ui::mod_hud::NametagStyle>,
) -> usize {
    (1 + usize::from(!counter.prefix.is_empty())) * style::record_cost(style, false)
        + usize::from(counter.separate) * style::record_cost(style, true)
}

pub(super) struct Layout {
    policy: Arc<CounterAnnotation>,
    prefix: Option<AtlasLine>,
    value: AtlasLine,
    pub(super) gap: f32,
}

impl Layout {
    /// Resolves cached cells; exhaustion omits the counter without displacing its name.
    pub(super) fn measure<'p>(
        policy: &Arc<CounterAnnotation>,
        atlas: &mut NametagAtlas,
        font: &RuntimeFontCatalog,
        layouts: &mut TextLayoutCache,
        pages: &impl Fn(usize) -> Option<GlyphPage<'p>>,
    ) -> Option<Self> {
        let prefix = if policy.prefix.is_empty() {
            None
        } else {
            Some(atlas.line(&policy.prefix, font, layouts, pages)?)
        };
        Some(Self {
            policy: Arc::clone(policy),
            prefix,
            value: atlas.line(&policy.value, font, layouts, pages)?,
            gap: atlas.line(&SPACE, font, layouts, pages)?.width_px,
        })
    }

    /// Uses actual prefix and value advances rather than estimated character widths.
    pub(super) fn width(&self) -> f32 {
        self.prefix.map_or(0., |p| p.width_px) + self.value.width_px
    }

    /// A separate counter occupies one line instead of extending the first name line.
    pub(super) fn separate(&self) -> bool {
        self.policy.separate
    }

    /// Selects placement before the name or its multiline block.
    pub(super) fn before(&self) -> bool {
        self.policy.before
    }

    /// Emits independently tinted runs before the existing depth/alpha pass construction.
    pub(super) fn append(
        &self,
        records: &mut Vec<NametagRecord>,
        common: NametagRecord,
        mut left: f32,
        index: usize,
        alpha: f32,
    ) {
        let side = NAMETAG_ATLAS_SIDE as f32;
        let texels = FONT_DESIGN_PIXEL_TEXELS as f32;
        for (line, rgba) in self
            .prefix
            .into_iter()
            .map(|p| (p, self.policy.prefix_color))
            .chain(std::iter::once((self.value, self.policy.value_color)))
        {
            let [x, y, width, height] = line.cell.map(|v| v as f32);
            let top = NAMETAG_LINE_PITCH_PX * index as f32 + line.top_px;
            let mut color = rgba.map(|v| f32::from(v) / 255.);
            color[3] *= alpha;
            records.push(NametagRecord {
                text: 1,
                rect: [left, top, left + width / texels, top + height / texels],
                uv: [x / side, y / side, (x + width) / side, (y + height) / side],
                color,
                ..common
            });
            left += line.width_px;
        }
    }
}
