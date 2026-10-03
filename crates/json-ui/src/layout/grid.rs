//! Grids as the client lays them out: effective dimensions from
//! `grid_dimensions`, `grid_rescaling_type` or `grid_fill_direction`, a
//! templated grid's own size (dimensions × template), and each cell's size and
//! position. Cells ignore their own anchors and `offset`.

use serde_json::Value;

use super::{Axis, LayoutEnv, Rect, ResolvedControl, axis_index, size};
use crate::expr::{Length, Unit};

/// Marks the retained `grid_item_template` the binder appends to a templated
/// grid: measured for the grid's rules, never placed.
pub const TEMPLATE_KEY: &str = "grid_template_node";

#[derive(Clone, Copy, PartialEq)]
enum Direction {
    Horizontal,
    Vertical,
    None,
}

fn direction(control: &ResolvedControl, key: &str) -> Direction {
    match control.properties.get(key).and_then(Value::as_str) {
        Some("horizontal") => Direction::Horizontal,
        Some("vertical") => Direction::Vertical,
        _ => Direction::None,
    }
}

pub(super) fn is_grid(control: &ResolvedControl) -> bool {
    control.control_type.as_deref() == Some("grid")
}

/// Whether the grid builds its cells from a `grid_item_template`.
pub(super) fn has_template(control: &ResolvedControl) -> bool {
    is_grid(control)
        && control
            .properties
            .get("grid_item_template")
            .and_then(Value::as_str)
            .is_some_and(|template| !template.is_empty())
}

pub(crate) fn is_template_node(control: &ResolvedControl) -> bool {
    control.properties.get(TEMPLATE_KEY) == Some(&Value::Bool(true))
}

/// The retained template among a grid's children, else its first cell (an
/// instance of it).
fn template(control: &ResolvedControl) -> Option<&ResolvedControl> {
    control
        .children
        .iter()
        .rev()
        .find(|child| is_template_node(child))
        .or_else(|| control.children.first())
}

/// A grid's cells: its children without the retained template.
pub(super) fn cells(control: &ResolvedControl) -> impl Iterator<Item = (usize, &ResolvedControl)> {
    control
        .children
        .iter()
        .enumerate()
        .filter(|(_, child)| !is_template_node(child))
}

/// A whole number (a bound value arrives as a float), else `None`.
fn int(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| {
        value
            .as_f64()
            .filter(|number| number.fract() == 0.0)
            .map(|number| number as i64)
    })
}

fn int_pair(control: &ResolvedControl, key: &str) -> Option<[f64; 2]> {
    let pair = control.properties.get(key)?.as_array()?;
    let int = |value: Option<&Value>| value.and_then(int).unwrap_or(0) as f64;
    Some([int(pair.first()), int(pair.get(1))])
}

/// `maximum_grid_items`, a bound `#maximum_grid_items` winning; only an integer
/// counts, else zero.
pub(crate) fn maximum_items(control: &ResolvedControl) -> usize {
    ["#maximum_grid_items", "maximum_grid_items"]
        .iter()
        .find_map(|key| control.properties.get(*key))
        .and_then(int)
        .map_or(0, |max| max.max(0) as usize)
}

/// Whether the cell count follows `maximum_grid_items` rather than dimensions.
pub(crate) fn rescales(control: &ResolvedControl) -> bool {
    direction(control, "grid_rescaling_type") != Direction::None
}

/// The fixed `[columns, rows]` from `grid_dimensions`, zero when unset.
pub(crate) fn fixed_dimensions(control: &ResolvedControl) -> [f64; 2] {
    int_pair(control, "grid_dimensions").unwrap_or([0.0; 2])
}

/// `[columns, rows]` for a grid of `grid` extent over cells of `cell` extent.
fn dimensions(control: &ResolvedControl, grid: [f64; 2], cell: [f64; 2]) -> [f64; 2] {
    let fit = |axis: usize| {
        if cell[axis] == 0.0 {
            1.0
        } else {
            (grid[axis] / cell[axis]).floor().max(1.0)
        }
    };
    match direction(control, "grid_rescaling_type") {
        Direction::Horizontal => {
            let columns = fit(0);
            let rows = (maximum_items(control) as f64 / columns).max(1.0).ceil();
            [columns, rows]
        }
        Direction::Vertical => {
            let rows = fit(1);
            [(maximum_items(control) as f64 / rows).ceil(), rows]
        }
        Direction::None => match direction(control, "grid_fill_direction") {
            Direction::Horizontal => [fit(0).ceil(), 1.0],
            Direction::Vertical => [1.0, fit(1).ceil()],
            Direction::None => fixed_dimensions(control),
        },
    }
}

/// How many cells the grid holds: `maximum_grid_items` when rescaling, else
/// columns × rows.
fn capacity(control: &ResolvedControl, dims: [f64; 2]) -> usize {
    if rescales(control) {
        maximum_items(control)
    } else {
        (dims[0].max(0.0) * dims[1].max(0.0)) as usize
    }
}

/// The template's `[w, h]` under a grid whose extent is known on `grid` axes.
fn template_size(control: &ResolvedControl, grid: [Option<f64>; 2], env: &LayoutEnv) -> [f64; 2] {
    template(control).map_or([0.0; 2], |template| {
        size::resolve_size(template, grid, [0.0; 2], env)
    })
}

