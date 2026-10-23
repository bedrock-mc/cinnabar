//! Fail-closed validation of pack rule identities against the pinned inputs.

use super::*;

impl PackRules {
    /// Rejects incomplete slots, stale vocabulary names and ambiguous selector products.
    pub fn validate(&self) -> Result<(), String> {
        let vocabulary = vocabulary();
        let mut blocks = BTreeSet::new();
        for bytes in [
            crate::pinned_block_registry_bytes(),
            include_bytes!("../../../data/block-registry-v1001.bin").as_slice(),
        ] {
            let protocol = crate::registry_header_protocol(bytes).map_err(|e| e.to_string())?;
            let records =
                crate::read_registry_for_protocol(bytes, protocol).map_err(|e| e.to_string())?;
            blocks.extend(records.iter().map(|record| record.name.to_string()));
        }
        let retail: BTreeSet<_> = std::str::from_utf8(crate::item::retail::SOURCE_BYTES)
            .map_err(|e| e.to_string())?
            .lines()
            .filter_map(|line| line.split_once('\t').map(|(_, name)| name))
            .collect();
        let require = |domain: &str, name: &str| -> Result<(), String> {
            if vocabulary
                .get(domain)
                .is_some_and(|names| names.contains(name))
            {
                Ok(())
            } else {
                Err(format!("unknown pack {domain} rule {name}"))
            }
        };
        let block = |name: &str| -> Result<(), String> {
            if blocks.contains(name) {
                Ok(())
            } else {
                Err(format!("unknown pack block rule {name}"))
            }
        };
        if self.textures.len() != PackTexture::ALL.len()
            || PackTexture::ALL
                .iter()
                .any(|slot| !self.textures.contains_key(slot))
            || self.blocks.len() != PackBlock::ALL.len()
            || PackBlock::ALL
                .iter()
                .any(|slot| !self.blocks.contains_key(slot))
        {
            return Err("incomplete typed pack slots".into());
        }
        let mut keys = BTreeSet::new();
        for key in self.textures.values() {
            require("terrain_texture", key)?;
            if !keys.insert(key) {
                return Err("duplicate typed pack texture".into());
            }
        }
        let mut names = BTreeSet::new();
        for name in self.blocks.values() {
            block(name)?;
            if !name.starts_with("minecraft:") || !names.insert(name) {
                return Err("invalid typed pack block identity".into());
            }
        }
        let alpha: BTreeSet<_> = self.alpha_textures.iter().collect();
        if alpha.len() != self.alpha_textures.len() {
            return Err("duplicate pack alpha texture".into());
        }
        for key in self.selectors.keys() {
            require("terrain_texture", key)?;
        }
        for selector in TextureSelector::ALL {
            if !self.selectors.values().any(|value| value == selector) {
                return Err("empty pack texture selector".into());
            }
        }
        for (selector, members) in &self.variants {
            let width = match selector {
                TextureSelector::Door => 7,
                TextureSelector::Flower => 11,
                TextureSelector::Sapling => 6,
                _ => return Err("unsupported ordered pack selector".into()),
            };
            if members.len() != width || members.iter().collect::<BTreeSet<_>>().len() != width {
                return Err("invalid ordered pack selector product".into());
            }
            for name in members {
                block(name)?;
            }
        }
        if self.variants.len() != 3 {
            return Err("incomplete ordered pack selectors".into());
        }
        if self.powered_prefix.is_empty() || !blocks.iter().any(|name| self.is_powered(name)) {
            return Err("empty powered pack selector".into());
        }
        for (prefix, name) in &self.mushrooms {
            block(name)?;
            if prefix.is_empty()
                || !vocabulary["terrain_texture"]
                    .iter()
                    .any(|key| key.starts_with(prefix))
            {
                return Err("empty mushroom pack selector".into());
            }
        }
        if self.mushrooms.is_empty() {
            return Err("missing mushroom pack selectors".into());
        }
        for (name, kind) in &self.materials {
            if *kind == MaterialKind::Provisional {
                return Err(format!(
                    "provisional material {name} is derived, not listed"
                ));
            }
            if *kind == MaterialKind::UnmaskedArmor {
                let facts = super::super::ItemFacts::builtin().map_err(|e| e.to_string())?;
                let identity = format!("minecraft:{name}");
                if !facts
                    .by_identifier(&identity)
                    .is_some_and(|fact| fact.traits.contains(super::super::ItemTraits::GLIDER))
                {
                    return Err("unknown glider material rule".into());
                }
            } else if !vocabulary["material"].contains(name.as_str()) {
                require("material", &format!("{name}.skinning"))?;
            }
        }
        for (name, template) in &self.omitted_render_controllers {
            if name == template || !template.starts_with("controller.render.") {
                return Err(format!("invalid omitted render controller rule {name}"));
            }
        }
        for (actor, egg) in &self.egg_aliases {
            require("actor", actor)?;
            if !retail.contains(egg.as_str()) {
                return Err(format!("unknown retail egg rule {egg}"));
            }
        }
        for (old, new) in &self.superseded_eggs {
            require("actor", old)?;
            require("actor", new)?;
            if self.egg_identifier(old) != self.egg_identifier(new) {
                return Err("superseded eggs disagree on retail identity".into());
            }
        }
        let mut minerals = BTreeSet::new();
        for mineral in &self.minerals {
            block(&mineral.identifier)?;
            require("terrain_texture", &mineral.key)?;
            if !minerals.insert(&mineral.identifier)
                || !mineral.path.starts_with("textures/blocks/")
            {
                return Err("invalid mineral pack contract".into());
            }
        }
        let mut alias_keys = BTreeSet::new();
        for (name, source) in &self.alias_cubes {
            block(name)?;
            let keys = match source {
                AliasCubeSource::Scalar(key) => std::slice::from_ref(key),
                AliasCubeSource::Pillar(keys) => keys.as_slice(),
            };
            for key in keys {
                require("terrain_texture", key)?;
                alias_keys.insert(key);
            }
        }
        if alias_keys != self.alias_paths.keys().collect() {
            return Err("alias cube paths differ from source keys".into());
        }
        for path in self.alias_paths.values() {
            if !path.starts_with("textures/blocks/") {
                return Err("invalid alias texture path".into());
            }
        }
        for name in self.item_visuals.keys() {
            if !retail.contains(name.as_str()) {
                return Err(format!("unknown retail item visual rule {name}"));
            }
        }
        if self.empty_hand != PackBlock::Air
            || self
                .item_visuals
                .values()
                .any(|kind| *kind == ItemVisualKind::EmptyHand)
        {
            return Err("pack rules require canonical air for the empty hand".into());
        }
        Ok(())
    }
}
