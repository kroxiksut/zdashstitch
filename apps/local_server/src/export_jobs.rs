// SPDX-License-Identifier: MIT OR Apache-2.0
//! Local, bounded export progress and cancellation, without file names or paths.
use crate::DialogCancellation;
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone, Default)]
pub(crate) struct ExportJobs(Arc<Mutex<HashMap<String, Arc<ExportJob>>>>);
pub(crate) struct ExportJob {
    state: Mutex<State>,
    pub(crate) dialog: DialogCancellation,
    created: Instant,
}
struct State {
    phase: &'static str,
    cancelled: bool,
    committing: bool,
    claimed: bool,
}
#[derive(Serialize)]
pub(crate) struct ExportStatus {
    phase: &'static str,
    cancellable: bool,
}

impl ExportJobs {
    pub(crate) fn create(&self) -> Option<String> {
        let mut jobs = self.0.lock().ok()?;
        jobs.retain(|_, job| job.created.elapsed() < Duration::from_secs(1800));
        if jobs.len() >= 32 {
            let oldest_finished = jobs
                .iter()
                .filter(|(_, job)| {
                    job.status().is_some_and(|status| {
                        matches!(status.phase, "done" | "failed" | "cancelled")
                    })
                })
                .min_by_key(|(_, job)| job.created)
                .map(|(id, _)| id.clone());
            if let Some(id) = oldest_finished {
                jobs.remove(&id);
            } else {
                return None;
            }
        }
        let mut bytes = [0_u8; 24];
        getrandom::fill(&mut bytes).ok()?;
        let id = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        jobs.insert(
            id.clone(),
            Arc::new(ExportJob {
                state: Mutex::new(State {
                    phase: "created",
                    cancelled: false,
                    committing: false,
                    claimed: false,
                }),
                dialog: DialogCancellation::default(),
                created: Instant::now(),
            }),
        );
        Some(id)
    }
    pub(crate) fn get(&self, id: &str) -> Option<Arc<ExportJob>> {
        self.0
            .lock()
            .ok()?
            .get(id)
            .cloned()
            .filter(|job| job.created.elapsed() < Duration::from_secs(1800))
    }
}
impl ExportJob {
    pub(crate) fn claim(&self) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if state.claimed || state.cancelled {
            return false;
        }
        state.claimed = true;
        true
    }
    pub(crate) fn stage(&self, phase: &'static str) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if state.cancelled {
            return false;
        }
        state.phase = phase;
        true
    }
    pub(crate) fn begin_commit(&self) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if state.cancelled {
            return false;
        }
        state.committing = true;
        state.phase = "committing";
        true
    }
    pub(crate) fn cancel(&self) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if state.committing || matches!(state.phase, "done" | "failed" | "cancelled") {
            return false;
        }
        state.cancelled = true;
        state.phase = "cancelled";
        self.dialog.cancel();
        true
    }
    pub(crate) fn is_cancelled(&self) -> bool {
        self.state.lock().map_or(true, |state| state.cancelled)
    }
    pub(crate) fn abort(&self) {
        if let Ok(mut state) = self.state.lock()
            && !matches!(state.phase, "done" | "cancelled")
        {
            if !state.committing {
                state.cancelled = true;
                self.dialog.cancel();
            }
            state.phase = "failed";
        }
    }
    pub(crate) fn finish(&self, success: bool) {
        if let Ok(mut state) = self.state.lock()
            && !state.cancelled
            && state.phase != "done"
        {
            state.phase = if success { "done" } else { "failed" };
        }
    }
    pub(crate) fn status(&self) -> Option<ExportStatus> {
        let state = self.state.lock().ok()?;
        Some(ExportStatus {
            phase: state.phase,
            cancellable: !state.committing
                && !state.cancelled
                && !matches!(state.phase, "done" | "failed"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_and_commit_are_mutually_exclusive() {
        let jobs = ExportJobs::default();
        let id = jobs.create().unwrap_or_else(|| panic!("create"));
        let job = jobs.get(&id).unwrap_or_else(|| panic!("get"));
        assert!(job.claim());
        assert!(!job.claim());
        assert!(job.cancel());
        assert!(!job.begin_commit());
        let id = jobs.create().unwrap_or_else(|| panic!("create"));
        let job = jobs.get(&id).unwrap_or_else(|| panic!("get"));
        assert!(job.claim());
        assert!(job.begin_commit());
        assert!(!job.cancel());
        job.finish(true);
        assert_eq!(job.status().map(|s| s.phase), Some("done"));
    }

    #[test]
    fn concurrent_cancel_and_commit_have_one_winner() {
        for _ in 0..64 {
            let jobs = ExportJobs::default();
            let id = jobs.create().unwrap_or_else(|| panic!("create"));
            let job = jobs.get(&id).unwrap_or_else(|| panic!("get"));
            let barrier = Arc::new(std::sync::Barrier::new(2));
            let other = job.clone();
            let other_barrier = barrier.clone();
            let thread = std::thread::spawn(move || {
                other_barrier.wait();
                other.cancel()
            });
            barrier.wait();
            let committed = job.begin_commit();
            let cancelled = thread.join().unwrap_or_else(|_| panic!("join"));
            assert_ne!(committed, cancelled);
        }
    }
}
