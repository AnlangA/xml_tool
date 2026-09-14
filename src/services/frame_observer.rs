//! Main-thread frame observation.
//!
//! The shell times its frame sections through [`FrameObserver`]:
//! durations over 16 ms log at debug, over 50 ms log at warning. A small
//! ring of recent samples is kept for diagnostics and tests.

use std::time::{Duration, Instant};

/// Thresholds fixed by the plan.
pub const DEBUG_THRESHOLD: Duration = Duration::from_millis(16);
pub const WARNING_THRESHOLD: Duration = Duration::from_millis(50);

/// One recorded section sample.
#[derive(Debug, Clone, Copy)]
pub struct Sample {
    pub label: &'static str,
    pub elapsed: Duration,
}

/// Collects per-frame section timings.
#[derive(Default)]
pub struct FrameObserver {
    samples: Vec<Sample>,
}

impl FrameObserver {
    /// Runs `section` and records how long it took under `label`.
    pub fn observe<T>(&mut self, label: &'static str, section: impl FnOnce() -> T) -> T {
        let started = Instant::now();
        let value = section();
        let elapsed = started.elapsed();
        self.record(label, elapsed);
        value
    }

    /// Records a duration measured by the caller.
    pub fn record(&mut self, label: &'static str, elapsed: Duration) {
        if elapsed > WARNING_THRESHOLD {
            log::warn!("[frame] {label} took {elapsed:?} (>50 ms)");
        } else if elapsed > DEBUG_THRESHOLD {
            log::debug!("[frame] {label} took {elapsed:?} (>16 ms)");
        }
        self.samples.push(Sample { label, elapsed });
        if self.samples.len() > 256 {
            self.samples.remove(0);
        }
    }

    /// Recent samples, oldest first.
    pub fn samples(&self) -> &[Sample] {
        &self.samples
    }

    /// The slowest recorded sample.
    pub fn slowest(&self) -> Option<Sample> {
        self.samples
            .iter()
            .copied()
            .max_by_key(|sample| sample.elapsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_buckets_thresholds() {
        let mut observer = FrameObserver::default();
        observer.record("fast", Duration::from_millis(5));
        observer.record("ok", Duration::from_millis(20));
        observer.record("slow", Duration::from_millis(80));
        assert_eq!(observer.samples().len(), 3);
        assert_eq!(observer.slowest().unwrap().label, "slow");
    }

    #[test]
    fn observe_measures_the_closure() {
        let mut observer = FrameObserver::default();
        let value = observer.observe("work", || 40 + 2);
        assert_eq!(value, 42);
        assert_eq!(observer.samples().len(), 1);
        assert_eq!(observer.samples()[0].label, "work");
    }
}
