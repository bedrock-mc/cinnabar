//! `assetc prepare`: builds every stale carrier in the carrier table in one process, running
//! independent recipes in parallel and each recipe only after the carriers it reads.

use std::{
    collections::HashMap,
    error::Error,
    fs,
    panic::{self, AssertUnwindSafe},
    path::PathBuf,
    sync::{Condvar, Mutex},
    thread,
    time::{Duration, Instant},
};

use assets::carriers::{self, COMPILED_DIR, Carrier, Input, Recipe, Sources, VANILLA_MANIFEST};
use serde_json::json;

use super::{
    Command,
    prepare_plan::{Context, Entry, Plan, Scope, Stamp, plan, read_stamp, write_stamp},
};

/// Digest of the compiler's own sources, from the build script.
pub(super) const COMPILER: &str = env!("ASSETC_SOURCE_SHA256");

pub(super) struct Options {
    pub root: PathBuf,
    pub kit: Option<PathBuf>,
    pub workspace: Option<PathBuf>,
    pub out: Option<PathBuf>,
    pub only: Vec<String>,
    pub check: bool,
    pub json: bool,
    pub accept_eula: bool,
    pub clouds_override: Option<PathBuf>,
}

pub(super) fn prepare(options: Options) -> Result<(), Box<dyn Error>> {
    let workspace = options
        .workspace
        .clone()
        .unwrap_or_else(|| options.root.clone());
    let sources = match &options.kit {
        Some(kit) => Sources::Kit(kit.clone()),
        None => Sources::Checkout(options.root.clone()),
    };
    let out = options
        .out
        .clone()
        .unwrap_or_else(|| workspace.join(COMPILED_DIR));
    let context = Context::new(sources, workspace, out, options.clouds_override.clone())?;
    let scope = Scope {
        installed_only: options.kit.is_some(),
        only: &options.only,
    };
    let mut stamp = read_stamp(&context.out);
    let plan = plan(&context, COMPILER, &scope, &stamp)?;
    let report = Reporter { json: options.json };
    if options.check {
        let stale: Vec<_> = plan.stale.iter().map(|carrier| carrier.name).collect();
        println!(
            "{}",
            json!({"current": stale.is_empty(), "stale": stale, "needs_pack": plan.needs_pack()})
        );
        return Ok(());
    }
    let started = Instant::now();
    report.plan(&plan);
    let refreshed = stamp.inputs != plan.inputs;
    stamp.inputs.clone_from(&plan.inputs);
    if plan.stale.is_empty() {
        if refreshed && context.out.is_dir() {
            write_stamp(&context.out, &stamp)?;
        }
        report.summary(&plan, started.elapsed());
        return Ok(());
    }
    if plan.needs_pack() && !context.pack.is_unpacked() {
        if !options.accept_eula {
            return Err(format!(
                "the pinned sample pack is missing at {}; fetch it with `make vanilla-assets`",
                context.pack.cache.display()
            )
            .into());
        }
        let manifest = context.sources.resolve(VANILLA_MANIFEST);
        super::vanilla_pack_command::acquire(&manifest, &context.workspace, true)?;
    }
    fs::create_dir_all(&context.out)?;
    for carrier in &plan.stale {
        stamp.carriers.remove(carrier.name);
        clear_outputs(carrier, &context);
    }
    write_stamp(&context.out, &stamp)?;
    let result = execute(&context, &plan, stamp, &report);
    report.summary(&plan, started.elapsed());
    result.map_err(Into::into)
}

/// Removes a carrier's earlier outputs so a failed optional rebuild leaves nothing stale behind.
fn clear_outputs(carrier: &Carrier, context: &Context) {
    for path in carrier.outputs(&context.out) {
        let _ = if path.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
    }
}

struct Queue {
    /// Indices into the stale list, in table order.
    pending: Vec<usize>,
    /// Finished recipes and whether they succeeded.
    finished: HashMap<Recipe, bool>,
    /// The first required failure; nothing new starts once it is set.
    abort: Option<String>,
}

