//! Which programs a pack defines for a dimension, and where their stage files live.

use crate::source::PackSource;
use std::collections::BTreeMap;

/// Declares stages and their discovery metadata together in pipeline order.
macro_rules! stage_kinds {
    ($($kind:ident => $extension:literal),+ $(,)?) => {
        /// A shader stage file kind, by extension.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum StageKind { $($kind),+ }

        impl StageKind {
            /// Every stage, in pipeline order.
            pub const ALL: [Self; { [$(stringify!($kind)),+].len() }] = [$(Self::$kind),+];

            /// The file extension, e.g. `vsh`.
            pub fn extension(self) -> &'static str {
                match self { $(Self::$kind => $extension),+ }
            }
        }
    };
}

stage_kinds! {
    Vertex => "vsh",
    TessControl => "tcs",
    TessEval => "tes",
    Geometry => "gsh",
    Fragment => "fsh",
    Compute => "csh",
}

impl StageKind {
    /// Finds the stage kind associated with a shader file extension.
    fn from_extension(ext: &str) -> Option<StageKind> {
        StageKind::ALL.into_iter().find(|s| s.extension() == ext)
    }
}

/// The pass a program belongs to, in Iris's frame order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProgramGroup {
    /// `setup*` compute programs, run once after loading.
    Setup,
    /// `begin*`, each frame before shadows.
    Begin,
    /// `shadow` and its geometry-specific variants.
    Shadow,
    /// `shadowcomp*` full-screen passes after the shadow pass.
    ShadowComposite,
    /// `prepare*` full-screen passes before terrain.
    Prepare,
    /// `gbuffers_*` geometry programs.
    Gbuffers,
    /// `dh_*` Distant Horizons geometry programs (only with Distant Horizons present).
    DistantHorizons,
    /// `deferred*` full-screen passes after opaque geometry.
    Deferred,
    /// `composite*` full-screen passes after translucent geometry.
    Composite,
    /// `final`, writing the screen.
    Final,
}

impl ProgramGroup {
    /// The `<stage>` of `texture.<stage>.<sampler>` bindings that apply to the group, as
    /// Iris's `TextureStage` names it: shadow programs share the gbuffers bindings and
    /// `final` shares the composite ones.
    pub fn texture_stage(self) -> &'static str {
        match self {
            ProgramGroup::Setup => "setup",
            ProgramGroup::Begin => "begin",
            ProgramGroup::ShadowComposite => "shadowcomp",
            ProgramGroup::Prepare => "prepare",
            ProgramGroup::Shadow | ProgramGroup::Gbuffers | ProgramGroup::DistantHorizons => {
                "gbuffers"
            }
            ProgramGroup::Deferred => "deferred",
            ProgramGroup::Composite | ProgramGroup::Final => "composite",
        }
    }
}

/// Whether a program is a draw (vertex + fragment, optional geometry/tessellation) or a
/// compute dispatch. A composite-style pass may have both: its `.csh` files run first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProgramKind {
    Render,
    Compute,
}

/// One program of the selected dimension: its name (the file stem, e.g. `composite3` or
/// `deferred4_a`), pass, kind, and stage files (`shaders/`-relative paths).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramSource {
    pub name: String,
    pub group: ProgramGroup,
    pub kind: ProgramKind,
    /// Folder the files came from: the dimension folder (e.g. `world0`) or `""` for the root.
    pub folder: String,
    pub stages: Vec<(StageKind, String)>,
}

