//! The host description packs see through Iris's standard macros
//! (<https://shaders.properties/current/reference/macros/>).

/// Operating system macro (`MC_OS_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostOs {
    Windows,
    MacOs,
    Linux,
    Other,
}

/// GPU vendor family, selecting `MC_GL_VENDOR_*` and `MC_GL_RENDERER_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Other,
}

/// The Iris release whose behaviour the translator and host follow, and whose
/// `IRIS_VERSION` packs see. It runs on the Java Edition named by the pinned GeyserMC mappings
/// ([`assets::facts::java_mapping::java_edition`]), which packs see as `MC_VERSION`.
pub const EMULATED_IRIS_VERSION: &str = "1.11.7";

/// Iris's numeric form of a dotted version (`StandardMacros.formatVersionString`): the
/// major part, then minor and patch as two digits each (`26.3` → 260300, `1.11.7` → 11107).
/// `None` when the version has fewer than two parts or is not numeric.
pub fn iris_version_number(version: &str) -> Option<u32> {
    let mut parts = version.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    let patch: u32 = parts.next().map_or(Some(0), |p| p.parse().ok())?;
    Some(major * 10_000 + minor * 100 + patch)
}

/// Values used to inspect pack metadata under a chosen compatibility environment.
/// The macro set describes that environment, including a fixed set of core GL extensions
/// and Iris features; it does not detect host capabilities or activate rendering.
#[derive(Clone, Debug, PartialEq)]
pub struct Environment {
    /// `MC_VERSION`, e.g. 260300 for 26.3.
    pub mc_version: u32,
    /// `MC_GL_VERSION` and `MC_GLSL_VERSION`.
    pub gl_version: u32,
    /// `IRIS_VERSION`, e.g. 11107 for 1.11.7.
    pub iris_version: u32,
    pub os: HostOs,
    pub vendor: GpuVendor,
    /// `MC_RENDER_QUALITY` and `MC_SHADOW_QUALITY`.
    pub render_quality: f32,
    pub shadow_quality: f32,
    /// `MC_HAND_DEPTH`.
    pub hand_depth: f32,
    /// `MC_NORMAL_MAP`, `MC_SPECULAR_MAP` and the labPBR format macros.
    pub normal_map: bool,
    pub specular_map: bool,
    pub lab_pbr: bool,
    /// `MC_ANISOTROPIC_FILTERING` level; 0 leaves it undefined. OptiFine defines it; Iris
    /// never does, and Complementary replaces its image with an error screen when it is set.
    pub anisotropic_filtering: u32,
    /// Distant Horizons is present: defines `DISTANT_HORIZONS` and loads `dh_*` programs.
    pub distant_horizons: bool,
}

impl Default for Environment {
    /// Provides the default compatibility environment for metadata inspection.
    fn default() -> Environment {
        Environment {
            mc_version: iris_version_number(assets::facts::java_mapping::java_edition())
                .unwrap_or_default(),
            gl_version: 460,
            iris_version: iris_version_number(EMULATED_IRIS_VERSION).unwrap_or_default(),
            os: HostOs::Windows,
            vendor: GpuVendor::Nvidia,
            render_quality: 1.0,
            shadow_quality: 1.0,
            hand_depth: 0.125,
            normal_map: true,
            specular_map: true,
            lab_pbr: true,
            anisotropic_filtering: 0,
            distant_horizons: false,
        }
    }
}

/// Iris feature flags in the compatibility macro environment (`IRIS_FEATURE_*`).
const IRIS_FEATURES: &[&str] = &[
    "CUSTOM_IMAGES",
    "COMPUTE_SHADERS",
    "SSBO",
    "HIGHER_SHADOWCOLOR",
    "SEPARATE_HARDWARE_SAMPLERS",
    "PER_BUFFER_BLENDING",
    "ENTITY_TRANSLUCENT",
    "FADE_VARIABLE",
    "BLOCK_EMISSION_ATTRIBUTE",
    "CUSTOM_TEXTURE_NAME",
];

/// Core GL 4.6 extensions advertised as `MC_GL_<name>`.
const CORE_EXTENSIONS: &[&str] = &[
    "ARB_shader_image_load_store",
    "ARB_compute_shader",
    "ARB_shader_storage_buffer_object",
    "ARB_gpu_shader5",
    "ARB_shader_texture_lod",
    "ARB_texture_gather",
    "ARB_shading_language_packing",
    "ARB_shader_bit_encoding",
    "EXT_gpu_shader4",
];

/// `MC_RENDER_STAGE_*` in Iris's numbering.
const RENDER_STAGES: &[&str] = &[
    "NONE",
    "SKY",
    "SUNSET",
    "CUSTOM_SKY",
    "SUN",
    "MOON",
    "STARS",
    "VOID",
    "TERRAIN_SOLID",
    "TERRAIN_CUTOUT_MIPPED",
    "TERRAIN_CUTOUT",
    "ENTITIES",
    "BLOCK_ENTITIES",
    "DESTROY",
    "OUTLINE",
    "DEBUG",
    "HAND_SOLID",
    "TERRAIN_TRANSLUCENT",
    "TRIPWIRE",
    "PARTICLES",
    "CLOUDS",
    "RAIN_SNOW",
    "WORLD_BORDER",
    "HAND_TRANSLUCENT",
];