/// Builds the stale carriers on a worker per core, recording each outcome in the stamp as it lands.
fn execute(context: &Context, plan: &Plan, stamp: Stamp, report: &Reporter) -> Result<(), String> {
    let jobs = &plan.stale;
    let queue = Mutex::new(Queue {
        pending: (0..jobs.len()).collect(),
        finished: HashMap::new(),
        abort: None,
    });
    let changed = Condvar::new();
    let stamp = Mutex::new(stamp);
    let workers = thread::available_parallelism()
        .map_or(4, usize::from)
        .min(jobs.len());
    thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some((index, blocked)) = next_job(&queue, &changed, jobs) {
                    let carrier = jobs[index];
                    report.start(carrier);
                    let started = Instant::now();
                    let result = match blocked {
                        Some(dependency) => Err(format!("needs {dependency}, which failed")),
                        None => build(carrier, context),
                    };
                    let recorded = record(context, &stamp, plan, carrier, &result);
                    report.finish(carrier, &result, started.elapsed());
                    let mut queue = queue.lock().unwrap();
                    queue.finished.insert(carrier.recipe, result.is_ok());
                    let failure = result.err().filter(|_| carrier.required).or(recorded.err());
                    if let Some(error) = failure {
                        queue
                            .abort
                            .get_or_insert_with(|| format!("{}: {error}", carrier.label));
                    }
                    changed.notify_all();
                }
            });
        }
    });
    queue.into_inner().unwrap().abort.map_or(Ok(()), Err)
}

