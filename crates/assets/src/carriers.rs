//! The carrier table: every compiled carrier with its `assetc` recipe, inputs and outputs. `assetc
//! prepare`, first-run setup, `dist-local` and the startup filename constants all read it.

use std::path::{Path, PathBuf};

/// Output directory below a checkout.
pub const COMPILED_DIR: &str = ".local/assets/compiled";
/// Per-carrier fingerprints, written beside the carriers they describe.
pub const STAMP_FILE: &str = "prepared.json";

pub const VANILLA_MANIFEST: &str = "assets/vanilla-source.json";
pub const FONT_MANIFEST: &str = "assets/cinnangles-sans-source.json";
pub const FONT_TEN_MANIFEST: &str = "assets/cinnangles-ten-source.json";
pub const FONT_SEVEN_MANIFEST: &str = "assets/cinnangles-seven-source.json";

mod fonts;
pub use fonts::FontFace;
const HUD_MANIFEST: &str = "assets/hud-source-v2193.json";
const BLOCK_REGISTRY: &str = "crates/assets/data/block-registry-v2193.bin";
const LIGHT_REGISTRY: &str = "crates/assets/data/block-light-registry-v2193.bin";
const BIOME_REGISTRY: &str = "crates/assets/data/biome-registry-v2193.bin";

/// The compiler entry point that builds a carrier.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Recipe {
    World,
    Atmosphere,
    Entity,
    Font,
    Hud,
    Lang,
    Languages,
    Icon,
    Audio,
    Actor,
    AudioBank,
    Equipment,
    Ui,
    Particle,
    BlockEntity,
    Weather,
    HudExtras,
    StarterSkins,
    OreUiPanoramas,
    AudioPcm,
}

