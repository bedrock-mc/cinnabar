//! Deterministic admission and delivery controls for cross-crate retirement fixtures.

use std::sync::Mutex;

use super::{
    admission::{Admission, JOB_OVERHEAD, Policy},
    producer::{Job, Step},
    *,
};

/// Manual world work sharing one admission owner across independent session inboxes.
#[doc(hidden)]
pub struct FarSummaryFixture {
    owner: Arc<Admission>,
    jobs: Arc<Mutex<VecDeque<Job>>>,
    assets: Arc<RuntimeAssets>,
}

impl FarSummaryFixture {
    /// Creates an isolated owner with enough bytes for `jobs` minimum-sized extraction slices.
    pub fn new(jobs: usize) -> Self {
        let slice_bytes = far_terrain::SurfaceSlice::MIN_BYTES;
        Self {
            owner: Arc::new(Admission::new(Policy {
                jobs,
                bytes: jobs * (slice_bytes + JOB_OVERHEAD),
                slice_bytes,
            })),
            jobs: Arc::default(),
            assets: Arc::new(RuntimeAssets::diagnostic()),
        }
    }

    /// Creates a session whose work runs only through this fixture's executor.
    pub fn inbox(&self) -> FarSummaryInbox {
        let jobs = Arc::clone(&self.jobs);
        FarSummaryInbox::new(Producer::new(
            Arc::clone(&self.owner),
            Arc::new(move |job| jobs.lock().unwrap().push_back(job)),
        ))
    }

    /// Queues an immutable final source against the fixture's retained diagnostic assets.
    pub fn enqueue(&self, inbox: &mut FarSummaryInbox, column: ChunkKey, snapshot: ColumnSnapshot) {
        let context = PendingContext {
            prepared: PreparedAssets::shared(Arc::clone(&self.assets)),
            air: 0,
            assets: Arc::clone(&self.assets),
            mode: NetworkIdMode::Sequential,
            tints: Arc::new(self.assets.biome_assets().resolve_live(&[]).unwrap()),
            default_biome: 0,
        };
        inbox.enqueue(column, snapshot, context);
    }

    /// Publishes a completed, charged result without running a worker, holding capacity until delivery.
    pub fn ready(&self, inbox: &mut FarSummaryInbox, column: ChunkKey, summary: EncodedSummary) {
        let permit = self
            .owner
            .acquire(JOB_OVERHEAD + summary.bytes.len())
            .expect("fixture result fits admission");
        let generation = inbox.issue(column);
        workers::send_result(
            &inbox.tx,
            Event::Step(Step {
                column,
                generation,
                progress: Progress::Complete(summary),
                _permit: permit,
            }),
        );
    }

    /// Runs one explicitly accepted job after releasing the manual executor lock.
    pub fn run_one(&self) -> bool {
        let job = self.jobs.lock().unwrap().pop_front();
        if let Some(job) = job {
            job();
            true
        } else {
            false
        }
    }

    /// Current charged count/bytes and their limits, for assertions across application drains.
    pub fn admission(&self) -> ((usize, usize), (usize, usize)) {
        let usage = self.owner.usage();
        let policy = self.owner.policy();
        ((usage.jobs, usage.bytes), (policy.jobs, policy.bytes))
    }
}
