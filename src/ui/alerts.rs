//! Alert center: deduplicating, bounded problem collection.
//!
//! Every warning, error, and info notice in the shell flows through here.
//! Repeated alerts (same stable code + message) increment a count instead
//! of piling up rows; the history is capped (oldest evicted); each row can
//! be dismissed individually or cleared in bulk; severity filters hide
//! classes; and any error raises `panel_requested` so the shell can open
//! the problems panel exactly once, on arrival.

use crate::core::{Diagnostic, Severity};

/// Maximum retained alerts; the oldest are evicted beyond this.
pub const MAX_ALERTS: usize = 200;

/// One alert row.
#[derive(Debug, Clone, PartialEq)]
pub struct Alert {
    pub severity: Severity,
    /// Stable snake_case code (e.g. `io`, `source-draft`).
    pub code: String,
    /// Localized human message.
    pub message: String,
    /// 1-based position when known, for jump-to-source.
    pub position: Option<(usize, usize)>,
    /// Session the alert belongs to; jump-to-source switches to its tab.
    pub session: Option<crate::services::task_manager::SessionId>,
    /// How many times this alert fired (dedup counter).
    pub count: u32,
    /// Monotonic frame-stamp when first seen (oldest-first ordering).
    pub sequence: u64,
}

/// Which severity classes the problems panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeverityFilter {
    pub errors: bool,
    pub warnings: bool,
    pub infos: bool,
}

impl Default for SeverityFilter {
    fn default() -> Self {
        SeverityFilter::ALL
    }
}

impl SeverityFilter {
    /// Everything visible (the default).
    pub const ALL: SeverityFilter = SeverityFilter {
        errors: true,
        warnings: true,
        infos: true,
    };

    /// Whether `severity` passes the filter.
    pub fn shows(self, severity: Severity) -> bool {
        match severity {
            Severity::Error => self.errors,
            Severity::Warning => self.warnings,
            Severity::Info => self.infos,
        }
    }
}

/// The alert collection plus panel state.
#[derive(Debug, Default)]
pub struct AlertCenter {
    alerts: Vec<Alert>,
    next_sequence: u64,
    /// Total counts per severity (independent of filtering).
    error_count: usize,
    warning_count: usize,
    info_count: usize,
    pub filter: SeverityFilter,
    /// Set when a new error arrives; the shell opens the panel and clears
    /// the flag.
    pub panel_requested: bool,
    /// Session stamped onto new alerts (the shell updates it per frame).
    pub session: Option<crate::services::task_manager::SessionId>,
}

impl AlertCenter {
    /// Records an alert, deduplicating repeats and enforcing the cap.
    pub fn push(&mut self, severity: Severity, code: &str, message: &str) {
        self.push_with_position(severity, code, message, None);
    }

    /// Records an alert with an optional 1-based (line, column).
    pub fn push_with_position(
        &mut self,
        severity: Severity,
        code: &str,
        message: &str,
        position: Option<(usize, usize)>,
    ) {
        if let Some(existing) = self
            .alerts
            .iter_mut()
            .find(|alert| alert.code == code && alert.message == message)
        {
            existing.count = existing.count.saturating_add(1);
            if position.is_some() {
                existing.position = position;
            }
        } else {
            self.evict_if_full();
            self.alerts.push(Alert {
                severity,
                code: code.to_string(),
                message: message.to_string(),
                position,
                session: self.session,
                count: 1,
                sequence: self.next_sequence,
            });
            self.next_sequence += 1;
            match severity {
                Severity::Error => self.error_count += 1,
                Severity::Warning => self.warning_count += 1,
                Severity::Info => self.info_count += 1,
            }
        }
        if severity == Severity::Error {
            self.panel_requested = true;
        }
    }

    /// Records a diagnostic (line/column arguments carry the position).
    pub fn push_diagnostic(&mut self, diagnostic: &Diagnostic) {
        let position = match (
            diagnostic.arguments.get("line"),
            diagnostic.arguments.get("column"),
        ) {
            (Some(line), Some(column)) => {
                line.parse::<usize>().ok().zip(column.parse::<usize>().ok())
            }
            _ => None,
        };
        self.push_with_position(
            diagnostic.severity,
            &diagnostic.code,
            &diagnostic.message_key,
            position,
        );
    }

    /// All retained alerts, oldest first.
    pub fn alerts(&self) -> &[Alert] {
        &self.alerts
    }

    /// Alerts visible under the current filter, oldest first.
    pub fn visible(&self) -> impl Iterator<Item = &Alert> {
        self.alerts
            .iter()
            .filter(|alert| self.filter.shows(alert.severity))
    }

    /// Dismisses the alert with the given sequence id.
    pub fn dismiss(&mut self, sequence: u64) {
        if let Some(index) = self
            .alerts
            .iter()
            .position(|alert| alert.sequence == sequence)
        {
            let removed = self.alerts.remove(index);
            match removed.severity {
                Severity::Error => self.error_count = self.error_count.saturating_sub(1),
                Severity::Warning => self.warning_count = self.warning_count.saturating_sub(1),
                Severity::Info => self.info_count = self.info_count.saturating_sub(1),
            }
        }
    }

    /// Clears every alert.
    pub fn clear(&mut self) {
        self.alerts.clear();
        self.error_count = 0;
        self.warning_count = 0;
        self.info_count = 0;
    }

    /// Drops alerts whose severity is currently filtered out.
    pub fn clear_hidden(&mut self) {
        let filter = self.filter;
        self.alerts.retain(|alert| filter.shows(alert.severity));
        self.recount();
    }