/// What a recipe reads besides the compiler itself; tracked paths are checkout-relative.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Input {
    /// The pinned sample `resource_pack`, identified by [`VANILLA_MANIFEST`].
    Pack,
    /// The pinned sample `behavior_pack`, passed only when it ships item definitions.
    BehaviorPack,
    /// The source manifest the compiler embeds.
    Manifest(&'static str),
    File(&'static str),
    /// The font file the given font manifest names, below `assets/fonts/`.
    FontFile(&'static str),
}

#[derive(Clone, Copy, Debug)]
pub struct Carrier {
    /// Stable key for `--only`, the stamp and `make <name>-assets`.
    pub name: &'static str,
    pub label: &'static str,
    pub recipe: Recipe,
    /// Optional semantic outline face and its pinned source metrics.
    pub font_face: Option<FontFace>,
    /// File, or directory, below the output directory.
    pub output: &'static str,
    pub report: Option<&'static str>,
    /// Packaged startup needs it, so a failed build aborts preparation.
    pub required: bool,
    /// Built for packaged installs; development-only carriers are not.
    pub installed: bool,
    pub inputs: &'static [Input],
    /// Carriers whose outputs this recipe reads.
    pub reads: &'static [&'static str],
}

const PACK: &[Input] = &[Input::Pack, Input::Manifest(VANILLA_MANIFEST)];

const fn carrier(
    name: &'static str,
    label: &'static str,
    recipe: Recipe,
    output: &'static str,
    report: Option<&'static str>,
    required: bool,
) -> Carrier {
    Carrier {
        name,
        label,
        recipe,
        font_face: None,
        output,
        report,
        required,
        installed: true,
        inputs: PACK,
        reads: &[],
    }
}

pub const WORLD: Carrier = Carrier {
    inputs: &[
        Input::Pack,
        Input::Manifest(VANILLA_MANIFEST),
        Input::File(BLOCK_REGISTRY),
        Input::File(LIGHT_REGISTRY),
        Input::File(BIOME_REGISTRY),
    ],
    ..carrier(
        "world",
        "Compiling world assets",
        Recipe::World,
        "vanilla-v2193.mcbea",
        None,
        true,
    )
};
pub const ATMOSPHERE: Carrier = carrier(
    "atmosphere",
    "Compiling sky assets",
    Recipe::Atmosphere,
    "vanilla-v1.mcbeatm",
    Some("atmosphere-assets.json"),
    true,
);
pub const ENTITY: Carrier = carrier(
    "entity",
    "Compiling entity assets",
    Recipe::Entity,
    "vanilla-v1.mcbeent",
    Some("entity-assets.json"),
    true,
);
pub const FONT: Carrier = Carrier {
    inputs: &[
        Input::Manifest(FONT_MANIFEST),
        Input::FontFile(FONT_MANIFEST),
    ],
    ..carrier(
        "font",
        "Compiling Cinnangles Sans",
        Recipe::Font,
        "ui-cinnangles-sans-v1.mcbefont",
        Some("ui-cinnangles-sans-font-assets.json"),
        false,
    )
};
pub const FONT_TEN: Carrier = Carrier {
    font_face: Some(FontFace {
        name: "Cinnangles Ten",
        manifest: include_bytes!("../../../assets/cinnangles-ten-source.json"),
        units_per_em: 1280,
        ascent: 1267,
        descent: 320,
    }),
    inputs: &[
        Input::Manifest(FONT_TEN_MANIFEST),
        Input::FontFile(FONT_TEN_MANIFEST),
    ],
    ..carrier(
        "font-ten",
        "Compiling Cinnangles Ten",
        Recipe::Font,
        "ui-cinnangles-ten-v1.mcbefont",
        Some("ui-cinnangles-ten-font-assets.json"),
        false,
    )
};
pub const FONT_SEVEN: Carrier = Carrier {
    font_face: Some(FontFace {
        name: "Cinnangles Seven",
        manifest: include_bytes!("../../../assets/cinnangles-seven-source.json"),
        units_per_em: 1280,
        ascent: 1024,
        descent: 128,
    }),
    inputs: &[
        Input::Manifest(FONT_SEVEN_MANIFEST),
        Input::FontFile(FONT_SEVEN_MANIFEST),
    ],
    ..carrier(
        "font-seven",
        "Compiling Cinnangles Seven",
        Recipe::Font,
        "ui-cinnangles-seven-v1.mcbefont",
        Some("ui-cinnangles-seven-font-assets.json"),
        false,
    )
};
pub const HUD: Carrier = Carrier {
    inputs: &[Input::Pack, Input::Manifest(HUD_MANIFEST)],
    ..carrier(
        "hud",
        "Compiling HUD sprites",
        Recipe::Hud,
        "vanilla-v1.mcbehud",
        Some("hud-assets.json"),
        true,
    )
};
pub const LANG: Carrier = carrier(
    "lang",
    "Compiling language files",
    Recipe::Lang,
    "vanilla-v1.mcbelang",
    Some("lang-assets.json"),
    true,
);
pub const LANGUAGES: Carrier = carrier(
    "language",
    "Compiling other languages",
    Recipe::Languages,
    "lang",
    None,
    false,
);
pub const ICON: Carrier = Carrier {
    reads: &[WORLD.name],
    ..carrier(
        "icon",
        "Compiling item icons",
        Recipe::Icon,
        "vanilla-v1.mcbeico",
        Some("icon-assets.json"),
        true,
    )
};
pub const AUDIO: Carrier = carrier(
    "audio",
    "Compiling audio catalog",
    Recipe::Audio,
    "vanilla-v1.mcbeaud",
    Some("audio-assets.json"),
    false,
);
pub const ACTOR: Carrier = carrier(
    "actor",
    "Compiling actor assets",
    Recipe::Actor,
    "vanilla-v1.mcbeact",
    Some("actor-assets.json"),
    false,
);
pub const AUDIO_BANK: Carrier = Carrier {
    inputs: &[Input::Pack],
    ..carrier(
        "audio-bank",
        "Compiling sound bank",
        Recipe::AudioBank,
        "vanilla-v1.mcbesnd",
        Some("audio-bank.json"),
        false,
    )
};
pub const EQUIPMENT: Carrier = Carrier {
    inputs: &[
        Input::Pack,
        Input::Manifest(VANILLA_MANIFEST),
        Input::BehaviorPack,
    ],
    ..carrier(
        "equipment",
        "Compiling equipment assets",
        Recipe::Equipment,
        "vanilla-v1.mcbeeqp",
        Some("equipment-assets.json"),
        false,
    )
};
pub const UI: Carrier = carrier(
    "ui",
    "Compiling UI textures",
    Recipe::Ui,
    "vanilla-v1.mcbeui",
    Some("ui-assets.json"),
    true,
);
pub const PARTICLE: Carrier = carrier(
    "particle",
    "Compiling particles",
    Recipe::Particle,
    "vanilla-v1.mcbept",
    Some("particle-assets.json"),
    false,
);
pub const BLOCK_ENTITY: Carrier = carrier(
    "block-entity",
    "Compiling block entities",
    Recipe::BlockEntity,
    "vanilla-v1.mcbeben",
    Some("block-entity-assets.json"),
    false,
);
pub const WEATHER: Carrier = Carrier {
    inputs: &[Input::Pack],
    ..carrier(
        "weather",
        "Compiling weather textures",
        Recipe::Weather,
        "vanilla-v1.mcbewth",
        None,
        false,
    )
};
pub const HUD_EXTRAS: Carrier = Carrier {
    inputs: &[Input::Pack],
    ..carrier(
        "hud-extras",
        "Compiling HUD extras",
        Recipe::HudExtras,
        "vanilla-v1.mcbehxt",
        None,
        false,
    )
};
pub const STARTER_SKINS: Carrier = Carrier {
    inputs: &[Input::Pack],
    ..carrier(
        "skins",
        "Compiling starter skins",
        Recipe::StarterSkins,
        "vanilla-v1.mcbeskn",
        None,
        false,
    )
};
/// Optional landscape crops prepared from the fetched pack for OreUI screens.
pub const OREUI_PANORAMAS: Carrier = carrier(
    "oreui-panoramas",
    "Compiling OreUI panorama banners",
    Recipe::OreUiPanoramas,
    "vanilla-v1.mcbeopa",
    None,
    false,
);
/// Development-only finite predecode of one reviewed sample.
pub const AUDIO_PCM: Carrier = Carrier {
    installed: false,
    reads: &[AUDIO.name],
    ..carrier(
        "audio-pcm",
        "Compiling reviewed PCM sample",
        Recipe::AudioPcm,
        "vanilla-v1.mcbepcm",
        Some("audio-pcm-assets.json"),
        false,
    )
};

/// Every carrier, in the order progress reports them.
pub const CARRIERS: &[Carrier] = &[
    WORLD,
    ATMOSPHERE,
    ENTITY,
    FONT,
    FONT_TEN,
    FONT_SEVEN,
    HUD,
    LANG,
    LANGUAGES,
    ICON,
    AUDIO,
    ACTOR,
    AUDIO_BANK,
    EQUIPMENT,
    UI,
    PARTICLE,
    BLOCK_ENTITY,
    WEATHER,
    HUD_EXTRAS,
    STARTER_SKINS,
    OREUI_PANORAMAS,
    AUDIO_PCM,
];

impl Carrier {
    /// The output and report paths below `dir`.
    pub fn outputs(&self, dir: &Path) -> impl Iterator<Item = PathBuf> {
        let dir = dir.to_path_buf();
        std::iter::once(self.output)
            .chain(self.report)
            .map(move |name| dir.join(name))
    }
}

#[must_use]
pub fn by_name(name: &str) -> Option<&'static Carrier> {
    CARRIERS.iter().find(|carrier| carrier.name == name)
}

