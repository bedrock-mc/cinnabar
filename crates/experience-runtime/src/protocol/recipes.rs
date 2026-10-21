//! The server's crafting-table recipes, which the adapter sends with `load` and server WIT 0.6's
//! `crafting-result` matches a guest's grid against.

use serde::{Deserialize, Serialize};

/// An item a recipe input accepts: its id, and its metadata or, when none, any metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemMatch {
    pub id: String,
    pub metadata: Option<u16>,
}

/// What a recipe makes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Crafted {
    pub id: String,
    pub metadata: u16,
    pub count: u8,
}

/// A crafting-table recipe. A shaped one has its `shape`, width then height, and `inputs` row by
/// row, an empty list for an empty cell; a shapeless one has no shape and an input per item it
/// takes. Each input lists the items it accepts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub shape: Option<[u8; 2]>,
    pub inputs: Vec<Vec<ItemMatch>>,
    pub output: Crafted,
}

impl ItemMatch {
    fn accepts(&self, id: &str, metadata: u16) -> bool {
        self.id == id && self.metadata.is_none_or(|own| own == metadata)
    }
}

/// The side of the crafting grid.
pub const GRID_SIDE: usize = 3;

impl Recipe {
    /// Whether the recipe takes `grid`, nine cells row by row, each `(id, metadata)` or none: a
    /// shaped recipe its shape anywhere in the grid, as drawn or mirrored left to right, every
    /// other cell empty; a shapeless one exactly its inputs, in any cells.
    pub fn matches(&self, grid: &[Option<(&str, u16)>]) -> bool {
        if grid.len() != GRID_SIDE * GRID_SIDE {
            return false;
        }
        match self.shape {
            Some(shape) => self.matches_shaped(shape, grid),
            None => self.matches_shapeless(grid),
        }
    }

    fn matches_shaped(&self, [width, height]: [u8; 2], grid: &[Option<(&str, u16)>]) -> bool {
        let (width, height) = (usize::from(width), usize::from(height));
        if width == 0 || height == 0 || width > GRID_SIDE || height > GRID_SIDE {
            return false;
        }
        if self.inputs.len() != width * height {
            return false;
        }
        for top in 0..=GRID_SIDE - height {
            for left in 0..=GRID_SIDE - width {
                for mirrored in [false, true] {
                    if self.fits(grid, width, height, top, left, mirrored) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Whether the shape, placed with its top left at `(left, top)`, takes every cell.
    fn fits(
        &self,
        grid: &[Option<(&str, u16)>],
        width: usize,
        height: usize,
        top: usize,
        left: usize,
        mirrored: bool,
    ) -> bool {
        (0..GRID_SIDE * GRID_SIDE).all(|cell| {
            let (row, column) = (cell / GRID_SIDE, cell % GRID_SIDE);
            let inside =
                (top..top + height).contains(&row) && (left..left + width).contains(&column);
            let input = inside.then(|| {
                let x = column - left;
                let x = if mirrored { width - 1 - x } else { x };
                &self.inputs[(row - top) * width + x]
            });
            match (input, grid[cell]) {
                (None, None) => true,
                (None, Some(_)) => false,
                (Some(options), None) => options.is_empty(),
                (Some(options), Some((id, metadata))) => {
                    options.iter().any(|option| option.accepts(id, metadata))
                }
            }
        })
    }

    fn matches_shapeless(&self, grid: &[Option<(&str, u16)>]) -> bool {
        let items: Vec<(&str, u16)> = grid.iter().flatten().copied().collect();
        if items.len() != self.inputs.len() || items.is_empty() {
            return false;
        }
        let mut used = vec![false; items.len()];
        assign(&self.inputs, &items, &mut used)
    }
}

/// Whether each of `inputs` takes a distinct one of `items`, searched in order.
fn assign(inputs: &[Vec<ItemMatch>], items: &[(&str, u16)], used: &mut [bool]) -> bool {
    let Some((first, rest)) = inputs.split_first() else {
        return true;
    };
    for (at, &(id, metadata)) in items.iter().enumerate() {
        if !used[at] && first.iter().any(|option| option.accepts(id, metadata)) {
            used[at] = true;
            if assign(rest, items, used) {
                return true;
            }
            used[at] = false;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str) -> ItemMatch {
        ItemMatch {
            id: id.to_owned(),
            metadata: None,
        }
    }

    fn crafted(id: &str, count: u8) -> Crafted {
        Crafted {
            id: id.to_owned(),
            metadata: 0,
            count,
        }
    }

    /// An axe: a 2×3 shape, so it fits two columns and mirrors.
    fn axe() -> Recipe {
        let planks = || vec![item("minecraft:oak_planks")];
        let stick = || vec![item("minecraft:stick")];
        Recipe {
            shape: Some([2, 3]),
            inputs: vec![planks(), planks(), planks(), stick(), vec![], stick()],
            output: crafted("minecraft:wooden_axe", 1),
        }
    }

    fn grid(cells: [Option<&'static str>; 9]) -> Vec<Option<(&'static str, u16)>> {
        cells
            .into_iter()
            .map(|cell| cell.map(|id| (id, 0)))
            .collect()
    }

    const P: Option<&str> = Some("minecraft:oak_planks");
    const S: Option<&str> = Some("minecraft:stick");

    #[test]
    fn shaped_recipes_match_anywhere_and_mirrored() {
        let axe = axe();
        assert!(axe.matches(&grid([P, P, None, P, S, None, None, S, None])));
        assert!(axe.matches(&grid([None, P, P, None, S, P, None, S, None])));
        // Mirrored: planks on the right of the stick.
        assert!(axe.matches(&grid([P, P, None, S, P, None, S, None, None])));
        // An extra item outside the shape.
        assert!(!axe.matches(&grid([P, P, P, P, S, None, None, S, None])));
        assert!(!axe.matches(&grid([P, P, None, P, S, None, None, None, None])));
    }

    #[test]
    fn shapeless_recipes_take_exactly_their_inputs_in_any_cells() {
        let dye = Recipe {
            shape: None,
            inputs: vec![vec![item("minecraft:poppy")]],
            output: crafted("minecraft:red_dye", 1),
        };
        let poppy = Some("minecraft:poppy");
        assert!(dye.matches(&grid([
            None, None, None, None, None, None, None, None, poppy
        ])));
        assert!(!dye.matches(&grid([
            poppy, poppy, None, None, None, None, None, None, None
        ])));
        assert!(!dye.matches(&grid([None; 9])));
    }

    #[test]
    fn metadata_matches_exactly_or_any() {
        let exact = Recipe {
            shape: None,
            inputs: vec![vec![ItemMatch {
                id: "minecraft:wool".to_owned(),
                metadata: Some(14),
            }]],
            output: crafted("minecraft:string", 4),
        };
        let mut cells = vec![None; 9];
        cells[0] = Some(("minecraft:wool", 14));
        assert!(exact.matches(&cells));
        cells[0] = Some(("minecraft:wool", 1));
        assert!(!exact.matches(&cells));
        assert!(!exact.matches(&cells[..8]));
    }
}
