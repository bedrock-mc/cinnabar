use ui::mod_panel::{Icon, Panel, Style};

use super::widgets::row_height;

pub(super) const NAV_HEIGHT: f64 = 28.0;
pub(super) const GAP: f64 = 8.0;
pub(super) const CARD_HEADER: f64 = 30.0;

pub(super) fn top_offset(viewport: [f64; 2]) -> f64 {
    (viewport[1] * 0.14).round().clamp(12.0, 64.0)
}

pub(super) struct Card<'a> {
    pub flat: bool,
    pub icon: Icon,
    pub label: &'a str,
    pub toggle: Option<usize>,
    pub controls: Vec<usize>,
    pub height: f64,
    pub offset: [f64; 2],
}

pub(super) struct Layout<'a> {
    pub width: f64,
    pub card_width: f64,
    pub categories: Vec<&'a str>,
    pub pages: Vec<Vec<Card<'a>>>,
}

impl<'a> Layout<'a> {
    pub fn new(panel: &'a Panel, viewport: [f64; 2], category: usize, rows: usize) -> Self {
        let compact = panel.style == Style::Compact;
        let width = (viewport[0] - 24.0)
            .min(if compact { 440.0 } else { 344.0 })
            .floor();
        let mut categories = Vec::new();
        for section in &panel.sections {
            if !categories.contains(&section.category.as_str()) {
                categories.push(section.category.as_str());
            }
        }
        let selected = categories.get(category).copied();
        let mut cards = Vec::new();
        for section in panel
            .sections
            .iter()
            .filter(|section| Some(section.category.as_str()) == selected)
        {
            let find = |id: &str| {
                panel
                    .controls
                    .iter()
                    .position(|control| control.id() == id)
                    .expect("validated section reference")
            };
            let controls: Vec<_> = section.controls.iter().map(|id| find(id)).collect();
            let height = if compact { 42.0 } else { CARD_HEADER }
                + controls
                    .iter()
                    .map(|index| {
                        if compact {
                            super::compact::row_height(&panel.controls[*index])
                        } else {
                            row_height(&panel.controls[*index])
                        }
                    })
                    .sum::<f64>();
            cards.push(Card {
                flat: false,
                icon: section.icon,
                label: &section.label,
                toggle: section.toggle.as_deref().map(find),
                controls,
                height,
                offset: [0.0; 2],
            });
        }
        let available = viewport[1] - if compact { 78.0 } else { NAV_HEIGHT + 42.0 };
        if cards.is_empty() || cards.iter().any(|card| card.height > available) {
            let indices: Vec<_> = if panel.sections.is_empty() {
                (0..panel.controls.len()).collect()
            } else {
                cards
                    .iter()
                    .flat_map(|card| card.toggle.into_iter().chain(card.controls.iter().copied()))
                    .collect()
            };
            let rows = rows.max(1);
            if compact {
                let mut pages = Vec::new();
                let mut controls = Vec::new();
                let mut height = 13.0;
                for index in indices {
                    let next = super::compact::row_height(&panel.controls[index]);
                    if !controls.is_empty() && (height + next > available || controls.len() == rows)
                    {
                        pages.push(vec![Card {
                            flat: true,
                            icon: Icon::None,
                            label: selected.unwrap_or("Controls"),
                            toggle: None,
                            controls: std::mem::take(&mut controls),
                            height,
                            offset: [10.0, NAV_HEIGHT + GAP + 10.0],
                        }]);
                        height = 13.0;
                    }
                    controls.push(index);
                    height += next;
                }
                if !controls.is_empty() || pages.is_empty() {
                    pages.push(vec![Card {
                        flat: true,
                        icon: Icon::None,
                        label: selected.unwrap_or("Controls"),
                        toggle: None,
                        controls,
                        height,
                        offset: [10.0, NAV_HEIGHT + GAP + 10.0],
                    }]);
                }
                return Self {
                    width,
                    card_width: width - 20.0,
                    categories,
                    pages,
                };
            }
            let mut pages: Vec<_> = indices
                .chunks(rows)
                .map(|controls| {
                    vec![Card {
                        flat: false,
                        icon: Icon::None,
                        label: selected.unwrap_or("Controls"),
                        toggle: None,
                        controls: controls.to_vec(),
                        height: CARD_HEADER + controls.len() as f64 * 26.0,
                        offset: [0.0, NAV_HEIGHT + GAP],
                    }]
                })
                .collect();
            if pages.is_empty() {
                pages.push(vec![Card {
                    flat: false,
                    icon: Icon::None,
                    label: "Controls",
                    toggle: None,
                    controls: Vec::new(),
                    height: CARD_HEADER,
                    offset: [0.0, NAV_HEIGHT + GAP],
                }]);
            }
            return Self {
                width,
                card_width: width,
                categories,
                pages,
            };
        }
        let columns = if compact && width >= 375.0 {
            3
        } else if width >= 300.0 {
            2
        } else {
            1
        };
        let inset = if compact { 10.0 } else { 0.0 };
        let card_width = (width - 2.0 * inset - (columns - 1) as f64 * GAP) / columns as f64;
        let mut pages: Vec<Vec<Card<'a>>> = vec![Vec::new()];
        let mut row = Vec::new();
        let mut y = NAV_HEIGHT + GAP;
        for card in cards {
            row.push(card);
            if row.len() == columns {
                append_row(&mut pages, &mut row, &mut y, available, card_width);
            }
        }
        append_row(&mut pages, &mut row, &mut y, available, card_width);
        if compact {
            for card in pages.iter_mut().flatten() {
                card.offset[0] += inset;
                card.offset[1] += 10.0;
            }
        }
        Self {
            width,
            card_width,
            categories,
            pages,
        }
    }

    pub fn height(&self, page: usize) -> f64 {
        self.pages[page]
            .iter()
            .map(|card| card.offset[1] + card.height)
            .fold(NAV_HEIGHT, f64::max)
            + if self.pages.len() > 1 { 22.0 } else { 0.0 }
    }
}

fn append_row<'a>(
    pages: &mut Vec<Vec<Card<'a>>>,
    row: &mut Vec<Card<'a>>,
    y: &mut f64,
    available: f64,
    width: f64,
) {
    let height = row.iter().map(|card| card.height).fold(0.0, f64::max);
    if !pages.last().unwrap().is_empty() && *y + height > NAV_HEIGHT + GAP + available {
        pages.push(Vec::new());
        *y = NAV_HEIGHT + GAP;
    }
    for (column, mut card) in row.drain(..).enumerate() {
        card.height = height;
        card.offset = [column as f64 * (width + GAP), *y];
        pages.last_mut().unwrap().push(card);
    }
    *y += height + GAP;
}