/// Iris's geometry programs and the program each falls back to when the pack lacks it.
pub const GEOMETRY_FALLBACKS: &[(&str, Option<&str>)] = &[
    ("gbuffers_basic", None),
    ("gbuffers_line", Some("gbuffers_basic")),
    ("gbuffers_textured", Some("gbuffers_basic")),
    ("gbuffers_textured_lit", Some("gbuffers_textured")),
    ("gbuffers_skybasic", Some("gbuffers_basic")),
    ("gbuffers_skytextured", Some("gbuffers_textured")),
    ("gbuffers_clouds", Some("gbuffers_textured")),
    ("gbuffers_terrain", Some("gbuffers_textured_lit")),
    ("gbuffers_terrain_solid", Some("gbuffers_terrain")),
    ("gbuffers_terrain_cutout", Some("gbuffers_terrain")),
    ("gbuffers_damagedblock", Some("gbuffers_terrain")),
    ("gbuffers_block", Some("gbuffers_terrain")),
    ("gbuffers_block_translucent", Some("gbuffers_block")),
    ("gbuffers_beaconbeam", Some("gbuffers_textured")),
    ("gbuffers_item", Some("gbuffers_textured_lit")),
    ("gbuffers_entities", Some("gbuffers_textured_lit")),
    ("gbuffers_entities_translucent", Some("gbuffers_entities")),
    ("gbuffers_lightning", Some("gbuffers_entities")),
    ("gbuffers_particles", Some("gbuffers_textured_lit")),
    ("gbuffers_particles_translucent", Some("gbuffers_particles")),
    ("gbuffers_entities_glowing", Some("gbuffers_entities")),
    ("gbuffers_armor_glint", Some("gbuffers_textured")),
    ("gbuffers_spidereyes", Some("gbuffers_textured")),
    ("gbuffers_hand", Some("gbuffers_textured_lit")),
    ("gbuffers_weather", Some("gbuffers_textured_lit")),
    ("gbuffers_water", Some("gbuffers_terrain")),
    ("gbuffers_hand_water", Some("gbuffers_hand")),
    ("dh_terrain", None),
    ("dh_water", Some("dh_terrain")),
    ("dh_generic", Some("dh_terrain")),
    ("dh_shadow", None),
    ("shadow", None),
    ("shadow_solid", Some("shadow")),
    ("shadow_cutout", Some("shadow")),
    ("shadow_water", Some("shadow")),
    ("shadow_entities", Some("shadow")),
    ("shadow_lightning", Some("shadow_entities")),
    ("shadow_block", Some("shadow")),
];

/// Composite-style families: `<family>`, `<family>1`…`<family>99`, plus `_a`…`_z` compute
/// variants. `setup` is compute-only.
const PASS_FAMILIES: &[(&str, ProgramGroup)] = &[
    ("setup", ProgramGroup::Setup),
    ("begin", ProgramGroup::Begin),
    ("shadowcomp", ProgramGroup::ShadowComposite),
    ("prepare", ProgramGroup::Prepare),
    ("deferred", ProgramGroup::Deferred),
    ("composite", ProgramGroup::Composite),
    ("final", ProgramGroup::Final),
];

/// The pass of a program name, or `None` for names Iris does not load.
pub fn program_group(name: &str) -> Option<ProgramGroup> {
    program_name(name).map(|(group, _)| group)
}

/// Resolves a known name and whether its family and suffix permit render stages.
fn program_name(name: &str) -> Option<(ProgramGroup, bool)> {
    // `shadow_a`…`shadow_z` are compute variants of the shadow pass.
    if name
        .strip_prefix("shadow_")
        .is_some_and(|s| s.len() == 1 && s.as_bytes()[0].is_ascii_lowercase())
    {
        return Some((ProgramGroup::Shadow, false));
    }
    if GEOMETRY_FALLBACKS.iter().any(|(n, _)| *n == name) {
        return Some((
            if name.starts_with("dh_") {
                ProgramGroup::DistantHorizons
            } else if name.starts_with("shadow") {
                ProgramGroup::Shadow
            } else {
                ProgramGroup::Gbuffers
            },
            true,
        ));
    }
    for (family, group) in PASS_FAMILIES {
        let Some(rest) = name.strip_prefix(family) else {
            continue;
        };
        let (index, suffix) = match rest.split_once('_') {
            Some((i, s)) => (i, Some(s)),
            None => (rest, None),
        };
        let index_ok = index.is_empty()
            || (*family != "final"
                && index.len() <= 2
                && index.bytes().all(|b| b.is_ascii_digit())
                && !index.starts_with('0'));
        let suffix_ok = suffix.is_none_or(|s| s.len() == 1 && s.as_bytes()[0].is_ascii_lowercase());
        if index_ok && suffix_ok {
            return Some((*group, *group != ProgramGroup::Setup && suffix.is_none()));
        }
    }
    None
}

/// The geometry program used for `name` when the pack does not define it, per Iris.
pub fn geometry_fallback(name: &str) -> Option<&'static str> {
    GEOMETRY_FALLBACKS
        .iter()
        .find(|(n, _)| *n == name)
        .and_then(|(_, f)| *f)
}

/// Recognizes a program stage path and returns its directory, stem and stage.
pub(crate) fn program_file(path: &str) -> Option<(&str, &str, StageKind)> {
    let (directory, file) = path.rsplit_once('/').unwrap_or(("", path));
    let (stem, extension) = file.rsplit_once('.')?;
    let (_, render) = program_name(stem)?;
    let stage = StageKind::from_extension(extension)?;
    (stage == StageKind::Compute || render).then_some((directory, stem, stage))
}

/// A program's stage files as (stage, `shaders/`-relative path).
type StageFiles = Vec<(StageKind, String)>;