/// A templated grid's size on `axis` when it is `default` or `%c`-led:
/// `scale × dimension × template`, where `scale` is 1 for `default` plus each
/// `%c` fraction. `None` leaves the ordinary rule in force.
pub(super) fn own_size(
    control: &ResolvedControl,
    axis: Axis,
    own: [Option<f64>; 2],
    env: &LayoutEnv,
) -> Option<f64> {
    if !has_template(control) {
        return None;
    }
    let scale = size::with_size(control, axis, |length| match length? {
        Length::Default => Some(1.0),
        Length::Terms(terms) => Some(
            terms
                .iter()
                .filter(|term| term.unit == Unit::PercentChildren)
                .map(|term| term.coeff / 100.0)
                .sum(),
        ),
        Length::Fill => Some(0.0),
    })?;
    if scale <= 0.0 {
        return None;
    }
    let cell = template_size(control, own, env);
    let dims = dimensions(
        control,
        [own[0].unwrap_or(0.0), own[1].unwrap_or(0.0)],
        cell,
    );
    let index = axis_index(axis);
    Some(scale * dims[index] * cell[index])
}

/// Every child's `[w, h]` in a grid of `extent`: a cell whose size axis has no
/// expression (absent, `default`, `fill`) takes the even share of the grid on
/// that axis. The retained template and cells past capacity measure zero.
pub(super) fn child_sizes(
    parent: &ResolvedControl,
    extent: [Option<f64>; 2],
    env: &LayoutEnv,
) -> Vec<[f64; 2]> {
    let layout = Layout::of(parent, extent, env);
    let mut sizes = vec![[0.0; 2]; parent.children.len()];
    for (index, child) in cells(parent).take(layout.limit) {
        sizes[index] = layout.cell_size(child, extent, env);
    }
    sizes
}

/// A grid's solved dimensions for one extent.
struct Layout {
    extent: [f64; 2],
    dims: [f64; 2],
    limit: usize,
    templated: bool,
    rescaling: Option<usize>,
}

impl Layout {
    fn of(parent: &ResolvedControl, extent: [Option<f64>; 2], env: &LayoutEnv) -> Self {
        let templated = has_template(parent);
        let cell = if templated {
            template_size(parent, extent, env)
        } else {
            [0.0; 2]
        };
        let extent = [extent[0].unwrap_or(0.0), extent[1].unwrap_or(0.0)];
        let dims = dimensions(parent, extent, cell);
        Self {
            extent,
            dims,
            limit: if templated {
                capacity(parent, dims)
            } else {
                usize::MAX
            },
            templated,
            rescaling: match direction(parent, "grid_rescaling_type") {
                Direction::Horizontal => Some(0),
                Direction::Vertical => Some(1),
                Direction::None => None,
            },
        }
    }

    /// The grid's extent over its dimension on `axis`, zero for a zero dimension.
    fn even(&self, axis: usize) -> f64 {
        if self.dims[axis].abs() < f32::EPSILON as f64 {
            0.0
        } else {
            self.extent[axis] / self.dims[axis]
        }
    }

    fn cell_size(
        &self,
        child: &ResolvedControl,
        extent: [Option<f64>; 2],
        env: &LayoutEnv,
    ) -> [f64; 2] {
        let mut size = size::resolve_size(child, extent, [0.0; 2], env);
        for (index, axis) in [Axis::X, Axis::Y].into_iter().enumerate() {
            let expression = size::with_size(
                child,
                axis,
                |length| matches!(length, Some(Length::Terms(terms)) if !terms.is_empty()),
            );
            if !expression {
                size[index] = self.even(index);
            }
        }
        size
    }

    /// A cell's `[column, row]`: row-major by creation order in a templated
    /// grid, else its `grid_position`.
    fn position(
        &self,
        child: &ResolvedControl,
        index: usize,
        count: usize,
        size: [f64; 2],
    ) -> [f64; 2] {
        if !self.templated {
            return int_pair(child, "grid_position").unwrap_or([0.0; 2]);
        }
        let fit = |axis: usize| {
            if size[axis] == 0.0 {
                1.0
            } else {
                (self.extent[axis] / size[axis]).floor().max(1.0)
            }
        };
        let columns = if self.rescaling == Some(1) {
            (count as f64 / fit(1)).ceil()
        } else {
            fit(0)
        };
        let columns = columns.max(1.0) as usize;
        [(index % columns) as f64, (index / columns) as f64]
    }

    /// A cell's offset from the grid's origin on `axis`: whole cells along a
    /// rescaling axis, centred in the leftover, else even shares.
    fn offset(&self, axis: usize, at: [f64; 2], size: [f64; 2]) -> f64 {
        if self.rescaling == Some(axis) {
            let centre = if size[axis].abs() < f32::EPSILON as f64 {
                0.0
            } else {
                (self.extent[axis] % size[axis]) * 0.5
            };
            size[axis] * at[axis] + centre
        } else {
            self.even(axis) * at[axis]
        }
    }
}

/// The rects of a grid's cells within `grid`, relative to its parent's origin.
/// Cells past a templated grid's capacity are not created.
pub(super) fn grid_children<'a>(
    parent: &'a ResolvedControl,
    grid: Rect,
    env: &LayoutEnv,
) -> Vec<(&'a ResolvedControl, Rect)> {
    let extent = [Some(grid.w), Some(grid.h)];
    let layout = Layout::of(parent, extent, env);
    let sizes = super::measure::sizes(parent, extent, env);
    let count = cells(parent).take(layout.limit).count();
    cells(parent)
        .take(layout.limit)
        .enumerate()
        .map(|(order, (index, child))| {
            let size = sizes[index];
            let at = layout.position(child, order, count, size);
            let rect = Rect::new(
                grid.x + layout.offset(0, at, size),
                grid.y + layout.offset(1, at, size),
                size[0],
                size[1],
            );
            (child, rect)
        })
        .collect()
}
