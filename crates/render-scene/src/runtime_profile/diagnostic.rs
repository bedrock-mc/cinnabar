//! Owned slow-frame fields, formatted after the frame thread releases its counters.

use crate::{runtime_profile_slow::SlowFrameReport, runtime_profile_uploads::UploadFrame};

pub(super) struct SlowFrameDiagnostic {
    pub timing: SlowFrameReport,
    #[cfg(any(test, feature = "alloc-stats"))]
    pub allocations: crate::runtime_profile_alloc::AllocationFrame,
    pub uploads: UploadFrame,
}

impl std::fmt::Display for SlowFrameDiagnostic {
    /// Formats captured intervals without reading the live timing or upload tables.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.timing.fmt(f)?;
        let mut fields = String::new();
        #[cfg(any(test, feature = "alloc-stats"))]
        self.allocations.append_fields(&mut fields);
        self.uploads.append_fields(&mut fields);
        f.write_str(&fields)
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;

    thread_local! {
        static REPORTS: std::cell::RefCell<Vec<Box<dyn std::fmt::Display + Send>>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }

    /// Retains the owned report until the test explicitly formats it.
    fn retain_report(report: Box<dyn std::fmt::Display + Send>) {
        REPORTS.with_borrow_mut(|reports| reports.push(report));
    }

    /// Later frame drains cannot change the allocation fields queued with a slow frame.
    #[test]
    fn deferred_report_retains_the_completed_allocation_interval() {
        let profiler =
            RuntimeStageProfiler::for_gameplay(false, None).with_diagnostic_sink(retain_report);
        profiler.begin_frame(true, false);
        profiler.state.allocations.record(
            RuntimeStage::UiPreparation,
            test_alloc::Allocations {
                calls: 2,
                bytes: 96,
            },
        );
        profiler.record(
            RuntimeStage::MainFrame,
            Instant::now(),
            Duration::from_millis(30),
        );
        profiler.begin_frame(true, false);
        profiler.begin_frame(true, false);
        let reports = REPORTS.with_borrow_mut(std::mem::take);
        assert_eq!(reports.len(), 1);
        let line = reports[0].to_string();
        assert!(line.contains("CINNABAR_SLOW_FRAME frame=1 "), "{line}");
        assert!(
            line.contains("stage_allocations=ui_preparation:2:96 "),
            "{line}"
        );
    }
}
