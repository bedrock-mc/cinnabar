//! Resolves sampler declarations in transformer and texture-binding order.

use crate::buffers::{canonical_sampler_name, unit_zero_texture};
use crate::reflection::{SamplerClass, TextureDimension};
use std::collections::HashMap;

/// Custom sampler bindings, separated by how their declarations take effect.
#[derive(Default)]
pub(crate) struct SamplerClasses {
    /// Exact custom-image sampler name → reflected image binding.
    pub images: HashMap<String, String>,
    /// Raw texture's exact declared name and shape → reflected name and class.
    pub named: HashMap<(String, TextureDimension), (String, SamplerClass)>,
    /// Canonical texture-unit name and shape → sampler class, excluding released overrides.
    pub units: HashMap<(String, TextureDimension), SamplerClass>,
}

impl SamplerClasses {
    /// Chooses a resource once, before reflection and the renderer consume its binding key.
    /// Images and raw textures keep distinct keys so a fallback cannot inherit their names.
    pub fn resolve(
        &self,
        name: &str,
        dimension: TextureDimension,
        unit_zero: bool,
        geometry: bool,
        normalize_legacy: bool,
    ) -> (String, SamplerClass) {
        // Normalize legacy sampler names before matching exact declarations.
        let declared = super::legacy::sampler_name(name);
        let declared = if normalize_legacy && matches!(declared, "texture" | "gcolor") {
            "gtexture"
        } else {
            declared
        };
        if let Some(binding) = self.images.get(declared) {
            return (binding.clone(), SamplerClass::ClampLinear);
        }
        if let Some(binding) = self.named.get(&(declared.to_owned(), dimension)) {
            return binding.clone();
        }
        let canonical = canonical_sampler_name(declared);
        if let Some(class) = self.units.get(&(canonical.clone(), dimension)) {
            return (canonical, *class);
        }
        // Recognized buffers and atlas names retain their built-in binding. Other scalar
        // float samplers read unit zero, including any plain override of that unit.
        let bound = if unit_zero {
            unit_zero_texture(&canonical, geometry).unwrap_or(&canonical)
        } else {
            &canonical
        };
        let class = self
            .units
            .get(&(bound.to_owned(), dimension))
            .copied()
            .unwrap_or_else(|| SamplerClass::for_name(bound));
        (bound.to_owned(), class)
    }
}