/// `DH_BLOCK_*` material ids in Distant Horizons' numbering.
const DH_BLOCKS: &[&str] = &[
    "UNKNOWN",
    "LEAVES",
    "STONE",
    "WOOD",
    "METAL",
    "DIRT",
    "LAVA",
    "DEEPSLATE",
    "SNOW",
    "SAND",
    "TERRACOTTA",
    "NETHER_STONE",
    "WATER",
    "GRASS",
    "AIR",
    "ILLUMINATED",
];

impl Environment {
    /// The standard macros as (name, value) pairs; flag macros have an empty value.
    pub fn standard_defines(&self) -> Vec<(String, String)> {
        let mut d: Vec<(String, String)> = Vec::new();
        let mut add = |k: &str, v: String| d.push((k.to_string(), v));
        add("MC_VERSION", self.mc_version.to_string());
        add("MC_GL_VERSION", self.gl_version.to_string());
        add("MC_GLSL_VERSION", self.gl_version.to_string());
        let os = match self.os {
            HostOs::Windows => "MC_OS_WINDOWS",
            HostOs::MacOs => "MC_OS_MAC",
            HostOs::Linux => "MC_OS_LINUX",
            HostOs::Other => "MC_OS_UNKNOWN",
        };
        add(os, String::new());
        let (vendors, renderer): (&[&str], &str) = match self.vendor {
            GpuVendor::Nvidia => (&["MC_GL_VENDOR_NVIDIA"], "MC_GL_RENDERER_GEFORCE"),
            GpuVendor::Amd => (
                &["MC_GL_VENDOR_AMD", "MC_GL_VENDOR_ATI"],
                "MC_GL_RENDERER_RADEON",
            ),
            GpuVendor::Intel => (&["MC_GL_VENDOR_INTEL"], "MC_GL_RENDERER_INTEL"),
            GpuVendor::Other => (&["MC_GL_VENDOR_OTHER"], "MC_GL_RENDERER_OTHER"),
        };
        for v in vendors {
            add(v, String::new());
        }
        add(renderer, String::new());
        add("MC_RENDER_QUALITY", float(self.render_quality));
        add("MC_SHADOW_QUALITY", float(self.shadow_quality));
        add("MC_HAND_DEPTH", float(self.hand_depth));
        if self.normal_map {
            add("MC_NORMAL_MAP", String::new());
        }
        if self.specular_map {
            add("MC_SPECULAR_MAP", String::new());
        }
        if self.lab_pbr {
            add("MC_TEXTURE_FORMAT_LAB_PBR", String::new());
            add("MC_TEXTURE_FORMAT_LAB_PBR_1_3", String::new());
        }
        if self.anisotropic_filtering > 0 {
            add(
                "MC_ANISOTROPIC_FILTERING",
                self.anisotropic_filtering.to_string(),
            );
        }
        add("IS_IRIS", String::new());
        add("IRIS_VERSION", self.iris_version.to_string());
        add("IRIS_TAG_SUPPORT", "2".to_string());
        for f in IRIS_FEATURES {
            add(&format!("IRIS_FEATURE_{f}"), String::new());
        }
        for e in CORE_EXTENSIONS {
            add(&format!("MC_GL_{e}"), String::new());
        }
        for (i, s) in RENDER_STAGES.iter().enumerate() {
            add(&format!("MC_RENDER_STAGE_{s}"), i.to_string());
        }
        for (i, s) in DH_BLOCKS.iter().enumerate() {
            add(&format!("DH_BLOCK_{s}"), i.to_string());
        }
        if self.distant_horizons {
            add("DISTANT_HORIZONS", String::new());
        }
        d.extend(crate::biomes::biome_defines(
            assets::facts::java_mapping::JavaBlockMapping::pinned().ok(),
        ));
        d
    }
}

/// Formats a macro value with a decimal point or exponent for GLSL.
fn float(v: f32) -> String {
    let s = format!("{v:?}");
    if s.contains('.') || s.contains('e') {
        s
    } else {
        format!("{s}.0")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn define<'a>(defines: &'a [(String, String)], name: &str) -> Option<&'a str> {
        defines
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn versions_use_iris_number_format() {
        assert_eq!(iris_version_number("26.3"), Some(260300));
        assert_eq!(iris_version_number("1.21.11"), Some(12111));
        assert_eq!(iris_version_number("1.11.7"), Some(11107));
        assert_eq!(iris_version_number("26"), None);
        assert_eq!(iris_version_number("1.x"), None);
    }

    #[test]
    fn default_macros_match_the_emulated_iris_release() {
        let d = Environment::default().standard_defines();
        let java = iris_version_number(assets::facts::java_mapping::java_edition())
            .expect("the pinned Java edition is a dotted version");
        assert_eq!(define(&d, "MC_VERSION"), Some(java.to_string().as_str()));
        assert_eq!(define(&d, "IRIS_VERSION"), Some("11107"));
        assert_eq!(define(&d, "MC_GL_VERSION"), Some("460"));
        assert_eq!(define(&d, "MC_GLSL_VERSION"), Some("460"));
        // Iris never defines it; Complementary shows an error screen when it is set.
        assert_eq!(define(&d, "MC_ANISOTROPIC_FILTERING"), None);
    }
}
