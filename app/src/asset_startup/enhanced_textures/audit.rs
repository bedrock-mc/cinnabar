use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use assets::{
    PBR_REF_COLOR, PBR_REF_HEIGHT, PBR_REF_MATERIAL, PBR_REF_NORMAL, PBR_REF_OCCLUSION,
    PBR_REF_SUBSURFACE,
};
use serde::{Deserialize, Serialize};

use super::REF_FALLBACK;

const PATH: &str = ".local/enhanced-pbr-audit.json";
const SCHEMA: u32 = 4;
const MAX_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Alias {
    pub(super) alias: String,
    pub(super) sources: usize,
    pub(super) mapped_sources: usize,
    pub(super) color: bool,
    pub(super) normal: bool,
    pub(super) material: bool,
    pub(super) height: bool,
    pub(super) occlusion: bool,
    pub(super) subsurface: bool,
    pub(super) missing: bool,
    pub(super) error: bool,
    pub(super) fallback: bool,
    pub(super) color_source: Option<String>,
    pub(super) normal_source: Option<String>,
    pub(super) material_source: Option<String>,
    pub(super) height_source: Option<String>,
    pub(super) native_color_size: Option<[u32; 2]>,
    pub(super) native_normal_size: Option<[u32; 2]>,
    pub(super) native_material_size: Option<[u32; 2]>,
    pub(super) color_style: String,
    pub(super) authored_normal: bool,
    pub(super) authored_material: bool,
    pub(super) derived_normal: bool,
    pub(super) full_material: bool,
    pub(super) photorealistic_color: bool,
    pub(super) native_512_photorealistic_color: bool,
    pub(super) native_512_photorealistic_material: bool,
    pub(super) authored_color: bool,
    pub(super) upload_tile_size: Option<u32>,
}

impl Alias {
    pub(super) fn new(alias: String, sources: usize) -> Self {
        Self {
            alias,
            sources,
            mapped_sources: 0,
            color: false,
            normal: false,
            material: false,
            height: false,
            occlusion: false,
            subsurface: false,
            missing: false,
            error: false,
            fallback: false,
            color_source: None,
            normal_source: None,
            material_source: None,
            height_source: None,
            native_color_size: None,
            native_normal_size: None,
            native_material_size: None,
            color_style: "unspecified".to_owned(),
            authored_normal: false,
            authored_material: false,
            derived_normal: false,
            full_material: false,
            photorealistic_color: false,
            native_512_photorealistic_color: false,
            native_512_photorealistic_material: false,
            authored_color: false,
            upload_tile_size: None,
        }
    }

    pub(super) fn apply_source(&mut self, texture: &pack_compiler::pbr::PbrTexture) {
        let source = texture.source();
        let path = |path: &Option<PathBuf>| {
            path.as_ref()
                .map(|path| path.to_string_lossy().replace('\\', "/"))
        };
        self.color_source = path(&source.color);
        self.normal_source = path(&source.normal);
        self.material_source = path(&source.material);
        self.height_source = path(&source.height);
        self.native_color_size = Some(source.native_color_size);
        self.native_normal_size = Some(source.native_normal_size);
        self.native_material_size = Some(source.native_material_size);
        self.color_style = source.color_style.as_str().to_owned();
        self.authored_normal = source.normal.is_some() && texture.flags() & PBR_REF_NORMAL != 0;
        self.authored_material = texture.flags() & PBR_REF_MATERIAL != 0;
    }

    pub(super) fn apply_reference(&mut self, reference: u32) {
        if reference == REF_FALLBACK {
            self.fallback = true;
            return;
        }
        self.mapped_sources += 1;
        self.color |= reference & PBR_REF_COLOR != 0;
        self.normal |= reference & PBR_REF_NORMAL != 0;
        self.material |= reference & PBR_REF_MATERIAL != 0;
        self.height |= reference & PBR_REF_HEIGHT != 0;
        self.occlusion |= reference & PBR_REF_OCCLUSION != 0;
        self.subsurface |= reference & PBR_REF_SUBSURFACE != 0;
    }