/// Blocks until a job is ready, or returns `None` once nothing is left to start.
fn next_job(
    queue: &Mutex<Queue>,
    changed: &Condvar,
    jobs: &[&'static Carrier],
) -> Option<(usize, Option<&'static str>)> {
    let mut queue = queue.lock().unwrap();
    loop {
        if queue.abort.is_some() {
            queue.pending.clear();
        }
        if queue.pending.is_empty() {
            return None;
        }
        if let Some(job) = take_ready(&mut queue, jobs) {
            return Some(job);
        }
        queue = changed.wait(queue).unwrap();
    }
}

/// The first pending job whose planned dependencies have finished, and a dependency that failed.
fn take_ready(
    queue: &mut Queue,
    jobs: &[&'static Carrier],
) -> Option<(usize, Option<&'static str>)> {
    let planned = |recipe: Recipe| jobs.iter().any(|carrier| carrier.recipe == recipe);
    let position = queue.pending.iter().position(|&index| {
        jobs[index]
            .reads
            .iter()
            .all(|&read| !planned(read) || queue.finished.contains_key(&read))
    })?;
    let index = queue.pending.remove(position);
    let blocked = jobs[index]
        .reads
        .iter()
        .find(|read| queue.finished.get(read) == Some(&false))
        .map(|&read| carriers::by_recipe(read).name);
    Some((index, blocked))
}

/// Stamps a success, or an optional failure so it is not retried until its inputs change.
fn record(
    context: &Context,
    stamp: &Mutex<Stamp>,
    plan: &Plan,
    carrier: &Carrier,
    result: &Result<(), String>,
) -> Result<(), String> {
    if result.is_err() {
        clear_outputs(carrier, context);
        if carrier.required {
            return Ok(());
        }
    }
    let mut stamp = stamp.lock().unwrap();
    stamp.carriers.insert(
        carrier.name.to_owned(),
        Entry {
            fingerprint: plan.fingerprints[&carrier.recipe].clone(),
            failed: result.is_err(),
        },
    );
    write_stamp(&context.out, &stamp).map_err(|error| format!("write the stamp: {error}"))
}

fn build(carrier: &Carrier, context: &Context) -> Result<(), String> {
    let command = command(carrier, context).map_err(|error| error.to_string())?;
    panic::catch_unwind(AssertUnwindSafe(|| {
        super::run(command).map_err(|error| error.to_string())
    }))
    .unwrap_or_else(|payload| {
        let message = payload
            .downcast_ref::<&str>()
            .map(|text| (*text).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        Err(format!("the compiler panicked: {message}"))
    })
}

/// The single-carrier command a recipe runs, with every path taken from the carrier table.
pub(super) fn command(carrier: &Carrier, context: &Context) -> Result<Command, Box<dyn Error>> {
    let pack = context.resource_pack();
    let out = context.out.join(carrier.output);
    let report = || {
        context
            .out
            .join(carrier.report.expect("recipe writes a report"))
    };
    let manifest = || context.manifest(carrier);
    let read = |recipe| context.out.join(carriers::by_recipe(recipe).output);
    Ok(match carrier.recipe {
        Recipe::World => {
            let [registry, light_registry, biome_registry] =
                <[PathBuf; 3]>::try_from(context.files(carrier))
                    .map_err(|_| "the world recipe takes three registries")?;
            Command::Compile {
                pack,
                source_manifest: manifest(),
                registry,
                light_registry,
                biome_registry,
                out,
            }
        }
        Recipe::Atmosphere => Command::Atmosphere {
            pack,
            source_manifest: manifest(),
            clouds_override: context.clouds_override.clone(),
            out,
            report: report(),
        },
        Recipe::Entity => Command::EntityAssets {
            pack,
            source_manifest: manifest(),
            out,
            report: report(),
        },
        Recipe::Font => {
            let font = carrier.inputs.iter().find_map(|input| match input {
                Input::FontFile(manifest) => Some(*manifest),
                _ => None,
            });
            Command::FontAssets {
                pack: None,
                font: Some(context.font_file(font.ok_or("the font recipe names no font")?)?),
                source_manifest: manifest(),
                out,
                report: report(),
            }
        }
        Recipe::Hud => Command::HudAssets {
            pack,
            source_manifest: manifest(),
            out,
            report: report(),
        },
        Recipe::Lang => Command::LangAssets {
            pack,
            source_manifest: manifest(),
            out,
            report: report(),
        },
        Recipe::Languages => Command::LanguageAssets {
            pack,
            source_manifest: manifest(),
            out_dir: out,
        },
        Recipe::Icon => Command::IconAssets {
            pack,
            source_manifest: manifest(),
            block_assets: Some(read(Recipe::World)),
            out,
            report: report(),
        },
        Recipe::Audio => Command::AudioAssets {
            pack,
            source_manifest: manifest(),
            out,
            report: report(),
        },
        Recipe::Actor => Command::ActorAssets {
            pack,
            source_manifest: manifest(),
            out,
            report: report(),
        },
        Recipe::AudioBank => Command::AudioBank {
            pack,
            out,
            report: report(),
        },
        Recipe::Equipment => Command::EquipmentAssets {
            pack,
            source_manifest: manifest(),
            out,
            report: report(),
            behavior_pack: context.behavior_pack(),
        },
        Recipe::Ui => Command::UiAssets {
            pack,
            source_manifest: manifest(),
            out,
            report: report(),
        },
        Recipe::Particle => Command::ParticleAssets {
            pack,
            source_manifest: manifest(),
            out,
            report: report(),
        },
        Recipe::BlockEntity => Command::BlockEntityAssets {
            pack,
            source_manifest: manifest(),
            out,
            report: report(),
        },
        Recipe::Weather => Command::WeatherAssets { pack, out },
        Recipe::HudExtras => Command::HudExtrasAssets { pack, out },
        Recipe::StarterSkins => Command::StarterSkinAssets { pack, out },
        Recipe::AudioPcm => Command::AudioPcmAssets {
            pack,
            catalog: read(Recipe::Audio),
            source_manifest: manifest(),
            out,
            report: report(),
        },
    })
}

/// Progress for people, or JSON lines for first-run setup.
struct Reporter {
    json: bool,
}

impl Reporter {
    fn plan(&self, plan: &Plan) {
        let stale: Vec<_> = plan.stale.iter().map(|carrier| carrier.name).collect();
        if self.json {
            println!("{}", json!({"event": "plan", "stale": stale}));
        } else if !stale.is_empty() {
            println!("prepare: rebuilding {}", stale.join(", "));
        }
    }

    fn start(&self, carrier: &Carrier) {
        if self.json {
            println!(
                "{}",
                json!({"event": "start", "name": carrier.name, "label": carrier.label})
            );
        }
    }

    fn finish(&self, carrier: &Carrier, result: &Result<(), String>, took: Duration) {
        match (result, self.json) {
            (Ok(()), true) => println!(
                "{}",
                json!({"event": "done", "name": carrier.name, "seconds": took.as_secs_f64()})
            ),
            (Ok(()), false) => println!("prepare: {} built in {:.2?}", carrier.name, took),
            (Err(error), true) => println!(
                "{}",
                json!({
                    "event": "failed",
                    "name": carrier.name,
                    "label": carrier.label,
                    "required": carrier.required,
                    "error": error,
                })
            ),
            (Err(error), false) => eprintln!(
                "prepare: {} failed{}: {error}",
                carrier.name,
                if carrier.required {
                    ""
                } else {
                    " (optional, skipped)"
                }
            ),
        }
    }

    fn summary(&self, plan: &Plan, took: Duration) {
        if !self.json {
            println!(
                "prepare: {} rebuilt, {} current in {:.2?}",
                plan.stale.len(),
                plan.selected.len() - plan.stale.len(),
                took
            );
        }
    }
}

#[cfg(test)]
#[path = "prepare_tests.rs"]
mod tests;