/// Enumerates one complete program directory in frame order: by group, then pass index, with a
/// pass's compute programs (`<pass>.csh`, then `<pass>_a`…`_z`) dispatched before its draw, as
/// Iris does.
pub fn enumerate_programs(source: &PackSource, folder: &str) -> Vec<ProgramSource> {
    let mut found: BTreeMap<(String, ProgramKind), StageFiles> = BTreeMap::new();
    for (path, _) in source.files() {
        let Some((dir, stem, stage)) = program_file(path) else {
            continue;
        };
        if dir != folder {
            continue;
        }
        let kind = if stage == StageKind::Compute {
            ProgramKind::Compute
        } else {
            ProgramKind::Render
        };
        found
            .entry((stem.to_string(), kind))
            .or_default()
            .push((stage, path.to_string()));
    }
    let mut out: Vec<ProgramSource> = found
        .into_iter()
        .filter_map(|((name, kind), mut stages)| {
            stages.sort();
            Some(ProgramSource {
                group: program_group(&name)?,
                name,
                kind,
                folder: folder.to_string(),
                stages,
            })
        })
        .collect();
    out.sort_by_key(|p| {
        let (family, index, suffix) = natural_key(&p.name);
        (
            p.group,
            family,
            index,
            p.kind == ProgramKind::Render,
            suffix,
        )
    });
    out
}

