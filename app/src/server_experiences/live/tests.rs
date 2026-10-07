use super::*;
use server_experience::{
    manifest::{Offer, PackageOffer, Permission, Scope},
    negotiation::VerifiedOffer,
    runtime::Transaction,
    wire::{Channel, Direction, Field, Scalar},
};

struct FakeWorker {
    response: Option<Transaction>,
    dispatched: Vec<Vec<u8>>,
    channels: Vec<String>,
    owner: Principal,
    epoch: u64,
}

impl Worker for FakeWorker {
    /// Leaves initialization pending until the test supplies its completion.
    fn spawn(_: &Path, _: Vec<u8>, owner: Principal, _: Capabilities, epoch: u64) -> Result<Self> {
        Ok(Self {
            response: None,
            dispatched: Vec::new(),
            channels: Vec::new(),
            owner,
            epoch,
        })
    }
    /// Delivers only explicitly completed transactions.
    fn poll(&mut self) -> Option<Result<Transaction>> {
        self.response.take().map(Ok)
    }
    /// Records delivered payloads and completes them on the next poll.
    fn dispatch(&mut self, request: Dispatch) -> Result<()> {
        self.dispatched.push(request.record);
        self.channels.push(request.channel);
        self.response = Some(Transaction {
            owner: self.owner.clone(),
            epoch: self.epoch,
            commands: Vec::new(),
        });
        Ok(())
    }
}

/// Reserves real aggregate budgets for pending component helpers without starting processes.
fn fixture(count: usize) -> Live<FakeWorker> {
    let scope = Scope {
        permissions: BTreeSet::from([Permission::Messaging, Permission::Ui]),
        origins: BTreeSet::new(),
        memory_bytes: 0,
        gpu_bytes: 0,
    };
    let packages = (0..count)
        .map(|i| PackageOffer {
            id: format!("bundle{i}"),
            publisher_key: String::new(),
            digest: format!("digest{i}"),
            bytes: 1,
            url: String::new(),
        })
        .collect::<Vec<_>>();
    let grant = Grant {
        offer: VerifiedOffer {
            offer: Offer {
                version: WIRE_VERSION,
                audience: String::new(),
                server_key: String::new(),
                revision: 1,
                expires_unix: u64::MAX,
                scope: scope.clone(),
                packages,
                fallback: String::new(),
                carrier: protocol::EXPERIENCE_CHANNEL.into(),
            },
            digest: String::new(),
        },
        session: "session".into(),
        connection: "connection".into(),
        subclient: 0,
        expires_unix: u64::MAX,
    };
    let mut budget = Budget::default();
    budget.begin_slice();
    let instances = grant
        .offer
        .offer
        .packages
        .iter()
        .map(|package| {
            let owner = Principal {
                session: grant.session.clone(),
                bundle: package.id.clone(),
                generation: INITIAL_BUNDLE_GENERATION,
            };
            budget.reserve(owner.clone(), 0, 0).unwrap();
            let channels = [Direction::ToClient, Direction::ToServer]
                .into_iter()
                .enumerate()
                .map(|(i, direction)| Channel {
                    id: format!("{}.events{i}", owner.bundle),
                    schema: API_VERSION,
                    direction,
                    fields: vec![Field::Bool],
                })
                .collect();
            (
                owner.bundle.clone(),
                Instance {
                    helper: None,
                    component: Some(vec![0]),
                    owner,
                    capabilities: Capabilities {
                        scope: scope.clone(),
                        assets: BTreeSet::new(),
                        channels,
                        actions: BTreeSet::new(),
                    },
                    contributions: Contributions::default(),
                    busy: true,
                },
            )
        })
        .collect();
    let mut live = Live {
        media: super::super::media::Media::new(grant.clone(), 1, PathBuf::new()),
        screens: Vec::new(),
        screens_built: None,
        scene_revision: 0,
        grant,
        instances,
        executable: PathBuf::new(),
        pending_sends: VecDeque::new(),
        pending_send_bytes: 0,
        budget,
        ingress: Ingress::new(0),
        egress: RateLimit::new(0),
        sequence: 1,
        slice_ms: 0,
        ready: false,
        epoch: 1,
    };
    live.initialize().unwrap();
    live
}

/// Completes an initializer with a valid outbound channel command.
fn complete(live: &mut Live<FakeWorker>, id: &str) {
    let instance = live.instances.get_mut(id).unwrap();
    instance.helper.as_mut().unwrap().response = Some(Transaction {
        owner: instance.owner.clone(),
        epoch: live.epoch,
        commands: vec![Command::Send {
            channel: format!("{id}.events1"),
            schema: API_VERSION,
            record: vec![Scalar::Bool(true)],
        }],
    });
}

