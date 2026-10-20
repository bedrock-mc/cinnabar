//! A loaded shader pack, its configuration metadata and whole-pack translation.

use crate::buffers::{BufferDirectives, BufferFormat, canonical_sampler_name};
use crate::dimensions::DimensionSelector;
use crate::environment::Environment;
use crate::ids::{BLOCK_FILE, ENTITY_FILE, ITEM_FILE, IdMaps};
use crate::options::{OptionValues, PackOption, discover_options, property_defines};
use crate::programs::{ProgramGroup, ProgramSource, enumerate_programs};
use crate::properties::ShaderProperties;
use crate::reflect::output_buffer;
use crate::reflection::SamplerClass;
use crate::source::{PackError, PackSource};
use crate::transform::SamplerClasses;
use crate::translate::{Context, ProgramTranslation, forcing, translate_program};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

/// Extensions of files that hold GLSL option definitions.
const SHADER_EXTENSIONS: &[&str] = &["glsl", "vsh", "fsh", "gsh", "csh", "tcs", "tes", "inc", "h"];

/// What to translate and with which settings.
#[derive(Clone, Debug)]
pub struct TranslateConfig {
    /// Program folder, e.g. `world0`; see [`ShaderPack::dimension_folder`]. A folder the pack
    /// lacks selects its base program set, as in [`ShaderPack::programs`].
    pub folder: String,
    pub environment: Environment,
    /// Option values chosen by the player; unset options keep the pack's defaults.
    pub options: OptionValues,
    /// Also translate programs the pack disables under `options`, switching on the options
    /// (or defining the macros) their enable rule needs; programs whose rule no such change
    /// satisfies (such as `= false`) are translated with `options` as they are. For tooling
    /// and coverage runs.
    pub force_disabled: bool,
}

impl Default for TranslateConfig {
    fn default() -> TranslateConfig {
        TranslateConfig {
            folder: "world0".to_string(),
            environment: Environment::default(),
            options: OptionValues::new(),
            force_disabled: false,
        }
    }
}

/// A whole-pack translation result.
#[derive(Clone, Debug)]
pub struct PackTranslation {
    /// `shaders.properties` evaluated with the configured options.
    pub properties: ShaderProperties,
    /// Buffer settings from the `const` directives of every enabled program.
    pub buffers: BufferDirectives,
    /// Every program of the selected folder, in frame order.
    pub programs: Vec<ProgramTranslation>,
}

/// A shader pack opened from a folder or zip, with its options discovered.
#[derive(Clone, Debug)]
pub struct ShaderPack {
    source: PackSource,
    options: Vec<PackOption>,
}

impl ShaderPack {
    /// Opens a pack folder or zip; see [`PackSource::open`].
    pub fn open(path: &Path) -> Result<ShaderPack, PackError> {
        Ok(ShaderPack::new(PackSource::open(path)?))
    }

    /// Wraps loaded files and discovers the pack's options.
    pub fn new(source: PackSource) -> ShaderPack {
        let options = discover_options(source.files().filter(|(path, _)| {
            path.rsplit_once('.')
                .is_some_and(|(_, ext)| SHADER_EXTENSIONS.contains(&ext))
        }));
        ShaderPack { source, options }
    }

    /// Borrows the loaded pack files and their origin.
    pub fn source(&self) -> &PackSource {
        &self.source
    }

    /// Options sorted by name.
    pub fn options(&self) -> &[PackOption] {
        &self.options
    }

    /// The folder used for a dimension id such as `minecraft:overworld`; `dimension.properties`
    /// conditions see the environment's macros.
    pub fn dimension_folder(&self, dimension: &str, environment: &Environment) -> String {
        self.dimensions(environment).folder(dimension)
    }

    /// Resolves `dimension.properties` under the environment's macros, as Iris does.
    fn dimensions(&self, environment: &Environment) -> DimensionSelector {
        let defines = environment.standard_defines().into_iter().collect();
        DimensionSelector::new(&self.source, &defines)
    }

    /// Macros visible to `shaders.properties` and enable rules: standard macros and options.
    pub fn defines(
        &self,
        environment: &Environment,
        options: &OptionValues,
    ) -> BTreeMap<String, String> {
        let mut d: BTreeMap<String, String> = environment.standard_defines().into_iter().collect();
        d.extend(property_defines(&self.options, options));
        d
    }

    /// `shaders.properties` with its conditionals evaluated (empty if the pack has none).
    pub fn properties(
        &self,
        environment: &Environment,
        options: &OptionValues,
    ) -> ShaderProperties {
        let text = self.source.text("shaders.properties").unwrap_or("");
        ShaderProperties::parse(text, &self.defines(environment, options))
    }

    /// `block.properties`, `item.properties` and `entity.properties` with their conditionals
    /// evaluated like `shaders.properties`; see [`IdMaps::parse`] (a pack without
    /// block.properties gets Iris's legacy block ids).
    pub fn id_maps(&self, environment: &Environment, options: &OptionValues) -> IdMaps {
        IdMaps::parse(
            self.source.text(BLOCK_FILE),
            self.source.text(ITEM_FILE),
            self.source.text(ENTITY_FILE),
            &self.defines(environment, options),
        )
    }

