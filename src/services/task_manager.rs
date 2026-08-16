//! Background task scheduling with staleness filtering and cancellation.
//!
//! `TaskManager` is deliberately minimal: jobs run on spawned threads
//! (bounded by a semaphore), communicate through an mpsc channel, and carry
//! the `JobId + SessionId + Revision` triple they were started with. A job
//! whose triple no longer matches the live document — because the user kept
//! editing, switched files, or cancelled — completes but its result is
//! dropped by [`TaskManager::take_outcome`].
//!
//! Cancellation is cooperative: jobs receive an [`CancelFlag`] and check it
//! at safe points. Library calls that cannot be interrupted still work:
//! their results are discarded when the flag is set.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use crate::core::Revision;

/// Unique id of a submitted job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(pub u64);

/// Identity of a document session (see `workspace::DocumentSession`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId(pub u64);

/// What a job produced when it finished.
pub struct JobOutcome {
    pub job: JobId,
    /// The job's return value, type-erased; callers downcast to the type
    /// their closure returned.
    pub result: Box<dyn std::any::Any + Send>,
}

/// Downcasts a job result, panicking with a useful message on mismatch.
#[macro_export]
macro_rules! job_result {
    ($outcome:expr, $ty:ty) => {
        $outcome
            .result
            .downcast::<$ty>()
            .map(|boxed| *boxed)
            .map_err(|bad| {
                format!(
                    "job {} returned {}",
                    $outcome.job.0,
                    std::any::type_name_of_val(&*bad)
                )
            })
            .expect("job payload type matches the caller's closure")
    };
}

/// Cooperative cancellation token shared with a running job.
pub type CancelFlag = Arc<AtomicBool>;

/// Description of a submitted job: what it is and which document state it
/// was computed against.
#[derive(Debug, Clone, Copy)]
pub struct TaskSpec {
    pub job: JobId,
    pub session: SessionId,
    pub revision: Revision,
}

/// Scheduler for background jobs. Clone-free; the UI owns one instance.
pub struct TaskManager {
    next_job: AtomicU64,
    pending: Mutex<Vec<PendingJob>>,
    /// Capacity bound on concurrently running threads.
    permits: Arc<CountingSemaphore>,
    /// Channel carrying finished job results back to the main thread.
    results: (
        std::sync::mpsc::Sender<FinishedJob>,
        Mutex<std::sync::mpsc::Receiver<FinishedJob>>,
    ),
}

struct PendingJob {
    spec: TaskSpec,
    cancel: CancelFlag,
}

type JobBody = Box<dyn FnOnce() -> Box<dyn std::any::Any + Send> + Send>;
type FinishedJob = (TaskSpec, CancelFlag, Box<dyn std::any::Any + Send>);

/// Minimal counting semaphore (MSRV-safe; `std::thread::Semaphore` is newer
/// than the 1.88 toolchain this project pins).
struct CountingSemaphore {
    state: Mutex<usize>,
    available: Condvar,
}

impl CountingSemaphore {
    fn new(permits: usize) -> Self {
        CountingSemaphore {
            state: Mutex::new(permits),
            available: Condvar::new(),
        }
    }

    fn acquire(&self) -> impl Drop + '_ {
        let mut remaining = self.state.lock().expect("semaphore lock");
        while *remaining == 0 {
            remaining = self
                .available
                .wait(remaining)
                .expect("semaphore lock poisoned");
        }
        *remaining -= 1;
        PermitGuard { semaphore: self }
    }
}

struct PermitGuard<'a> {
    semaphore: &'a CountingSemaphore,
}

