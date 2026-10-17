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

/// Values behind the standard macros. Vendor GL extensions (NV, AMD, KHR subgroup) are never
/// advertised, so packs take their portable code paths; every core GL 4.6 extension a pack
/// tests through `MC_GL_ARB_*` is advertised because the translated code targets GLSL 4.50.
#[derive(Clone, Debug, PartialEq)]
pub struct Environment {
    /// `MC_VERSION`, e.g. 12111 for 1.21.11.
    pub mc_version: u32,
    /// `MC_GL_VERSION` and `MC_GLSL_VERSION`.
    pub gl_version: u32,
    /// `IRIS_VERSION`, e.g. 10900 for 1.9.0.
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
    /// `MC_ANISOTROPIC_FILTERING` level; 0 leaves it undefined.
    pub anisotropic_filtering: u32,
    /// Distant Horizons is present: defines `DISTANT_HORIZONS` and loads `dh_*` programs.
    pub distant_horizons: bool,
}

impl Default for Environment {
    fn default() -> Environment {
        Environment {
            mc_version: 12111,
            gl_version: 460,
            iris_version: 10900,
            os: HostOs::Windows,
            vendor: GpuVendor::Nvidia,
            render_quality: 1.0,
            shadow_quality: 1.0,
            hand_depth: 0.125,
            normal_map: true,
            specular_map: true,
            lab_pbr: true,
            anisotropic_filtering: 1,
            distant_horizons: false,
        }
    }
}

/// Iris features this translator supports (`IRIS_FEATURE_*`).
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
        d
    }
}

fn float(v: f32) -> String {
    let s = format!("{v:?}");
    if s.contains('.') || s.contains('e') {
        s
    } else {
        format!("{s}.0")
    }
}
