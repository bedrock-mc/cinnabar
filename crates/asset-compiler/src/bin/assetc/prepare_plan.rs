//! Which carriers a prepare run rebuilds. A carrier's fingerprint hashes the compiler source, the
//! tracked inputs it declares in the carrier table and the fingerprints of carriers it reads; the
//! stamp beside the carriers records what each was built from.

use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    error::Error,
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use assets::{
    VanillaSource,
    carriers::{self, CARRIERS, Carrier, Input, Recipe, STAMP_FILE, Sources, VANILLA_MANIFEST},
    vanilla_pack::PackPaths,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA: u32 = 2;

/// Where one run reads its inputs and writes its carriers.
pub(super) struct Context {
    pub sources: Sources,
    pub workspace: PathBuf,
    pub pack: PackPaths,
    pub out: PathBuf,
    pub clouds_override: Option<PathBuf>,
}

impl Context {
    pub(super) fn new(
        sources: Sources,
        workspace: PathBuf,
        out: PathBuf,
        clouds_override: Option<PathBuf>,
    ) -> Result<Self, Box<dyn Error>> {
        let source = VanillaSource::read(&sources.resolve(VANILLA_MANIFEST))?;
        let pack = source.local_paths(&workspace)?;
        Ok(Self {
            sources,
            workspace,
            pack,
            out,
            clouds_override,
        })
    }

    pub(super) fn resource_pack(&self) -> PathBuf {
        self.pack.cache.join("resource_pack")
    }

    /// The behavior pack, when it ships the item definitions equipment reads.
    pub(super) fn behavior_pack(&self) -> Option<PathBuf> {
        let dir = self.pack.cache.join("behavior_pack");
        dir.join("items").is_dir().then_some(dir)
    }

    /// The source manifest `carrier` declares.
    pub(super) fn manifest(&self, carrier: &Carrier) -> PathBuf {
        let path = carrier
            .inputs
            .iter()
            .find_map(|input| match input {
                Input::Manifest(path) => Some(*path),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{} declares no source manifest", carrier.name));
        self.sources.resolve(path)
    }

    /// The other tracked files `carrier` declares, in table order.
    pub(super) fn files(&self, carrier: &Carrier) -> Vec<PathBuf> {
        carrier
            .inputs
            .iter()
            .filter_map(|input| match input {
                Input::File(path) => Some(self.sources.resolve(path)),
                _ => None,
            })
            .collect()
    }

    /// The font file a font manifest names.
    pub(super) fn font_file(&self, manifest: &str) -> Result<PathBuf, Box<dyn Error>> {
        let path = self.sources.resolve(manifest);
        let value: serde_json::Value = serde_json::from_slice(&read(&path)?)?;
        let name = value["font_file"]
            .as_str()
            .ok_or_else(|| format!("{} names no font_file", path.display()))?;
        Ok(self.sources.resolve(&format!("assets/fonts/{name}")))
    }
}

/// Which carriers a run may build.
pub(super) struct Scope<'a> {
    pub installed_only: bool,
    /// Carrier names; empty means every carrier in scope.
    pub only: &'a [String],
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub(super) struct Stamp {
    schema: u32,
    pub carriers: BTreeMap<String, Entry>,
    /// Input digests keyed by path, reused while a file's size and mtime are unchanged.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, FileDigest>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(super) struct FileDigest {
    size: u64,
    modified_ns: u64,
    sha256: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(super) struct Entry {
    pub fingerprint: String,
    /// An optional carrier whose build failed; retried once its fingerprint changes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub failed: bool,
}

/// The stamp in `dir`; empty when absent, unreadable or from another schema.
pub(super) fn read_stamp(dir: &Path) -> Stamp {
    fs::read(dir.join(STAMP_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Stamp>(&bytes).ok())
        .filter(|stamp| stamp.schema == SCHEMA)
        .unwrap_or_default()
}

pub(super) fn write_stamp(dir: &Path, stamp: &Stamp) -> Result<(), Box<dyn Error>> {
    let stamp = Stamp {
        schema: SCHEMA,
        carriers: stamp.carriers.clone(),
        inputs: stamp.inputs.clone(),
    };
    assets::write_blob_atomic(&dir.join(STAMP_FILE), &serde_json::to_vec_pretty(&stamp)?)?;
    Ok(())
}

pub(super) struct Plan {
    /// Every carrier in scope, in table order.
    pub selected: Vec<&'static Carrier>,
    pub stale: Vec<&'static Carrier>,
    pub fingerprints: HashMap<&'static str, String>,
    /// Digests of every input this plan read, for the next run's stamp.
    pub inputs: BTreeMap<String, FileDigest>,
}

impl Plan {
    pub(super) fn needs_pack(&self) -> bool {
        self.stale
            .iter()
            .any(|carrier| carrier.inputs.contains(&Input::Pack))
    }
}

pub(super) fn plan(
    context: &Context,
    compiler: &str,
    scope: &Scope,
    stamp: &Stamp,
) -> Result<Plan, Box<dyn Error>> {
    let selected = select(scope)?;
    let mut fingerprints = HashMap::new();
    let digests = Digests {
        previous: &stamp.inputs,
        current: RefCell::default(),
    };
    for carrier in &selected {
        let print = fingerprint(carrier, context, compiler, &digests, &fingerprints)?;
        fingerprints.insert(carrier.name, print);
    }
    let stale = selected
        .iter()
        .copied()
        .filter(|carrier| match stamp.carriers.get(carrier.name) {
            Some(entry) if entry.fingerprint == fingerprints[carrier.name] => {
                !entry.failed && !carrier.outputs(&context.out).all(|path| path.exists())
            }
            _ => true,
        })
        .collect();
    Ok(Plan {
        selected,
        stale,
        fingerprints,
        inputs: digests.current.into_inner(),
    })
}

/// The carriers in scope plus everything they read, in table order.
fn select(scope: &Scope) -> Result<Vec<&'static Carrier>, Box<dyn Error>> {
    let in_scope = |carrier: &Carrier| carrier.installed || !scope.installed_only;
    let mut wanted: Vec<&'static str> = Vec::new();
    if scope.only.is_empty() {
        wanted.extend(CARRIERS.iter().filter(|c| in_scope(c)).map(|c| c.name));
    }
    for name in scope.only {
        let carrier = carriers::by_name(name).ok_or_else(|| {
            let names: Vec<_> = CARRIERS.iter().map(|carrier| carrier.name).collect();
            format!(
                "unknown carrier '{name}'; expected one of {}",
                names.join(", ")
            )
        })?;
        wanted.push(carrier.name);
    }
    // Reads point at earlier entries, so one reverse pass closes over them.
    for carrier in CARRIERS.iter().rev() {
        if wanted.contains(&carrier.name) {
            wanted.extend(carrier.reads);
        }
    }
    Ok(CARRIERS
        .iter()
        .filter(|carrier| wanted.contains(&carrier.name) && in_scope(carrier))
        .collect())
}

fn fingerprint(
    carrier: &Carrier,
    context: &Context,
    compiler: &str,
    digests: &Digests,
    earlier: &HashMap<&'static str, String>,
) -> Result<String, Box<dyn Error>> {
    let mut hasher = Sha256::new();
    let mut field = |bytes: &[u8]| {
        hasher.update(bytes);
        hasher.update([0]);
    };
    field(compiler.as_bytes());
    field(carrier.name.as_bytes());
    for name in carrier.outputs(Path::new("")) {
        field(name.to_string_lossy().as_bytes());
    }
    for input in carrier.inputs {
        match input {
            // The pin identifies both packs; the extracted cache is transient and may be gone.
            Input::Pack | Input::BehaviorPack => {
                let manifest = read(&context.sources.resolve(VANILLA_MANIFEST))?;
                field(if *input == Input::Pack {
                    b"resource-pack"
                } else {
                    b"behavior-pack"
                });
                field(&assets::canonical_source_manifest_sha256(&manifest));
            }
            Input::Manifest(path) | Input::File(path) => {
                field(path.as_bytes());
                field(digests.sha256(&context.sources.resolve(path))?.as_bytes());
            }
            Input::FontFile(manifest) => {
                field(digests.sha256(&context.font_file(manifest)?)?.as_bytes());
            }
        }
    }
    if carrier.recipe == Recipe::Atmosphere
        && let Some(clouds) = &context.clouds_override
    {
        field(b"clouds-override");
        field(digests.sha256(clouds)?.as_bytes());
    }
    for read in carrier.reads {
        let dependency = earlier
            .get(read)
            .ok_or_else(|| format!("{} reads a carrier outside this run", carrier.name))?;
        field(dependency.as_bytes());
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// File digests that skip rehashing a file whose size and mtime match the stamp.
struct Digests<'a> {
    previous: &'a BTreeMap<String, FileDigest>,
    current: RefCell<BTreeMap<String, FileDigest>>,
}

impl Digests<'_> {
    fn sha256(&self, path: &Path) -> Result<String, Box<dyn Error>> {
        let key = path.to_string_lossy().into_owned();
        if let Some(known) = self.current.borrow().get(&key) {
            return Ok(known.sha256.clone());
        }
        let metadata =
            fs::metadata(path).map_err(|error| format!("read {}: {error}", path.display()))?;
        let modified_ns = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |since| u64::try_from(since.as_nanos()).unwrap_or(0));
        let sha256 = match self.previous.get(&key).filter(|previous| {
            modified_ns != 0
                && (previous.size, previous.modified_ns) == (metadata.len(), modified_ns)
        }) {
            Some(previous) => previous.sha256.clone(),
            None => format!("{:x}", Sha256::digest(read(path)?)),
        };
        self.current.borrow_mut().insert(
            key,
            FileDigest {
                size: metadata.len(),
                modified_ns,
                sha256: sha256.clone(),
            },
        );
        Ok(sha256)
    }
}

fn read(path: &Path) -> Result<Vec<u8>, Box<dyn Error>> {
    fs::read(path).map_err(|error| format!("read {}: {error}", path.display()).into())
}