/// Queues a reliable event through the real schema, sequence and rate validators.
fn receive(live: &mut Live<FakeWorker>, id: &str, sequence: u64) {
    let message = Envelope {
        version: WIRE_VERSION,
        session: live.grant.session.clone(),
        connection: live.grant.connection.clone(),
        subclient: 0,
        bundle: id.into(),
        generation: INITIAL_BUNDLE_GENERATION,
        channel: format!("{id}.events0"),
        schema: API_VERSION,
        sequence,
        world_epoch: live.epoch,
        payload: vec![Scalar::Bool(sequence.is_multiple_of(2))],
    };
    live.receive(&serde_json::to_vec(&message).unwrap(), CALLBACK_INTERVAL_MS)
        .unwrap();
}

#[test]
fn staggered_initialization_retains_sends_and_publishes_ready_first() {
    let mut live = fixture(2);
    complete(&mut live, "bundle0");
    assert!(live.poll(1, 0).unwrap().is_empty());
    assert!(!live.ready);
    assert_eq!(live.pending_sends.len(), 1);
    complete(&mut live, "bundle1");
    let packets = live.poll(1, 1).unwrap();
    assert!(matches!(
        serde_json::from_slice::<Control>(&packets[0]).unwrap(),
        Control::Ready { .. }
    ));
    let sends = packets[1..]
        .iter()
        .map(|bytes| serde_json::from_slice::<Envelope>(bytes).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        sends.iter().map(|send| send.sequence).collect::<Vec<_>>(),
        [1, 2]
    );
    assert_eq!(sends[0].bundle, "bundle0");
    assert_eq!(sends[1].bundle, "bundle1");
    assert_eq!(live.pending_send_bytes, 0);
}

#[test]
fn four_components_initialize_across_slices_before_readiness() {
    let mut live = fixture(MAX_BUNDLES);
    assert_eq!(
        live.instances
            .values()
            .filter(|instance| instance.helper.is_some())
            .count(),
        2
    );
    complete(&mut live, "bundle0");
    complete(&mut live, "bundle1");
    assert!(live.poll(1, 0).unwrap().is_empty());
    assert!(!live.ready);
    assert!(live.poll(1, CALLBACK_INTERVAL_MS).unwrap().is_empty());
    assert!(
        live.instances
            .values()
            .all(|instance| instance.helper.is_some())
    );
    complete(&mut live, "bundle2");
    assert!(live.poll(1, CALLBACK_INTERVAL_MS + 1).unwrap().is_empty());
    complete(&mut live, "bundle3");
    assert_eq!(
        live.poll(1, CALLBACK_INTERVAL_MS + 2).unwrap().len(),
        MAX_BUNDLES + 1
    );
    assert!(live.ready);
}

#[test]
fn reliable_bursts_wait_for_busy_helpers_and_aggregate_callback_budget() {
    let mut live = fixture(3);
    complete(&mut live, "bundle0");
    complete(&mut live, "bundle1");
    live.poll(1, CALLBACK_INTERVAL_MS).unwrap();
    complete(&mut live, "bundle2");
    live.poll(1, CALLBACK_INTERVAL_MS + 1).unwrap();
    for (index, id) in ["bundle0", "bundle0", "bundle1", "bundle2"]
        .into_iter()
        .enumerate()
    {
        receive(&mut live, id, index as u64 + 1);
    }
    live.poll(1, 2 * CALLBACK_INTERVAL_MS).unwrap();
    assert!(live.instances["bundle0"].busy);
    assert_eq!(live.ingress.peek(u64::MAX, 1).unwrap().sequence, 2);
    live.poll(1, 2 * CALLBACK_INTERVAL_MS + 1).unwrap();
    assert_eq!(live.ingress.peek(u64::MAX, 1).unwrap().sequence, 3);
    assert!(
        live.instances["bundle1"]
            .helper
            .as_ref()
            .unwrap()
            .dispatched
            .is_empty()
    );
    for tick in 3..=6 {
        live.poll(1, tick * CALLBACK_INTERVAL_MS).unwrap();
    }
    let delivered = live
        .instances
        .values()
        .map(|instance| instance.helper.as_ref().unwrap().dispatched.len())
        .collect::<Vec<_>>();
    assert_eq!(delivered, [2, 1, 1]);
    assert!(live.ingress.peek(u64::MAX, 1).is_none());
    let records = &live.instances["bundle0"]
        .helper
        .as_ref()
        .unwrap()
        .dispatched;
    assert_eq!(
        records,
        &[
            serde_json::to_vec(&vec![Scalar::Bool(false)]).unwrap(),
            serde_json::to_vec(&vec![Scalar::Bool(true)]).unwrap()
        ]
    );
}