    /// Discovers one complete program directory, falling back to the base set when absent.
    /// An empty folder selects the root set; `dh_*` programs require Distant Horizons.
    /// Enable rules are available separately through [`ShaderProperties::program_rule`].
    pub fn programs(&self, folder: &str, environment: &Environment) -> Vec<ProgramSource> {
        let dimensions = self.dimensions(environment);
        enumerate_programs(&self.source, dimensions.program_folder(folder))
            .into_iter()
            .filter(|p| environment.distant_horizons || p.group != ProgramGroup::DistantHorizons)
            .collect()
    }

    /// Translates every program of the configured folder, in parallel. Failures are per
    /// program; buffer formats in each reflection come from the whole pack's directives.
    pub fn translate(&self, config: &TranslateConfig) -> PackTranslation {
        let properties = self.properties(&config.environment, &config.options);
        let programs = self.programs(&config.folder, &config.environment);
        let mut programs: Vec<ProgramTranslation> = programs
            .par_iter()
            .map(|program| self.translate_one(program, config, &properties))
            .collect();
        let mut buffers = BufferDirectives::default();
        for c in programs
            .iter()
            .filter(|p| p.enabled)
            .flat_map(|p| &p.consts)
        {
            buffers.accept(c);
        }
        for p in &mut programs {
            let shadow =
                matches!(p.program.group, ProgramGroup::Shadow) || p.program.name == "dh_shadow";
            if let Some(r) = &mut p.reflection {
                for o in &mut r.color_outputs {
                    o.format = o.buffer.map_or(BufferFormat::DEFAULT, |b| {
                        buffers.format(output_buffer(b, shadow))
                    });
                }
            }
        }
        PackTranslation {
            properties,
            buffers,
            programs,
        }
    }

    /// Translates one program, honouring its enable rule from the folder it came from.
    fn translate_one(
        &self,
        program: &ProgramSource,
        config: &TranslateConfig,
        properties: &ShaderProperties,
    ) -> ProgramTranslation {
        let defines = self.defines(&config.environment, &config.options);
        let rule = properties.program_rule(&program.folder, &program.name);
        let enabled = rule.is_none_or(|r| r.is_enabled(&defines));
        let mut values = config.options.clone();
        let mut pp_defines = config.environment.standard_defines();
        let mut forced = vec![];
        let mut own_properties = None;
        if !enabled {
            if !config.force_disabled {
                return ProgramTranslation {
                    program: program.clone(),
                    enabled,
                    forced,
                    stages: vec![],
                    diagnostics: vec![],
                    reflection: None,
                    draw_buffers: vec![0],
                    consts: vec![],
                };
            }
            let flips = rule
                .and_then(|r| forcing(r, &self.options, &defines))
                .unwrap_or_default();
            for (name, value) in &flips {
                if value.is_empty() {
                    pp_defines.push((name.clone(), String::new()));
                } else {
                    values.insert(name.clone(), value.clone());
                }
            }
            let mut forced_defines = self.defines(&config.environment, &values);
            forced_defines.extend(pp_defines.iter().filter(|(_, v)| v.is_empty()).cloned());
            let text = self.source.text("shaders.properties").unwrap_or("");
            own_properties = Some(ShaderProperties::parse(text, &forced_defines));
            forced = flips;
        }
        let properties = own_properties.as_ref().unwrap_or(properties);
        let image_formats: HashMap<String, String> = properties
            .images
            .iter()
            .map(|i| {
                let q = BufferFormat::parse(&i.internal_format)
                    .and_then(BufferFormat::glsl_qualifier)
                    .unwrap_or_else(|| i.internal_format.to_ascii_lowercase());
                (i.name.clone(), q)
            })
            .collect();
        // Custom images clamp and filter linearly; custom textures follow their `.mcmeta`
        // for samplers of their shape, a stage's own binding winning over `texture.noise`.
        let mut sampler_classes: SamplerClasses = properties
            .images
            .iter()
            .map(|i| {
                (
                    (canonical_sampler_name(&i.sampler), None),
                    SamplerClass::ClampLinear,
                )
            })
            .collect();
        for noise in [true, false] {
            for t in &properties.custom_textures {
                if t.applies_to(program.group) && (t.stage == "noise") == noise {
                    sampler_classes.insert(
                        (
                            canonical_sampler_name(t.uniform()),
                            Some(t.source.dimension()),
                        ),
                        t.sampler_class(&self.source),
                    );
                }
            }
        }
        let cx = Context {
            source: &self.source,
            options: &self.options,
            values: &values,
            defines: &pp_defines,
            image_formats: &image_formats,
            sampler_classes: &sampler_classes,
        };
        translate_program(&cx, program, enabled, forced)
    }
}