/// Every carrier packaged startup needs.
pub fn required() -> impl Iterator<Item = &'static Carrier> {
    CARRIERS.iter().filter(|carrier| carrier.required)
}

/// Whether `dir` holds every required carrier.
#[must_use]
pub fn required_present(dir: &Path) -> bool {
    required().all(|carrier| dir.join(carrier.output).is_file())
}

/// Preparation-kit directories and the checkout prefixes they stand in for.
const KIT_LAYOUT: [(&str, &str); 2] = [("assets", "assets/"), ("data", "crates/assets/data/")];

/// Where tracked inputs resolve: a checkout, or an installer's preparation kit.
#[derive(Clone, Debug)]
pub enum Sources {
    Checkout(PathBuf),
    Kit(PathBuf),
}

impl Sources {
    /// The file a checkout-relative path names here.
    #[must_use]
    pub fn resolve(&self, relative: &str) -> PathBuf {
        match self {
            Self::Checkout(root) => root.join(relative),
            Self::Kit(kit) => KIT_LAYOUT
                .iter()
                .find_map(|(dir, prefix)| {
                    relative
                        .strip_prefix(prefix)
                        .map(|rest| kit.join(dir).join(rest))
                })
                .unwrap_or_else(|| kit.join(relative)),
        }
    }
}

