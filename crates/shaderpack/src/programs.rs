//! Which programs a pack defines for a dimension, and where their stage files live.

use crate::source::PackSource;
use std::collections::BTreeMap;

/// A shader stage file kind, by extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StageKind {
    Vertex,
    Geometry,
    TessControl,
    TessEval,
    Fragment,
    Compute,
}

impl StageKind {
    /// Every stage, in pipeline order (vertex outputs are linked before fragment inputs).
    pub const ALL: [StageKind; 6] = [
        StageKind::Vertex,
        StageKind::TessControl,
        StageKind::TessEval,
        StageKind::Geometry,
        StageKind::Fragment,
        StageKind::Compute,
    ];

    /// The file extension, e.g. `vsh`.
    pub fn extension(self) -> &'static str {
        match self {
            StageKind::Vertex => "vsh",
            StageKind::Geometry => "gsh",
            StageKind::TessControl => "tcs",
            StageKind::TessEval => "tes",
            StageKind::Fragment => "fsh",
            StageKind::Compute => "csh",
        }
    }

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
    ("dh_generic", None),
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
    if GEOMETRY_FALLBACKS.iter().any(|(n, _)| *n == name) {
        return Some(if name.starts_with("dh_") {
            ProgramGroup::DistantHorizons
        } else if name.starts_with("shadow") {
            ProgramGroup::Shadow
        } else {
            ProgramGroup::Gbuffers
        });
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
            return Some(*group);
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

/// The folder of `dimension` (a namespaced id like `minecraft:the_nether`):
/// `dimension.properties` (`dimension.<folder> = <id> …`, `*` matching any id) first, then
/// the vanilla folders, then `world0`.
pub fn dimension_folder(source: &PackSource, dimension: &str) -> String {
    if let Some(props) = source.text("dimension.properties") {
        let mut wildcard = None;
        for line in props.lines() {
            let line = line.trim();
            let Some(rest) = line.strip_prefix("dimension.") else {
                continue;
            };
            let Some((folder, ids)) = rest.split_once('=') else {
                continue;
            };
            let folder = folder.trim();
            for id in ids.split_whitespace() {
                if id == dimension {
                    return folder.to_string();
                }
                if id == "*" {
                    wildcard = Some(folder.to_string());
                }
            }
        }
        if let Some(folder) = wildcard {
            return folder;
        }
    }
    match dimension {
        "minecraft:the_nether" => "world-1",
        "minecraft:the_end" => "world1",
        _ => "world0",
    }
    .to_string()
}

/// A program's stage files as (stage, `shaders/`-relative path).
type StageFiles = Vec<(StageKind, String)>;

/// Every program Iris would load for dimension folder `folder`: a program found in the
/// folder takes all its stages from there, otherwise from the `shaders/` root. Files whose
/// stem is not an Iris program name are ignored. Sorted by group, then name, render first.
pub fn enumerate_programs(source: &PackSource, folder: &str) -> Vec<ProgramSource> {
    // (name, kind) -> folder -> stages
    let mut found: BTreeMap<(String, ProgramKind), BTreeMap<String, StageFiles>> = BTreeMap::new();
    for (path, _) in source.files() {
        let (dir, file) = match path.rsplit_once('/') {
            Some((d, f)) => (d, f),
            None => ("", path),
        };
        if dir != folder && !dir.is_empty() {
            continue;
        }
        let Some((stem, ext)) = file.rsplit_once('.') else {
            continue;
        };
        let Some(stage) = StageKind::from_extension(ext) else {
            continue;
        };
        if program_group(stem).is_none() {
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
            .entry(dir.to_string())
            .or_default()
            .push((stage, path.to_string()));
    }
    let mut out: Vec<ProgramSource> = found
        .into_iter()
        .filter_map(|((name, kind), mut by_folder)| {
            let (dir, mut stages) = by_folder
                .remove_entry(folder)
                .or_else(|| by_folder.remove_entry(""))?;
            stages.sort();
            Some(ProgramSource {
                group: program_group(&name)?,
                name,
                kind,
                folder: dir,
                stages,
            })
        })
        .collect();
    out.sort_by(|a, b| {
        (a.group, natural_key(&a.name), a.kind).cmp(&(b.group, natural_key(&b.name), b.kind))
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
                ("composite", "world0", ProgramKind::Render),
                ("composite_a", "world0", ProgramKind::Compute),
                ("final", "", ProgramKind::Render),
            ]
        );
        assert_eq!(
            programs[0].stages,
            [
                (StageKind::Vertex, "world0/composite.vsh".to_string()),
                (StageKind::Fragment, "world0/composite.fsh".to_string()),
            ]
        );
    }

    #[test]
    fn dimension_properties_map_ids_with_wildcard_and_default_to_world0() {
        let source = PackSource::from_files([(
            "dimension.properties",
            "dimension.world-1 = minecraft:the_nether\ndimension.world0 = minecraft:overworld *\n",
        )]);
        assert_eq!(dimension_folder(&source, "minecraft:the_nether"), "world-1");
        assert_eq!(dimension_folder(&source, "mymod:moon"), "world0");
        let bare = PackSource::from_files([("a.fsh", "")]);
        assert_eq!(dimension_folder(&bare, "minecraft:the_end"), "world1");
        assert_eq!(dimension_folder(&bare, "mymod:moon"), "world0");
    }
}
