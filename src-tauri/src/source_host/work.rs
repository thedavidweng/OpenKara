use super::{incompatible, types::*, unknown, Inner, SourceHost};
use std::{
    sync::{Arc, Weak},
    time::{Duration, Instant},
};

const WORK_APPROVAL_REQUIRED: &str =
    "This source has active sign-in, preview, or import work. Choose Stop Work to continue.";
const WORK_STOP_TIMEOUT: &str =
    "Source work has not stopped. Close the active sign-in, preview, or import, then retry.";

pub struct WorkLease {
    inner: Weak<Inner>,
    source_id: String,
    id: u64,
    cancellation: Cancellation,
}
impl WorkLease {
    pub fn cancellation(&self) -> Cancellation {
        self.cancellation.clone()
    }
}
impl Drop for WorkLease {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade() {
            if let Some(live) = inner.state.lock().unwrap().sources.get_mut(&self.source_id) {
                live.work.remove(&self.id);
            }
            inner.publish(&self.source_id);
        }
    }
}
impl SourceHost {
    pub fn acquire_work(&self, id: &str, kind: WorkKind) -> HostResult<WorkLease> {
        self.inner.check()?;
        let mut state = self.inner.state.lock().unwrap();
        let source = state.stored.sources.get(id).ok_or_else(unknown)?;
        if !source.enabled {
            return Err(HostError::new(
                "source_disabled",
                "Enable the source before starting work.",
            ));
        }
        if source
            .active
            .as_ref()
            .is_none_or(|release| self.inner.blocked(source, release))
        {
            return Err(incompatible());
        }
        state.sequence += 1;
        let sequence = state.sequence;
        let live = state.sources.entry(id.into()).or_default();
        if live.stopping {
            return Err(HostError::new("busy", "Source work is stopping."));
        }
        let cancel = Cancellation::new();
        live.work.insert(sequence, (kind, cancel.clone()));
        drop(state);
        self.inner.publish(id);
        Ok(WorkLease {
            inner: Arc::downgrade(&self.inner),
            source_id: id.into(),
            id: sequence,
            cancellation: cancel,
        })
    }
    pub(super) async fn stop_work(&self, id: &str, approved: bool) -> HostResult<()> {
        {
            let mut state = self.inner.state.lock().unwrap();
            let live = state.sources.entry(id.into()).or_default();
            if !live.work.is_empty() && !approved {
                return Err(HostError::new("active_work", WORK_APPROVAL_REQUIRED));
            }
            live.stopping = true;
            for (_, cancel) in live.work.values() {
                cancel.cancel();
            }
        }
        self.inner.publish(id);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if self
                .inner
                .state
                .lock()
                .unwrap()
                .sources
                .get(id)
                .is_none_or(|live| live.work.is_empty())
            {
                break;
            }
            if Instant::now() >= deadline {
                self.inner
                    .state
                    .lock()
                    .unwrap()
                    .sources
                    .entry(id.into())
                    .or_default()
                    .stopping = false;
                self.inner.publish(id);
                return Err(HostError::new("active_work", WORK_STOP_TIMEOUT));
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let gate = self.inner.gate(id, false);
        let _guard = gate.lock().await;
        let instance = self
            .inner
            .state
            .lock()
            .unwrap()
            .sources
            .entry(id.into())
            .or_default()
            .instance
            .take();
        if let Some(instance) = instance {
            instance.stop().await;
        }
        Ok(())
    }
}
