use super::terrain::{TerrainPaths, TerrainTextureMap};

impl TerrainTextureMap {
    /// Literal atlas tint, without the distinct alpha-mask overlay operation.
    /// Unknown extensions and malformed colours remain unresolved.
    pub(crate) fn fixed_tint_source(
        &self,
        key: &str,
        variant: u32,
    ) -> Option<(&str, Option<[u8; 3]>)> {
        // A positional selector is not one immutable atlas image. The lily
        // route must not copy its first image while retaining untinted peers.
        if self
            .position_variations
            .contains_key(&(key.into(), variant))
        {
            return None;
        }
        let (path, tint) = match self.entries.get(key)? {
            TerrainPaths::Static {
                path,
                overlay_color: None,
                tint_color,
                has_extra_metadata: false,
                ..
            } if variant == 0 => (path.as_ref(), tint_color.as_ref()),
            TerrainPaths::Variants {
                paths,
                overlay_colors,
                tint_colors,
                has_extra_metadata: false,
                ..
            } => {
                let index = variant as usize;
                if overlay_colors.get(index)?.is_some() {
                    return None;
                }
                (paths.get(index)?.as_ref(), tint_colors.get(index)?.as_ref())
            }
            _ => return None,
        };
        let colour = match tint {
            Some(tint) => Some(literal_rgb(tint.as_str()?)?),
            None => None,
        };
        Some((path, colour))
    }
}

fn literal_rgb(source: &str) -> Option<[u8; 3]> {
    // TextureJSONParser delegates to colour parsing:
    // hexadecimal strings select their low RGB bytes, with alpha one.
    let hex = source.strip_prefix('#')?;
    if !matches!(hex.len(), 6 | 8) || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let rgb = u32::from_str_radix(hex, 16).ok()?;
    Some([(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8])
}
