//! Banner pattern identities shared by world and UI presentation.

/// One pattern's wire code and artwork route; its table index is the native UI ID.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BannerPattern {
    pub code: &'static str,
    pub texture: &'static str,
    pub path: &'static str,
}

macro_rules! pattern {
    ($code:literal, $texture:literal) => {
        BannerPattern {
            code: $code,
            texture: $texture,
            path: concat!("textures/entity/banner/banner_", $texture),
        }
    };
}

/// Pattern order used by native banner UI bindings, including the base and illager mark.
pub const BANNER_PATTERNS: &[BannerPattern] = &[
    pattern!("", "base"),
    pattern!("bo", "border"),
    pattern!("bri", "bricks"),
    pattern!("mc", "circle"),
    pattern!("cre", "creeper"),
    pattern!("sku", "skull"),
    pattern!("cr", "cross"),
    pattern!("cbo", "curly_border"),
    pattern!("ld", "diagonal_up_left"),
    pattern!("rud", "diagonal_up_right"),
    pattern!("lud", "diagonal_left"),
    pattern!("rd", "diagonal_right"),
    pattern!("flo", "flower"),
    pattern!("gra", "gradient"),
    pattern!("gru", "gradient_up"),
    pattern!("hh", "half_horizontal"),
    pattern!("hhb", "half_horizontal_bottom"),
    pattern!("vh", "half_vertical"),
    pattern!("vhr", "half_vertical_right"),
    pattern!("ill", "illager"),
    pattern!("moj", "mojang"),
    pattern!("mr", "rhombus"),
    pattern!("ss", "small_stripes"),
    pattern!("bl", "square_bottom_left"),
    pattern!("br", "square_bottom_right"),
    pattern!("tl", "square_top_left"),
    pattern!("tr", "square_top_right"),
    pattern!("sc", "straight_cross"),
    pattern!("bs", "stripe_bottom"),
    pattern!("cs", "stripe_center"),
    pattern!("dls", "stripe_downleft"),
    pattern!("drs", "stripe_downright"),
    pattern!("ls", "stripe_left"),
    pattern!("ms", "stripe_middle"),
    pattern!("rs", "stripe_right"),
    pattern!("ts", "stripe_top"),
    pattern!("bt", "triangle_bottom"),
    pattern!("tt", "triangle_top"),
    pattern!("bts", "triangles_bottom"),
    pattern!("tts", "triangles_top"),
    pattern!("pig", "piglin"),
    pattern!("glb", "globe"),
    pattern!("flw", "flow"),
    pattern!("gus", "guster"),
];

/// Resolves a native numeric UI pattern ID.
pub fn pattern_by_ui_id(id: usize) -> Option<&'static BannerPattern> {
    BANNER_PATTERNS.get(id)
}

/// Resolves a wire pattern code to the shared pattern artwork.
pub fn pattern_by_code(code: &str) -> Option<&'static BannerPattern> {
    BANNER_PATTERNS.iter().find(|pattern| pattern.code == code)
}

/// Returns the native numeric UI ID for a wire pattern code.
pub fn pattern_ui_id(code: &str) -> Option<usize> {
    BANNER_PATTERNS
        .iter()
        .position(|pattern| pattern.code == code)
}

/// Returns the texture stem for a banner's wire pattern code.
pub fn pattern_texture(code: &str) -> Option<&'static str> {
    Some(pattern_by_code(code)?.texture)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pattern_artwork_routes_exist_in_the_pinned_texture_inventory() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(crate::vanilla_source().cache_dir.as_ref())
            .join("resource_pack/textures/entity/banner");
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                eprintln!(
                    "missing fixture: pinned banner artwork {}",
                    directory.display()
                );
                return;
            }
            Err(error) => panic!("read pinned banner artwork: {error}"),
        };
        // The separate banner_pattern_illager artwork is not an additional pattern layer.
        let inventory: std::collections::BTreeSet<_> = entries
            .map(Result::unwrap)
            .filter_map(|entry| {
                let path = entry.path();
                (matches!(path.extension()?.to_str()?, "png" | "tga")
                    && path.file_stem()?.to_str()?.starts_with("banner_")
                    && !path.file_stem()?.to_str()?.ends_with("_mers")
                    && path.file_stem()?.to_str()? != "banner_pattern_illager")
                    .then(|| path.file_stem().unwrap().to_string_lossy().into_owned())
            })
            .collect();
        let routed: std::collections::BTreeSet<_> = BANNER_PATTERNS
            .iter()
            .map(|pattern| format!("banner_{}", pattern.texture))
            .collect();
        assert!(!inventory.is_empty());
        assert_eq!(
            routed, inventory,
            "every pinned pattern artwork needs a route"
        );
    }
}