#[test]
fn changed_dimension_rejects_completed_old_epoch_output_before_publication() {
    let mut live = fixture(1);
    complete(&mut live, "bundle0");
    assert!(live.poll(2, CALLBACK_INTERVAL_MS).is_err());
    assert_eq!(live.sequence, 1);
    assert!(!live.ready);
    assert!(live.pending_sends.is_empty());
    assert!(
        live.instances["bundle0"]
            .helper
            .as_ref()
            .unwrap()
            .response
            .is_some()
    );
}

const INTRO: &str = "media/clip.json";

/// Grants one fixture bundle the media and scene adapters and an indexed descriptor.
fn grant_media(live: &mut Live<FakeWorker>, id: &str) {
    let capabilities = &mut live.instances.get_mut(id).unwrap().capabilities;
    capabilities
        .scope
        .permissions
        .extend([Permission::Media, Permission::Scene]);
    capabilities.assets.insert(INTRO.into());
}

fn media_quad(x: f32) -> server_experience::runtime::SceneObject {
    server_experience::runtime::SceneObject::Quad {
        texture: INTRO.into(),
        transform: [x, 64.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        size: [16.0, 9.0],
    }
}

#[test]
fn guest_media_controls_reach_the_player_and_transitions_return_to_the_guest() {
    let mut live = fixture(1);
    grant_media(&mut live, "bundle0");
    let instance = live.instances.get_mut("bundle0").unwrap();
    instance.helper.as_mut().unwrap().response = Some(Transaction {
        owner: instance.owner.clone(),
        epoch: live.epoch,
        commands: vec![Command::Media {
            id: INTRO.into(),
            operation: server_experience::runtime::MediaOperation::Play,
            position_ms: 0,
        }],
    });
    live.poll(1, 0).unwrap();
    // No verified descriptor was registered, so the player is refused and reported stopped.
    live.media_mut().service(0, 0, true);
    live.poll(1, CALLBACK_INTERVAL_MS).unwrap();
    let helper = live.instances["bundle0"].helper.as_ref().unwrap();
    assert_eq!(helper.channels, [super::super::media::EVENT_CHANNEL]);
    let record: Vec<Scalar> = serde_json::from_slice(&helper.dispatched[0]).unwrap();
    assert!(matches!(
        record.as_slice(),
        [Scalar::Text(path), Scalar::Choice(2), Scalar::Integer(0)] if path == INTRO
    ));
}

#[test]
fn only_a_bundles_own_quad_textured_by_its_playing_media_becomes_a_textured_screen() {
    let mut live = fixture(2);
    for id in ["bundle0", "bundle1"] {
        let contributions = &mut live.instances.get_mut(id).unwrap().contributions;
        contributions
            .scene
            .insert(1, media_quad(if id == "bundle0" { 5.0 } else { 9.0 }));
    }
    assert!(
        live.changed_screens().unwrap().is_empty(),
        "no player means no screen"
    );
    let frame = render::MediaFrame {
        serial: 3,
        width: 2,
        height: 2,
        rgba: std::sync::Arc::from(vec![255u8; 16]),
    };
    live.media_mut().set_frame("bundle0", INTRO, Some(frame));
    let screens = live.changed_screens().unwrap();
    assert_eq!(screens.len(), 1);
    assert_eq!(screens[0].center, [5.0, 64.0, 0.0]);
    assert_eq!(screens[0].half_right, [8.0, 0.0, 0.0]);
    assert_eq!(screens[0].half_up, [0.0, 4.5, 0.0]);
    assert_eq!(screens[0].frame.as_ref().map(|frame| frame.serial), Some(3));
}

#[test]
fn an_unchanged_media_scene_is_presented_without_rebuilding() {
    let mut live = fixture(1);
    live.instances
        .get_mut("bundle0")
        .unwrap()
        .contributions
        .scene
        .insert(1, media_quad(5.0));
    let frame = render::MediaFrame {
        serial: 3,
        width: 2,
        height: 2,
        rgba: std::sync::Arc::from(vec![255u8; 16]),
    };
    live.media_mut().set_frame("bundle0", INTRO, Some(frame));
    assert_eq!(live.changed_screens().map(<[_]>::len), Some(1));
    let before = crate::tests::alloc_count::thread_allocations();
    let unchanged = live.changed_screens().is_none();
    assert_eq!(crate::tests::alloc_count::thread_allocations() - before, 0);
    assert!(unchanged);
}
