//! A loaded shader pack and its configuration metadata.

use crate::dimensions::DimensionSelector;
use crate::environment::Environment;
use crate::ids::{BLOCK_FILE, ENTITY_FILE, ITEM_FILE, IdMaps};
use crate::options::{OptionValues, PackOption, discover_options, property_defines};
use crate::programs::{ProgramGroup, ProgramSource, enumerate_programs};
use crate::properties::ShaderProperties;
use crate::source::{PackError, PackSource};
use std::collections::BTreeMap;
use std::path::Path;

/// Extensions of files that hold GLSL option definitions.
const SHADER_EXTENSIONS: &[&str] = &["glsl", "vsh", "fsh", "gsh", "csh", "tcs", "tes", "inc", "h"];

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
}
