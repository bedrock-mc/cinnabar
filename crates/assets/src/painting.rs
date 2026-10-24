//! Painting identities, dimensions and texture selections shared by meshes and interaction.

/// One admitted motive. Dimensions and atlas coordinates are logical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaintingMotive {
    pub title: &'static str,
    pub size: [u8; 2],
    pub atlas_origin: Option<[u8; 2]>,
}

impl PaintingMotive {
    /// The runtime texture selected for the front of this motive.
    pub fn texture(self) -> String {
        match self.atlas_origin {
            Some(_) => PAINTING_ATLAS.into(),
            None => format!("textures/painting/{}", self.title),
        }
    }

    /// Catalog identity of one independently textured painting part.
    pub fn part_identifier(self, border: bool) -> String {
        format!(
            "cinnabar:painting/{}/{}",
            self.title,
            if border { "border" } else { "front" }
        )
    }
}

pub const PAINTING_ATLAS: &str = "textures/painting/kz";
pub const PAINTING_ATLAS_SIDE: u16 = 256;
pub const PAINTING_BORDER_ORIGIN: [f32; 2] = [192.0, 0.0];
pub const PAINTING_BORDER_SAMPLE_PIXELS: f32 = 0.5;
pub const PAINTING_HALF_DEPTH: f32 = 1.0 / 32.0;
pub const PAINTING_HITBOX_INSET: f32 = 1.0 / 32.0;

macro_rules! motives {
    ($($title:literal, $width:literal, $height:literal, $uv:expr;)*) => {
        pub const PAINTING_MOTIVES: &[PaintingMotive] = &[$(PaintingMotive {
            title: $title, size: [$width, $height], atlas_origin: $uv,
        }),*];
    };
}

motives! {
    "Kebab", 16, 16, Some([0, 0]);
    "Aztec", 16, 16, Some([16, 0]);
    "Alban", 16, 16, Some([32, 0]);
    "Aztec2", 16, 16, Some([48, 0]);
    "Bomb", 16, 16, Some([64, 0]);
    "Plant", 16, 16, Some([80, 0]);
    "Wasteland", 16, 16, Some([96, 0]);
    "Pool", 32, 16, Some([0, 32]);
    "Courbet", 32, 16, Some([32, 32]);
    "Sea", 32, 16, Some([64, 32]);
    "Sunset", 32, 16, Some([96, 32]);
    "Creebet", 32, 16, Some([128, 32]);
    "Wanderer", 16, 32, Some([0, 64]);
    "Graham", 16, 32, Some([16, 64]);
    "Match", 32, 32, Some([0, 128]);
    "Bust", 32, 32, Some([32, 128]);
    "Stage", 32, 32, Some([64, 128]);
    "Void", 32, 32, Some([96, 128]);
    "SkullAndRoses", 32, 32, Some([128, 128]);
    "Wither", 32, 32, Some([160, 128]);
    "Fighters", 64, 32, Some([0, 96]);
    "Pointer", 64, 64, Some([0, 192]);
    "Pigscene", 64, 64, Some([64, 192]);
    "BurningSkull", 64, 64, Some([128, 192]);
    "Skeleton", 64, 48, Some([192, 64]);
    "DonkeyKong", 64, 48, Some([192, 112]);
    "Earth", 32, 32, Some([0, 160]);
    "Wind", 32, 32, Some([32, 160]);
    "Fire", 32, 32, Some([64, 160]);
    "Water", 32, 32, Some([96, 160]);
    "baroque", 32, 32, None;
    "humble", 32, 32, None;
    "meditative", 16, 16, None;
    "prairie_ride", 16, 32, None;
    "unpacked", 64, 64, None;
    "backyard", 48, 64, None;
    "bouquet", 48, 48, None;
    "cavebird", 48, 48, None;
    "changing", 64, 32, None;
    "cotan", 48, 48, None;
    "endboss", 48, 48, None;
    "fern", 48, 48, None;
    "finding", 64, 32, None;
    "lowmist", 64, 32, None;
    "orb", 64, 64, None;
    "owlemons", 48, 48, None;
    "passage", 64, 32, None;
    "pond", 48, 64, None;
    "sunflowers", 48, 48, None;
    "tides", 48, 48, None;
    "dennis", 48, 48, None;
}

/// Looks up the exact case-sensitive wire title, falling back to the first motive.
pub fn motive(title: &str) -> PaintingMotive {
    PAINTING_MOTIVES
        .iter()
        .copied()
        .find(|motive| motive.title == title)
        .unwrap_or(PAINTING_MOTIVES[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gallery_painting_sizes_match_the_runtime_catalog() {
        let gallery: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/localserver/painting_motives.json"
        ))
        .unwrap();
        let expected: Vec<_> = PAINTING_MOTIVES
            .iter()
            .map(|motive| serde_json::json!({"title": motive.title, "size": motive.size}))
            .collect();
        assert_eq!(
            gallery,
            serde_json::Value::Array(expected),
            "regenerate with cargo run -p assets --example export_painting_motives"
        );
    }

    #[test]
    fn resource_pack_revision_requires_a_painting_catalog_audit() {
        use sha2::{Digest, Sha256};
        let audited_manifest = [
            0xa3, 0xe0, 0xb4, 0xad, 0xac, 0x67, 0xc8, 0x53, 0x68, 0x18, 0xb1, 0x1b, 0xcc, 0xa6,
            0xf3, 0xa8, 0x24, 0xcf, 0xd3, 0x84, 0xd7, 0x41, 0x51, 0x9b, 0x62, 0xa7, 0xb0, 0x33,
            0xef, 0xe3, 0xa3, 0x85,
        ];
        let manifest: serde_json::Value =
            serde_json::from_str(crate::VANILLA_SOURCE_MANIFEST).unwrap();
        let identity =
            serde_json::to_vec(&[&manifest["tag"], &manifest["commit"], &manifest["sha256"]])
                .unwrap();
        assert_eq!(
            <[u8; 32]>::from(Sha256::digest(&identity)),
            audited_manifest,
            "update painting identities, dimensions and atlas regions when the resource source changes"
        );
    }

    #[test]
    fn motive_regions_fit_their_declared_atlas() {
        let mut titles = std::collections::BTreeSet::new();
        for motive in PAINTING_MOTIVES {
            assert!(titles.insert(motive.title));
            assert!(motive.size.iter().all(|side| *side > 0 && side % 16 == 0));
            if let Some(origin) = motive.atlas_origin {
                for (origin, side) in origin.into_iter().zip(motive.size) {
                    assert!(u16::from(origin) + u16::from(side) <= PAINTING_ATLAS_SIDE);
                }
            }
        }
    }

    #[test]
    fn pinned_pack_individual_motives_match_the_runtime_catalog() {
        let Some(root) = std::env::var_os("CINNABAR_VANILLA_RESOURCE_PACK") else {
            eprintln!(
                "skipping painting motive inventory: CINNABAR_VANILLA_RESOURCE_PACK is not configured"
            );
            return;
        };
        let paths =
            std::fs::read_dir(std::path::Path::new(&root).join("textures/painting")).unwrap();
        let actual: std::collections::BTreeSet<_> = paths
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "png"))
            .filter_map(|path| path.file_stem().unwrap().to_str().map(str::to_owned))
            .filter(|name| name != "kz")
            .collect();
        let expected: std::collections::BTreeSet<_> = PAINTING_MOTIVES
            .iter()
            .filter(|motive| motive.atlas_origin.is_none())
            .map(|motive| motive.title.to_owned())
            .collect();
        assert_eq!(
            actual, expected,
            "painting catalog must track the pinned runtime pack"
        );
    }
}