    pub(super) fn finish(&mut self) {
        self.fallback |= self.mapped_sources < self.sources;
        self.authored_normal &= self.normal;
        self.authored_material &= self.material;
        self.derived_normal = self.normal && !self.authored_normal;
        self.full_material =
            self.color && self.authored_normal && self.authored_material && !self.fallback;
        self.authored_color = self.color
            && self.color_source.is_some()
            && self.color_style != pack_compiler::pbr::PbrColorStyle::Vanilla.as_str();
        self.photorealistic_color = self.color
            && self.color_style == pack_compiler::pbr::PbrColorStyle::Photorealistic.as_str();
        self.native_512_photorealistic_color = self.photorealistic_color
            && self.color_source.is_some()
            && self
                .native_color_size
                .is_some_and(|size| size.into_iter().min().unwrap_or(0) >= assets::PBR_TILE_SIZE);
        self.native_512_photorealistic_material = self.native_512_photorealistic_color
            && self.full_material
            && [self.native_normal_size, self.native_material_size]
                .into_iter()
                .all(|size| {
                    size.is_some_and(|size| {
                        size.into_iter().min().unwrap_or(0) >= assets::PBR_TILE_SIZE
                    })
                });
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Report {
    pub(super) schema: u32,
    pub(super) fingerprint: String,
    pub(super) source: String,
    pub(super) tile_size: u32,
    pub(super) page_sizes: Option<[u32; 2]>,
    pub(super) mapped_texture_refs: usize,
    pub(super) albedo_layers: u32,
    pub(super) aliases: Vec<Alias>,
}

impl Report {
    pub(super) fn decoded(
        key: &[u8; 32],
        aliases: Vec<Alias>,
        mapped_texture_refs: usize,
        albedo_layers: u32,
        page_sizes: Option<[u32; 2]>,
    ) -> Self {
        Self {
            schema: SCHEMA,
            fingerprint: fingerprint(key),
            source: "decoded".to_owned(),
            tile_size: assets::PBR_TILE_SIZE,
            page_sizes,
            mapped_texture_refs,
            albedo_layers,
            aliases,
        }
    }

    fn summary(&self) -> Summary {
        let mut summary = Summary::default();
        for alias in &self.aliases {
            summary.aliases += 1;
            summary.color += usize::from(alias.color);
            summary.normal += usize::from(alias.normal);
            summary.material += usize::from(alias.material);
            summary.height += usize::from(alias.height);
            summary.occlusion += usize::from(alias.occlusion);
            summary.subsurface += usize::from(alias.subsurface);
            summary.missing += usize::from(alias.missing);
            summary.errors += usize::from(alias.error);
            summary.fallback += usize::from(alias.fallback);
            summary.authored_normal += usize::from(alias.authored_normal);
            summary.derived_normal += usize::from(alias.derived_normal);
            summary.full_material += usize::from(alias.full_material);
            summary.photorealistic_color += usize::from(alias.photorealistic_color);
            summary.native_512_photorealistic_color +=
                usize::from(alias.native_512_photorealistic_color);
            summary.native_512_photorealistic_material +=
                usize::from(alias.native_512_photorealistic_material);
            summary.authored_color += usize::from(alias.authored_color);
        }
        summary
    }

    pub(super) fn print(&self) {
        let summary = self.summary();
        let pages = self.page_sizes.map_or_else(
            || "none".to_owned(),
            |[high, low]| format!("{high}x{high} and {low}x{low}"),
        );
        eprintln!(
            "Enhanced terrain audit ({}): {}/{} supplied color aliases, {} mapped texture refs, {} albedo layers; upload pages {}; {} normal, {} material, {} height, {} occlusion, {} subsurface; {} without normal, {} without material, {} without height; {} missing colors, {} errors, {} fallbacks; report {}",
            self.source,
            summary.color,
            summary.aliases,
            self.mapped_texture_refs,
            self.albedo_layers,
            pages,
            summary.normal,
            summary.material,
            summary.height,
            summary.occlusion,
            summary.subsurface,
            summary.color.saturating_sub(summary.normal),
            summary.color.saturating_sub(summary.material),
            summary.color.saturating_sub(summary.height),
            summary.missing,
            summary.errors,
            summary.fallback,
            PATH,
        );
        eprintln!(
            "Enhanced authored coverage: {}/{} complete authored normal/material aliases with color; {} authored color aliases, {} authored normals, {} height-derived normals; {} declared photorealistic color aliases, {} with native color frames at least {} pixels, {} with all three native maps at that resolution",
            summary.full_material,
            summary.aliases,
            summary.authored_color,
            summary.authored_normal,
            summary.derived_normal,
            summary.photorealistic_color,
            summary.native_512_photorealistic_color,
            self.tile_size,
            summary.native_512_photorealistic_material,
        );
    }
}

#[derive(Default)]
struct Summary {
    aliases: usize,
    color: usize,
    normal: usize,
    material: usize,
    height: usize,
    occlusion: usize,
    subsurface: usize,
    missing: usize,
    errors: usize,
    fallback: usize,
    authored_normal: usize,
    derived_normal: usize,
    full_material: usize,
    photorealistic_color: usize,
    native_512_photorealistic_color: usize,
    native_512_photorealistic_material: usize,
    authored_color: usize,
}

fn fingerprint(key: &[u8; 32]) -> String {
    key.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(super) fn load(key: &[u8; 32]) -> Option<Report> {
    let path = Path::new(PATH);
    let size = fs::metadata(path).ok()?.len();
    if size > MAX_BYTES {
        return None;
    }
    let report: Report = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    (report.schema == SCHEMA && report.fingerprint == fingerprint(key)).then_some(report)
}

pub(super) fn save(report: &Report) {
    let temporary = PathBuf::from(format!("{PATH}.{}.tmp", std::process::id()));
    let result = (|| -> std::io::Result<()> {
        fs::create_dir_all(Path::new(PATH).parent().unwrap_or(Path::new(".")))?;
        let bytes = serde_json::to_vec_pretty(report).map_err(std::io::Error::other)?;
        let mut file = fs::File::create(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        if Path::new(PATH).is_file() {
            fs::remove_file(PATH)?;
        }
        fs::rename(&temporary, PATH)
    })();
    if let Err(error) = result {
        eprintln!("authored Enhanced texture audit: {error}");
        let _ = fs::remove_file(temporary);
    }
}