impl Drop for PermitGuard<'_> {
    fn drop(&mut self) {
        let mut remaining = self.semaphore.state.lock().expect("semaphore lock");
        *remaining += 1;
        self.semaphore.available.notify_one();
    }
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskManager {
    /// A manager allowing up to `max_parallel` concurrent jobs.
    pub fn new() -> TaskManager {
        TaskManager::with_parallelism(4)
    }

    /// A manager with an explicit worker bound.
    pub fn with_parallelism(max_parallel: usize) -> TaskManager {
        let (tx, rx) = std::sync::mpsc::channel();
        TaskManager {
            next_job: AtomicU64::new(1),
            pending: Mutex::new(Vec::new()),
            permits: Arc::new(CountingSemaphore::new(max_parallel.max(1))),
            results: (tx, Mutex::new(rx)),
        }
    }

    /// Submits `body` as a background job attributed to `session` at
    /// `revision`. Returns the job id and its cancel flag. The closure's
    /// return value is delivered back type-erased; the poller downcasts it.
    pub fn spawn(
        &self,
        session: SessionId,
        revision: Revision,
        body: impl FnOnce(CancelFlag) -> Box<dyn std::any::Any + Send> + Send + 'static,
    ) -> (JobId, CancelFlag) {
        let job = JobId(self.next_job.fetch_add(1, Ordering::SeqCst));
        let spec = TaskSpec {
            job,
            session,
            revision,
        };
        let cancel: CancelFlag = Arc::new(AtomicBool::new(false));
        self.pending
            .lock()
            .expect("pending jobs lock")
            .push(PendingJob {
                spec,
                cancel: Arc::clone(&cancel),
            });

        let sender = self.results.0.clone();
        let permits = Arc::clone(&self.permits);
        let cancel_for_body = Arc::clone(&cancel);
        let body: JobBody = Box::new(move || body(cancel_for_body));
        let spec_for_thread = spec;
        let cancel_for_thread = Arc::clone(&cancel);
        std::thread::spawn(move || {
            let _permit = permits.acquire();
            let result = body();
            // A cancelled job still reports back; the manager drops its
            // payload when the flag is set so late completions stay inert.
            let _ = sender.send((spec_for_thread, cancel_for_thread, result));
        });
        (job, cancel)
    }

    /// Requests cancellation of `job`; its result, if it arrives, is
    /// discarded.
    pub fn cancel(&self, job: JobId) {
        if let Some(pending) = self
            .pending
            .lock()
            .expect("pending jobs lock")
            .iter()
            .find(|pending| pending.spec.job == job)
        {
            pending.cancel.store(true, Ordering::SeqCst);
        }
    }

    /// Cancels every job attributed to `session`.
    pub fn cancel_session(&self, session: SessionId) {
        for pending in self.pending.lock().expect("pending jobs lock").iter() {
            if pending.spec.session == session {
                pending.cancel.store(true, Ordering::SeqCst);
            }
        }
    }

    /// Non-blocking poll for the next finished job whose triple still
    /// matches `session`/`revision` and whose cancellation flag is clear.
    ///
    /// Stale or cancelled results are dropped silently. `Ok(None)` means
    /// "nothing applicable right now" (channel drained); `Err` means the
    /// channel closed.
    pub fn take_outcome(
        &self,
        session: SessionId,
        revision: Revision,
    ) -> Result<Option<JobOutcome>, std::sync::mpsc::TryRecvError> {
        loop {
            let received = {
                let receiver = self.results.1.lock().expect("results lock");
                receiver.try_recv()?
            };
            let (spec, cancel, payload) = received;
            if cancel.load(Ordering::SeqCst) {
                continue; // cancelled job: drop
            }
            if spec.session != session || spec.revision != revision {
                continue; // stale: drop
            }
            return Ok(Some(JobOutcome {
                job: spec.job,
                result: payload,
            }));
        }
    }

    /// Blocking variant for tests: drains until an applicable outcome
    /// arrives (or the deadline passes).
    pub fn wait_for_outcome(&self, session: SessionId, revision: Revision) -> Option<JobOutcome> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while std::time::Instant::now() < deadline {
            match self.take_outcome(session, revision) {
                Ok(outcome) => return outcome,
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return None,
            }
        }
        None
    }
}