/// Orders `composite2` before `composite10`.
fn natural_key(name: &str) -> (String, u32, String) {
    let alpha_end = name
        .find(|c: char| c.is_ascii_digit() || c == '_')
        .unwrap_or(name.len());
    let rest = &name[alpha_end..];
    let digits_end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    let index = rest[..digits_end].parse().unwrap_or(0);
    (
        name[..alpha_end].to_string(),
        index,
        rest[digits_end..].to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dimensions::DimensionSelector;

    #[test]
    fn geometry_catalogue_has_unique_names_and_terminating_fallbacks() {
        let names: std::collections::BTreeSet<_> =
            GEOMETRY_FALLBACKS.iter().map(|(name, _)| *name).collect();
        assert_eq!(names.len(), GEOMETRY_FALLBACKS.len());
        let source = PackSource::from_files(GEOMETRY_FALLBACKS.iter().flat_map(|(name, _)| {
            StageKind::ALL.map(|stage| (format!("{name}.{}", stage.extension()), ""))
        }));
        let programs = enumerate_programs(&source, "");
        assert_eq!(programs.len(), GEOMETRY_FALLBACKS.len() * 2);
        for &(name, fallback) in GEOMETRY_FALLBACKS {
            assert_eq!(geometry_fallback(name), fallback);
            let mut visited = std::collections::BTreeSet::new();
            let mut current = Some(name);
            while let Some(name) = current {
                assert!(names.contains(name), "unknown fallback {name}");
                assert!(visited.insert(name), "fallback cycle at {name}");
                current = geometry_fallback(name);
            }
            for program in programs.iter().filter(|program| program.name == name) {
                assert_eq!(Some(program.group), program_group(name));
                let expected: std::collections::BTreeSet<_> = StageKind::ALL
                    .into_iter()
                    .filter(|stage| {
                        (*stage == StageKind::Compute) == (program.kind == ProgramKind::Compute)
                    })
                    .collect();
                let actual: std::collections::BTreeSet<_> =
                    program.stages.iter().map(|(stage, _)| *stage).collect();
                assert_eq!(program.stages.len(), expected.len());
                assert_eq!(actual, expected, "{name}");
            }
        }
    }

    #[test]
    fn pass_family_grammar_covers_indices_and_compute_suffixes() {
        for &(family, group) in PASS_FAMILIES {
            for index in 0..=100 {
                let name = if index == 0 {
                    family.to_string()
                } else {
                    format!("{family}{index}")
                };
                let valid = index == 0 || (family != "final" && index < 100);
                assert_eq!(program_group(&name), valid.then_some(group), "{name}");
                assert_eq!(
                    program_file(&format!("{name}.csh")).is_some(),
                    valid,
                    "{name}"
                );
                assert_eq!(
                    program_file(&format!("{name}.fsh")).is_some(),
                    valid && group != ProgramGroup::Setup,
                    "{name}"
                );
            }
            for suffix in 'a'..='z' {
                let name = format!("{family}_{suffix}");
                assert_eq!(program_group(&name), Some(group));
                assert!(program_file(&format!("{name}.csh")).is_some());
                assert!(program_file(&format!("{name}.fsh")).is_none());
            }
            for suffix in ["00", "01", "_aa", "_A", "_"] {
                assert_eq!(program_group(&format!("{family}{suffix}")), None);
            }
        }
    }

    #[test]
    fn program_names_follow_iris() {
        assert_eq!(program_group("composite"), Some(ProgramGroup::Composite));
        assert_eq!(program_group("composite15"), Some(ProgramGroup::Composite));
        assert_eq!(program_group("deferred4_a"), Some(ProgramGroup::Deferred));
        assert_eq!(
            program_group("shadowcomp1"),
            Some(ProgramGroup::ShadowComposite)
        );
        assert_eq!(program_group("shadow_cutout"), Some(ProgramGroup::Shadow));
        assert_eq!(
            program_group("gbuffers_water"),
            Some(ProgramGroup::Gbuffers)
        );
        assert_eq!(
            program_group("dh_water"),
            Some(ProgramGroup::DistantHorizons)
        );
        assert_eq!(program_group("final"), Some(ProgramGroup::Final));
        assert_eq!(program_group("composite100"), None);
        assert_eq!(program_group("composite01"), None);
        assert_eq!(program_group("final1"), None);
        assert_eq!(program_group("gbuffers_unknown"), None);
        assert_eq!(
            geometry_fallback("gbuffers_hand_water"),
            Some("gbuffers_hand")
        );
        assert_eq!(geometry_fallback("dh_generic"), Some("dh_terrain"));
        assert_eq!(program_group("shadow_a"), Some(ProgramGroup::Shadow));
        assert!(program_file("shadow_z.csh").is_some());
        assert!(program_file("shadow_a.fsh").is_none());
        assert_eq!(program_group("shadow_aa"), None);
    }

    #[test]
    fn dimension_folder_programs_override_root_programs() {
        let source = PackSource::from_files([
            ("final.vsh", ""),
            ("final.fsh", ""),
            ("composite.fsh", ""),
            ("composite.vsh", ""),
            ("world0/composite.fsh", ""),
            ("world0/composite.vsh", ""),
            ("world0/composite_a.csh", ""),
            ("world0/notes.txt", ""),
            ("lib/common.glsl", ""),
        ]);
        let programs = enumerate_programs(&source, "world0");
        let names: Vec<(&str, &str, ProgramKind)> = programs
            .iter()
            .map(|p| (p.name.as_str(), p.folder.as_str(), p.kind))
            .collect();
        assert_eq!(
            names,
            [
                ("composite_a", "world0", ProgramKind::Compute),
                ("composite", "world0", ProgramKind::Render),
            ]
        );
        assert_eq!(
            programs[1].stages,
            [
                (StageKind::Vertex, "world0/composite.vsh".to_string()),
                (StageKind::Fragment, "world0/composite.fsh".to_string()),
            ]
        );
    }

    #[test]
    fn a_pass_dispatches_its_computes_before_its_draw() {
        let source = PackSource::from_files([
            ("deferred4.vsh", ""),
            ("deferred4.fsh", ""),
            ("deferred4_a.csh", ""),
            ("deferred4.csh", ""),
            ("deferred3.fsh", ""),
            ("deferred3.vsh", ""),
            ("deferred10_b.csh", ""),
        ]);
        let order: Vec<(String, ProgramKind)> = enumerate_programs(&source, "")
            .into_iter()
            .map(|p| (p.name, p.kind))
            .collect();
        let expected = [
            ("deferred3", ProgramKind::Render),
            ("deferred4", ProgramKind::Compute),
            ("deferred4_a", ProgramKind::Compute),
            ("deferred4", ProgramKind::Render),
            ("deferred10_b", ProgramKind::Compute),
        ]
        .map(|(n, k)| (n.to_string(), k));
        assert_eq!(order, expected);
    }

    #[test]
    fn dimension_properties_map_ids_with_wildcard_and_default_to_world0() {
        let source = PackSource::from_files([
            (
                "dimension.properties",
                "dimension.world-1 = minecraft:the_nether\ndimension.world0 = minecraft:overworld *\n",
            ),
            ("world-1/final.fsh", ""),
            ("world0/final.fsh", ""),
        ]);
        assert_eq!(
            DimensionSelector::new(&source, &BTreeMap::new()).folder("minecraft:the_nether"),
            "world-1"
        );
        assert_eq!(
            DimensionSelector::new(&source, &BTreeMap::new()).folder("mymod:moon"),
            "world0"
        );
        let bare = PackSource::from_files([("a.fsh", "")]);
        assert_eq!(
            DimensionSelector::new(&bare, &BTreeMap::new()).folder("minecraft:the_end"),
            ""
        );
        assert_eq!(
            DimensionSelector::new(&bare, &BTreeMap::new()).folder("mymod:moon"),
            ""
        );
    }
}
