//! Host-owned declarative transactions and aggregate reservations.

use crate::{
    manifest::{Permission, Scope, identifier, plain_text},
    policy::*,
    screen,
    wire::{Channel, Direction, Scalar},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Fuel for one client part callback. Provisional: enough to decode a multi-fragment list record
/// (SP3's terminal sends ~22 KB); matches the server runtime's per-callback budget.
pub const CALLBACK_FUEL: u64 = 10_000_000;
/// Fuel for a player mod's `init` and `data-changed`, which copy the whole session through the
/// canonical ABI: one guest allocation per string and list, about 3,000 fuel per recipe and
/// 2,500 to 5,000 per item. Twice vanilla's session at the target (4,000 items, 3,000
/// three-by-three recipes) measured 27.7M in `examples/mods/screen-probe`'s `data-changed`
/// (mod-host `a_twice_vanilla_session_loads_but_an_event_may_not_spend_that_much`), leaving
/// room for a mod's own indexing. The precedent is the Experience runtime's `REGISTER_FUEL`
/// (`crates/experience-runtime/src/limits.rs`), also 100M for its one-off registration. A
/// session that outgrows this calls for a host-side query API (items and recipes on demand)
/// rather than a larger copy.
pub const LOAD_FUEL: u64 = 100_000_000;
pub const SESSION_FUEL: u64 = CALLBACK_FUEL * 2;
pub const CALLBACK_INTERVAL_MS: u64 = 34;
pub const MAX_WIDGETS: usize = 128;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct Principal {
    pub session: String,
    pub bundle: String,
    pub generation: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Widget {
        id: String,
        text: String,
    },
    /// Opens or switches the modal to an indexed template; `None` closes it.
    Screen {
        template: Option<String>,
    },
    /// Replaces the rows of one named collection the modal's templates read.
    Collection {
        name: String,
        rows: Vec<screen::Row>,
    },
    /// Binds one `#name` for the whole modal.
    Value {
        name: String,
        value: screen::Value,
    },
    /// Sets the text of the modal's edit boxes whose `text_box_name` is `control`.
    Text {
        control: String,
        text: String,
    },
    Send {
        channel: String,
        schema: u16,
        record: Vec<Scalar>,
    },
    Scene {
        id: u32,
        object: Option<SceneObject>,
    },
    Media {
        id: String,
        operation: MediaOperation,
        position_ms: u64,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaOperation {
    Prepare,
    Play,
    Pause,
    Seek,
    Stop,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SceneObject {
    Quad {
        texture: String,
        transform: [f32; 10],
        size: [f32; 2],
    },
    Mesh {
        asset: String,
        transform: [f32; 10],
        triangles: u32,
    },
    Particles {
        effect: String,
        transform: [f32; 10],
        count: u32,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Transaction {
    pub owner: Principal,
    pub epoch: u64,
    pub commands: Vec<Command>,
}

#[derive(Clone, Debug, Default)]
pub struct Contributions {
    pub widgets: BTreeMap<String, String>,
    pub modal: screen::Modal,
    pub scene: BTreeMap<u32, SceneObject>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub scope: Scope,
    pub assets: BTreeSet<String>,
    /// The manifest's `templates`: the only files the modal may open.
    pub templates: BTreeSet<String>,
    pub channels: Vec<Channel>,
    pub actions: BTreeSet<String>,
    /// The session's largest message, in bytes of record JSON: the negotiated wire's
    /// `max_message_bytes`. A send over it is refused when it is made, since the wire could not
    /// carry it.
    pub max_message_bytes: u32,
}

impl Capabilities {
    /// Whether a modal control may deliver `action` to this guest: declared in the manifest and
    /// granted `input`.
    pub fn may_deliver(&self, action: &str) -> bool {
        self.scope.permissions.contains(&Permission::Input) && self.actions.contains(action)
    }

    /// Validates a staged operation on both sides of the helper boundary.
    pub fn validate(&self, command: &Command) -> Result<()> {
        let permission = match command {
            Command::Widget { id, text } => {
                ensure!(
                    identifier(id) && (text.is_empty() || plain_text(text, MAX_WIDGET_TEXT_BYTES)),
                    "invalid widget"
                );
                Permission::Ui
            }
            Command::Screen { template } => {
                ensure!(
                    template
                        .as_ref()
                        .is_none_or(|id| self.templates.contains(id)),
                    "unknown screen"
                );
                Permission::ModalUi
            }
            Command::Collection { name, rows } => {
                ensure!(screen::collection_name(name), "invalid collection name");
                screen::validate_rows(rows)?;
                Permission::ModalUi
            }
            Command::Value { name, value } => {
                ensure!(screen::binding_name(name), "invalid binding name");
                value.validate()?;
                Permission::ModalUi
            }
            Command::Text { control, text } => {
                ensure!(identifier(control), "invalid edit box name");
                ensure!(screen::edit_text(text), "invalid edit box text");
                Permission::ModalUi
            }
            Command::Send {
                channel,
                schema,
                record,
            } => {
                let declaration = self
                    .channels
                    .iter()
                    .find(|c| &c.id == channel && c.schema == *schema)
                    .ok_or_else(|| anyhow::anyhow!("undeclared channel"))?;
                declaration.validate(
                    record,
                    Direction::ToServer,
                    self.max_message_bytes as usize,
                )?;
                Permission::Messaging
            }
            Command::Scene { object, .. } => {
                if let Some(object) = object {
                    let (asset, transform) = match object {
                        SceneObject::Quad {
                            texture,
                            transform,
                            size,
                        } => {
                            ensure!(
                                size.iter().all(|v| v.is_finite() && *v > 0.0 && *v <= 64.0),
                                "invalid quad size"
                            );
                            (texture, transform)
                        }
                        SceneObject::Mesh {
                            asset,
                            transform,
                            triangles,
                        } => {
                            ensure!(*triangles <= MAX_TRIANGLES, "mesh too large");
                            (asset, transform)
                        }
                        SceneObject::Particles {
                            effect,
                            transform,
                            count,
                        } => {
                            ensure!(*count <= MAX_PARTICLES, "particle budget exceeded");
                            (effect, transform)
                        }
                    };
                    ensure!(self.assets.contains(asset), "unowned scene asset");
                    ensure!(
                        transform.iter().all(|v| v.is_finite()),
                        "nonfinite transform"
                    );
                    ensure!(
                        transform[..3].iter().all(|v| v.abs() <= 30_000_000.0),
                        "translation out of range"
                    );
                    let rotation = transform[3..7].iter().map(|v| v * v).sum::<f32>();
                    ensure!(
                        (rotation - 1.0).abs() <= 0.001,
                        "quaternion must be normalized"
                    );
                    ensure!(
                        transform[7..].iter().all(|v| *v > 0.0 && *v <= 64.0),
                        "invalid scale"
                    );
                }
                Permission::Scene
            }
            Command::Media { id, .. } => {
                ensure!(self.assets.contains(id), "unowned media descriptor");
                Permission::Media
            }
        };
        ensure!(
            self.scope.permissions.contains(&permission),
            "capability denied"
        );
        Ok(())
    }
}

impl Contributions {
    /// Applies all retained changes privately; a rejection preserves the old state.
    pub fn apply(
        &mut self,
        transaction: &Transaction,
        owner: &Principal,
        epoch: u64,
        capabilities: &Capabilities,
    ) -> Result<()> {
        ensure!(
            &transaction.owner == owner && transaction.epoch == epoch,
            "stale or foreign handle"
        );
        ensure!(
            serde_json::to_vec(transaction)?.len() <= MAX_HOST_OUTPUT,
            "transaction too large"
        );
        let mut candidate = self.clone();
        for command in &transaction.commands {
            capabilities.validate(command)?;
            match command {
                Command::Widget { id, text } => {
                    if text.is_empty() {
                        candidate.widgets.remove(id);
                    } else {
                        candidate.widgets.insert(id.clone(), text.clone());
                    }
                }
                Command::Screen { template } => candidate.modal.open(template.clone()),
                Command::Collection { name, rows } => {
                    candidate.modal.set_collection(name.clone(), rows.clone());
                }
                Command::Value { name, value } => {
                    candidate.modal.set_value(name.clone(), value.clone());
                }
                Command::Text { control, text } => {
                    candidate.modal.set_text(control.clone(), text.clone());
                }
                Command::Scene { id, object } => {
                    if let Some(object) = object {
                        candidate.scene.insert(*id, object.clone());
                    } else {
                        candidate.scene.remove(id);
                    }
                }
                Command::Send { .. } | Command::Media { .. } => {}
            }
        }
        ensure!(
            candidate.widgets.len() <= MAX_WIDGETS,
            "widget budget exceeded"
        );
        candidate.modal.check()?;
        ensure!(
            candidate.scene.len() <= MAX_DRAWS as usize,
            "draw budget exceeded"
        );
        let mut triangles = 0u64;
        let mut particles = 0u64;
        for object in candidate.scene.values() {
            match object {
                SceneObject::Quad { .. } => triangles += 2,
                SceneObject::Mesh {
                    triangles: count, ..
                } => triangles += u64::from(*count),
                SceneObject::Particles { count, .. } => particles += u64::from(*count),
            }
        }
        ensure!(
            triangles <= u64::from(MAX_TRIANGLES) && particles <= u64::from(MAX_PARTICLES),
            "scene budget exceeded"
        );
        *self = candidate;
        Ok(())
    }
}

#[derive(Default)]
pub struct Budget {
    reservations: BTreeMap<Principal, (u64, u64)>,
    quarantined: BTreeSet<(String, String)>,
    fuel: u64,
}

impl Budget {
    /// Reserves worst-case guest and GPU memory before launching a helper.
    pub fn reserve(&mut self, owner: Principal, memory: u64, gpu: u64) -> Result<()> {
        ensure!(
            !self
                .quarantined
                .contains(&(owner.session.clone(), owner.bundle.clone()))
                && !self.reservations.contains_key(&owner),
            "instance unavailable"
        );
        ensure!(
            self.reservations.len() < MAX_BUNDLES && memory <= MAX_GUEST_MEMORY,
            "bundle limit exceeded"
        );
        let memory_total: u64 = self.reservations.values().map(|v| v.0).sum();
        let gpu_total: u64 = self.reservations.values().map(|v| v.1).sum();
        ensure!(
            memory <= MAX_SESSION_MEMORY - memory_total && gpu <= MAX_GPU_BYTES - gpu_total,
            "aggregate memory budget exceeded"
        );
        self.reservations.insert(owner, (memory, gpu));
        Ok(())
    }

    /// Starts one aggregate scheduling slice, independent of bundle count.
    pub fn begin_slice(&mut self) {
        self.fuel = SESSION_FUEL;
    }

    /// Reports whether a reserved guest can run without exceeding this slice.
    pub fn can_dispatch(&self, owner: &Principal) -> bool {
        self.reservations.contains_key(owner) && self.fuel >= CALLBACK_FUEL
    }

    /// Charges the full callback allowance before scheduling guest work.
    pub fn dispatch(&mut self, owner: &Principal) -> Result<u64> {
        ensure!(self.can_dispatch(owner), "callback deferred");
        self.fuel -= CALLBACK_FUEL;
        Ok(CALLBACK_FUEL)
    }

    /// A crashed bundle cannot be restarted until the whole session is replaced.
    pub fn quarantine(&mut self, owner: &Principal) {
        self.reservations.remove(owner);
        self.quarantined
            .insert((owner.session.clone(), owner.bundle.clone()));
    }
}
