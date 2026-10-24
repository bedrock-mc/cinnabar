use super::*;

/// A whole-model material decision; child indices advance in the shared node table.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", deny_unknown_fields)]
pub enum EntityRenderMaterialSelection {
    Raster {
        material: EntityRenderMaterial,
        state: Option<EntityRenderMaterialState>,
    },
    Branch {
        condition: u32,
        yes: u32,
        no: u32,
    },
    Array {
        index: u32,
        choices: Box<[u32]>,
    },
}

impl EntityRenderMaterialSelection {
    /// The expression evaluated by this decision; raster leaves read no Molang state.
    pub fn expression(&self) -> Option<u32> {
        match self {
            Self::Raster { .. } => None,
            Self::Branch { condition, .. } => Some(*condition),
            Self::Array { index, .. } => Some(*index),
        }
    }

    /// Child decisions in authored order.
    fn children(&self) -> impl Iterator<Item = u32> + '_ {
        let pair = match self {
            Self::Branch { yes, no, .. } => [Some(*yes), Some(*no)],
            Self::Raster { .. } | Self::Array { .. } => [None; 2],
        };
        let choices = match self {
            Self::Array { choices, .. } => choices.as_ref(),
            Self::Raster { .. } | Self::Branch { .. } => &[],
        };
        pair.into_iter().flatten().chain(choices.iter().copied())
    }
}

impl EntityRenderData {
    /// Walks reachable decisions once, tolerating unvalidated indices without looping.
    pub fn material_nodes(
        &self,
        layer: &EntityRenderLayer,
    ) -> impl Iterator<Item = &EntityRenderMaterialSelection> {
        let mut pending: Vec<_> = layer.material_selection.into_iter().collect();
        let mut visited = std::collections::BTreeSet::new();
        std::iter::from_fn(move || {
            while let Some(index) = pending.pop() {
                if !visited.insert(index) {
                    continue;
                }
                let Some(node) = self.material_selections.get(index as usize) else {
                    continue;
                };
                pending.extend(node.children());
                return Some(node);
            }
            None
        })
    }

    /// Every possible raster state, for artwork preservation and conservative capabilities.
    pub fn materials(
        &self,
        layer: &EntityRenderLayer,
    ) -> impl Iterator<Item = (EntityRenderMaterial, Option<EntityRenderMaterialState>)> {
        layer
            .material_selection
            .is_none()
            .then_some((layer.material, layer.material_state))
            .into_iter()
            .chain(self.material_nodes(layer).filter_map(|node| match node {
                EntityRenderMaterialSelection::Raster { material, state } => {
                    Some((*material, *state))
                }
                _ => None,
            }))
    }
}

/// Checks expression references, nonempty arrays and forward-only links before runtime traversal.
pub(super) fn validate(render: &EntityRenderData, expressions: usize) -> Result<(), AssetError> {
    if render.material_selections.len() > MAX_ENTITY_RENDER_CANDIDATES {
        return Err(invalid("entity material selection count exceeds bound"));
    }
    for (index, node) in render.material_selections.iter().enumerate() {
        if node
            .expression()
            .is_some_and(|expression| expression as usize >= expressions)
            || matches!(node, EntityRenderMaterialSelection::Array { choices, .. } if choices.is_empty())
            || node.children().any(|child| {
                child as usize <= index || child as usize >= render.material_selections.len()
            })
        {
            return Err(invalid("entity material selection is invalid"));
        }
    }
    Ok(())
}