/// The bundled compiler inside a preparation kit.
#[must_use]
pub fn kit_compiler(kit: &Path) -> PathBuf {
    kit.join("bin").join(if cfg!(windows) {
        "assetc.exe"
    } else {
        "assetc"
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn names_and_outputs_are_unique() {
        let mut names = HashSet::new();
        let mut files = HashSet::new();
        for carrier in CARRIERS {
            assert!(names.insert(carrier.name), "{}", carrier.name);
            for file in std::iter::once(carrier.output).chain(carrier.report) {
                assert!(files.insert(file), "{file}");
            }
        }
    }

    #[test]
    fn a_carrier_reads_only_earlier_carriers_built_in_the_same_scope() {
        for carrier in CARRIERS {
            for &read in carrier.reads {
                let dependency = by_name(read).expect("every dependency names a carrier");
                let position = |name| CARRIERS.iter().position(|c| c.name == name);
                assert!(
                    position(read) < position(carrier.name),
                    "{} must follow {}, which it reads",
                    carrier.name,
                    dependency.name
                );
                assert!(
                    dependency.installed || !carrier.installed,
                    "installed {} reads development-only {}",
                    carrier.name,
                    dependency.name
                );
                assert!(
                    dependency.required || !carrier.required,
                    "required {} reads optional {}",
                    carrier.name,
                    dependency.name
                );
            }
        }
    }

    #[test]
    fn required_carriers_are_built_for_installs() {
        assert!(required().all(|carrier| carrier.installed));
    }

    #[test]
    fn shipped_fonts_are_optional_and_independent_of_the_game_pack() {
        for carrier in [FONT, FONT_SEVEN, FONT_TEN] {
            assert!(!carrier.required);
            assert!(carrier.installed);
            assert_eq!(carrier.recipe, Recipe::Font);
            assert!(!carrier.inputs.contains(&Input::Pack));
        }
    }

    #[test]
    fn every_tracked_input_exists_in_the_checkout() {
        let root = Sources::Checkout(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."));
        for carrier in CARRIERS {
            for input in carrier.inputs {
                if let Input::Manifest(path) | Input::File(path) | Input::FontFile(path) = input {
                    assert!(root.resolve(path).is_file(), "{}: {path}", carrier.name);
                }
            }
        }
    }

    #[test]
    fn kit_paths_follow_the_packaged_layout() {
        let kit = Sources::Kit(PathBuf::from("kit"));
        assert_eq!(
            kit.resolve(BLOCK_REGISTRY),
            Path::new("kit/data/block-registry-v2193.bin")
        );
        assert_eq!(
            kit.resolve(VANILLA_MANIFEST),
            Path::new("kit/assets/vanilla-source.json")
        );
    }

    #[test]
    fn required_present_needs_every_required_output() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!required_present(dir.path()));
        for carrier in required() {
            std::fs::write(dir.path().join(carrier.output), b"x").unwrap();
        }
        assert!(required_present(dir.path()));
        std::fs::remove_file(dir.path().join(WORLD.output)).unwrap();
        assert!(!required_present(dir.path()));
    }
}