    /// Drops every alert carrying `code` (resolved-state transitions).
    pub fn clear_code(&mut self, code: &str) {
        self.alerts.retain(|alert| alert.code != code);
        self.recount();
    }

    /// Number of retained alerts (all severities).
    pub fn len(&self) -> usize {
        self.alerts.len()
    }

    /// Whether anything is retained.
    pub fn is_empty(&self) -> bool {
        self.alerts.is_empty()
    }

    /// (errors, warnings, infos) totals.
    pub fn counts(&self) -> (usize, usize, usize) {
        (self.error_count, self.warning_count, self.info_count)
    }

    /// Whether any error is retained (for status-bar emphasis).
    pub fn has_errors(&self) -> bool {
        self.error_count > 0
    }

    fn evict_if_full(&mut self) {
        if self.alerts.len() >= MAX_ALERTS
            && let Some(oldest) = self.alerts.first().cloned()
        {
            match oldest.severity {
                Severity::Error => self.error_count = self.error_count.saturating_sub(1),
                Severity::Warning => self.warning_count = self.warning_count.saturating_sub(1),
                Severity::Info => self.info_count = self.info_count.saturating_sub(1),
            }
            self.alerts.remove(0);
        }
    }

    fn recount(&mut self) {
        self.error_count = 0;
        self.warning_count = 0;
        self.info_count = 0;
        for alert in &self.alerts {
            match alert.severity {
                Severity::Error => self.error_count += 1,
                Severity::Warning => self.warning_count += 1,
                Severity::Info => self.info_count += 1,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicates_increment_counts() {
        let mut center = AlertCenter::default();
        center.push(Severity::Warning, "io", "disk full");
        center.push(Severity::Warning, "io", "disk full");
        center.push(Severity::Warning, "io", "disk full");
        assert_eq!(center.len(), 1, "dedup keeps one row");
        assert_eq!(center.alerts()[0].count, 3);
        assert_eq!(center.counts(), (0, 1, 0));
    }

    #[test]
    fn distinct_messages_stay_separate() {
        let mut center = AlertCenter::default();
        center.push(Severity::Error, "io", "a");
        center.push(Severity::Error, "io", "b");
        assert_eq!(center.len(), 2);
        assert_eq!(center.counts(), (2, 0, 0));
    }

    #[test]
    fn errors_request_the_panel_once_until_consumed() {
        let mut center = AlertCenter::default();
        center.push(Severity::Info, "xpath", "3 nodes");
        assert!(!center.panel_requested, "info stays quiet");
        center.push(Severity::Error, "parse", "broken");
        assert!(center.panel_requested);
        center.panel_requested = false;
        center.push(Severity::Error, "parse", "broken");
        assert!(
            center.panel_requested,
            "every error (duplicates included) re-requests the panel; dedup only saves rows"
        );
    }

    #[test]
    fn cap_evicts_oldest() {
        let mut center = AlertCenter {
            alerts: Vec::new(),
            next_sequence: 0,
            error_count: 0,
            warning_count: 0,
            info_count: 0,
            filter: SeverityFilter::ALL,
            panel_requested: false,
            session: None,
        };
        for index in 0..(MAX_ALERTS + 25) {
            center.push(Severity::Info, "bulk", &format!("n{index}"));
        }
        assert_eq!(center.len(), MAX_ALERTS);
        assert_eq!(center.alerts().first().unwrap().code, "bulk");
        // The oldest 25 messages were evicted.
        assert_eq!(center.alerts().first().unwrap().message, "n25");
    }

    #[test]
    fn filter_and_dismiss() {
        let mut center = AlertCenter::default();
        center.push(Severity::Error, "e", "error");
        let warning_sequence = {
            center.push(Severity::Warning, "w", "warning");
            center.alerts().last().unwrap().sequence
        };
        center.push(Severity::Info, "i", "info");
        center.filter = SeverityFilter {
            errors: true,
            warnings: false,
            infos: false,
        };
        assert_eq!(center.visible().count(), 1);
        assert_eq!(center.visible().next().unwrap().code, "e");

        center.dismiss(warning_sequence);
        assert_eq!(center.counts(), (1, 0, 1), "warning count drops");

        center.clear();
        assert!(center.is_empty());
        assert_eq!(center.counts(), (0, 0, 0));
    }

    #[test]
    fn diagnostics_carry_positions() {
        let mut center = AlertCenter::default();
        let diagnostic = Diagnostic::new(Severity::Error, "source-draft", "mismatched tag")
            .with_argument("line", "2")
            .with_argument("column", "7");
        center.push_diagnostic(&diagnostic);
        assert_eq!(center.alerts()[0].position, Some((2, 7)));
        assert!(center.has_errors());
    }

    #[test]
    fn clear_code_removes_only_that_code() {
        let mut center = AlertCenter::default();
        center.push(Severity::Warning, "exi-fidelity", "drops comments");
        center.push(Severity::Error, "io", "gone");
        center.clear_code("io");
        assert_eq!(center.len(), 1);
        assert_eq!(center.alerts()[0].code, "exi-fidelity");
        assert_eq!(center.counts(), (0, 1, 0));
    }
}

#[cfg(test)]
mod default_tests {
    use super::*;

    #[test]
    fn default_filter_shows_everything() {
        assert!(SeverityFilter::default().shows(Severity::Error));
        assert!(SeverityFilter::default().shows(Severity::Warning));
        assert!(SeverityFilter::default().shows(Severity::Info));
        let center = AlertCenter::default();
        assert_eq!(center.filter, SeverityFilter::ALL);
    }
}
