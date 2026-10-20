use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Default)]
pub(super) struct RetainedDocuments {
    live: AtomicUsize,
    peak: AtomicUsize,
}

pub(super) struct RetainedDocument {
    counter: Arc<RetainedDocuments>,
}

impl RetainedDocument {
    /// Counts a retained normalization input until it is consumed or dropped.
    pub(super) fn new(counter: Arc<RetainedDocuments>) -> Self {
        let live = counter.live.fetch_add(1, Ordering::SeqCst) + 1;
        counter.peak.fetch_max(live, Ordering::SeqCst);
        Self { counter }
    }
}

impl Drop for RetainedDocument {
    /// Releases the live count with the owning normalized document.
    fn drop(&mut self) {
        self.counter.live.fetch_sub(1, Ordering::SeqCst);
    }
}

#[test]
fn preparation_memory_releases_rejected_documents() {
    rayon::ThreadPoolBuilder::new().num_threads(8).build().unwrap().install(|| {
        let counter = Arc::new(RetainedDocuments::default());
        let files = (0..8).map(|index| (
            format!("models/entity/{index}.json").into_boxed_str(),
            serde_json::to_vec(&serde_json::json!({"format_version":"1.12.0", "minecraft:geometry":[{
                "description":{"identifier":format!("geometry.fixture_{index}")}, "bones":[]
            }]})).unwrap(),
        )).collect();
        let mut selected = select_sources(files).into_iter().map(PreparedFile::new).collect::<Vec<_>>();
        for source in &mut selected { source.retained_documents = Some(Arc::clone(&counter)); }
        let result = compile_selected(&mut selected, None, &mut false,
            SourceBudget { count: 0, ..SourceBudget::default() }, &|| false).unwrap().unwrap();
        assert!(result.is_none());
        assert!(selected.iter().all(|source| source.products.is_none()));
        assert_eq!(counter.live.load(Ordering::SeqCst), 0);
        assert_eq!(counter.peak.load(Ordering::SeqCst), 1,
            "budget-rejected geometry documents must be released before the next normalization");
    });
}
