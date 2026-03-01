//! Syscall anomaly detection for shell command output.
//!
//! Monitors shell command output for syscall-related patterns (strace output,
//! audit logs, denied operations) and flags anomalies against a configurable
//! baseline. Supports alert budgets and cooldown to prevent alert fatigue.

use std::collections::HashMap;
use std::time::Instant;

/// A single detected syscall event parsed from command output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyscallEvent {
    pub syscall: String,
    pub denied: bool,
    pub raw_line: String,
}

/// Verdict from the anomaly detector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnomalyVerdict {
    /// No anomaly detected.
    Clean,
    /// Anomalies were detected.
    Alert { alerts: Vec<String> },
    /// Alert budget exhausted — further alerts suppressed until cooldown.
    Suppressed { reason: String },
}

/// Configuration for the syscall anomaly detector.
#[derive(Debug, Clone)]
pub struct SyscallAnomalyConfig {
    pub enabled: bool,
    pub strict_mode: bool,
    pub alert_on_unknown_syscall: bool,
    pub max_denied_events_per_minute: u32,
    pub max_total_events_per_minute: u32,
    pub max_alerts_per_minute: u32,
    pub alert_cooldown_secs: u64,
    pub baseline_syscalls: Vec<String>,
}

impl Default for SyscallAnomalyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            strict_mode: false,
            alert_on_unknown_syscall: true,
            max_denied_events_per_minute: 5,
            max_total_events_per_minute: 120,
            max_alerts_per_minute: 30,
            alert_cooldown_secs: 20,
            baseline_syscalls: vec![
                "read".to_string(),
                "write".to_string(),
                "openat".to_string(),
                "close".to_string(),
                "execve".to_string(),
                "futex".to_string(),
            ],
        }
    }
}

/// Stateful syscall anomaly detector.
///
/// Tracks event rates, alert budgets, and cooldown windows.
pub struct SyscallAnomalyDetector {
    config: SyscallAnomalyConfig,
    /// Counts of events within the current minute window.
    total_events_this_window: u32,
    denied_events_this_window: u32,
    alerts_this_window: u32,
    /// When the current window started.
    window_start: Instant,
    /// Cooldown: if set, alerts are suppressed until this instant.
    cooldown_until: Option<Instant>,
    /// Counts of each syscall seen (for reporting).
    syscall_counts: HashMap<String, u32>,
}

impl SyscallAnomalyDetector {
    pub fn new(config: SyscallAnomalyConfig) -> Self {
        Self {
            config,
            total_events_this_window: 0,
            denied_events_this_window: 0,
            alerts_this_window: 0,
            window_start: Instant::now(),
            cooldown_until: None,
            syscall_counts: HashMap::new(),
        }
    }

    /// Reset the rate-limiting window if a minute has elapsed.
    fn maybe_reset_window(&mut self) {
        let elapsed = self.window_start.elapsed().as_secs();
        if elapsed >= 60 {
            self.total_events_this_window = 0;
            self.denied_events_this_window = 0;
            self.alerts_this_window = 0;
            self.window_start = Instant::now();
        }
    }

    /// Check if we're in cooldown.
    fn in_cooldown(&self) -> bool {
        self.cooldown_until
            .map(|until| Instant::now() < until)
            .unwrap_or(false)
    }

    /// Analyze shell command output for syscall anomalies.
    pub fn analyze(&mut self, command_output: &str) -> AnomalyVerdict {
        if !self.config.enabled {
            return AnomalyVerdict::Clean;
        }

        self.maybe_reset_window();

        if self.in_cooldown() {
            return AnomalyVerdict::Suppressed {
                reason: "alert cooldown active".to_string(),
            };
        }

        let events = parse_syscall_events(command_output);
        if events.is_empty() {
            return AnomalyVerdict::Clean;
        }

        let mut alerts = Vec::new();

        for event in &events {
            self.total_events_this_window += 1;
            *self.syscall_counts.entry(event.syscall.clone()).or_insert(0) += 1;

            if event.denied {
                self.denied_events_this_window += 1;
            }

            // Check if syscall is in baseline
            let is_known = self
                .config
                .baseline_syscalls
                .iter()
                .any(|b| b == &event.syscall);

            if !is_known && self.config.alert_on_unknown_syscall {
                alerts.push(format!(
                    "unknown syscall '{}' not in baseline",
                    event.syscall
                ));
            }

            if event.denied {
                alerts.push(format!("denied syscall: {}", event.syscall));
            }
        }

        // Rate limit checks
        if self.denied_events_this_window > self.config.max_denied_events_per_minute {
            alerts.push(format!(
                "denied event rate {} exceeds limit {}/min",
                self.denied_events_this_window, self.config.max_denied_events_per_minute
            ));
        }

        if self.total_events_this_window > self.config.max_total_events_per_minute {
            alerts.push(format!(
                "total event rate {} exceeds limit {}/min",
                self.total_events_this_window, self.config.max_total_events_per_minute
            ));
        }

        if alerts.is_empty() {
            return AnomalyVerdict::Clean;
        }

        // Enforce alert budget
        self.alerts_this_window += 1;
        if self.alerts_this_window > self.config.max_alerts_per_minute {
            self.cooldown_until = Some(
                Instant::now()
                    + std::time::Duration::from_secs(self.config.alert_cooldown_secs),
            );
            return AnomalyVerdict::Suppressed {
                reason: format!(
                    "alert budget exhausted ({}/min), cooldown {}s",
                    self.config.max_alerts_per_minute, self.config.alert_cooldown_secs
                ),
            };
        }

        // In strict mode, include all alerts; otherwise deduplicate
        if !self.config.strict_mode {
            alerts.dedup();
        }

        AnomalyVerdict::Alert { alerts }
    }

    /// Return the accumulated syscall counts.
    pub fn syscall_counts(&self) -> &HashMap<String, u32> {
        &self.syscall_counts
    }

    /// Reset all state.
    pub fn reset(&mut self) {
        self.total_events_this_window = 0;
        self.denied_events_this_window = 0;
        self.alerts_this_window = 0;
        self.window_start = Instant::now();
        self.cooldown_until = None;
        self.syscall_counts.clear();
    }
}

/// Parse command output for syscall events.
///
/// Recognises:
/// - strace-style lines: `openat(AT_FDCWD, "/etc/passwd", O_RDONLY) = 3`
/// - audit log lines: `type=SYSCALL ... syscall=59 ... denied`
/// - seccomp lines: `audit: seccomp ... syscall=read ...`
pub fn parse_syscall_events(output: &str) -> Vec<SyscallEvent> {
    let mut events = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // strace-style: `syscall_name(args...) = result`
        if let Some(event) = parse_strace_line(trimmed) {
            events.push(event);
            continue;
        }

        // audit/seccomp-style: `type=SYSCALL ... syscall=NAME ...`
        if let Some(event) = parse_audit_line(trimmed) {
            events.push(event);
            continue;
        }
    }

    events
}

/// Parse a strace-style line like `openat(AT_FDCWD, "/etc/passwd", O_RDONLY) = 3`
fn parse_strace_line(line: &str) -> Option<SyscallEvent> {
    let paren_idx = line.find('(')?;
    let syscall_name = line[..paren_idx].trim();

    // Filter out lines that don't look like syscall names
    if syscall_name.is_empty()
        || syscall_name.contains(' ')
        || syscall_name.len() > 32
    {
        return None;
    }

    // All lowercase + underscore is the typical pattern for syscall names
    if !syscall_name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return None;
    }

    let denied = line.contains("EACCES")
        || line.contains("EPERM")
        || line.contains("= -1 EACCES")
        || line.contains("= -1 EPERM");

    Some(SyscallEvent {
        syscall: syscall_name.to_string(),
        denied,
        raw_line: line.to_string(),
    })
}

/// Parse an audit/seccomp log line.
fn parse_audit_line(line: &str) -> Option<SyscallEvent> {
    // Look for `syscall=NAME` or `syscall=NUMBER`
    let syscall_prefix = "syscall=";
    let idx = line.find(syscall_prefix)?;
    let after = &line[idx + syscall_prefix.len()..];
    let syscall_token: String = after
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();

    if syscall_token.is_empty() {
        return None;
    }

    // Map well-known syscall numbers to names (Linux x86_64)
    let syscall_name = match syscall_token.as_str() {
        "0" => "read".to_string(),
        "1" => "write".to_string(),
        "2" => "open".to_string(),
        "3" => "close".to_string(),
        "59" => "execve".to_string(),
        "56" => "clone".to_string(),
        "57" => "fork".to_string(),
        "62" => "kill".to_string(),
        "101" => "ptrace".to_string(),
        "257" => "openat".to_string(),
        other => other.to_string(),
    };

    let denied = line.contains("denied")
        || line.contains("DENIED")
        || line.contains("blocked")
        || line.contains("action=blocked");

    Some(SyscallEvent {
        syscall: syscall_name,
        denied,
        raw_line: line.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_strace_basic() {
        let line = r#"openat(AT_FDCWD, "/etc/passwd", O_RDONLY) = 3"#;
        let events = parse_syscall_events(line);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].syscall, "openat");
        assert!(!events[0].denied);
    }

    #[test]
    fn parse_strace_denied() {
        let line = r#"openat(AT_FDCWD, "/root/.ssh/id_rsa", O_RDONLY) = -1 EACCES (Permission denied)"#;
        let events = parse_syscall_events(line);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].syscall, "openat");
        assert!(events[0].denied);
    }

    #[test]
    fn parse_strace_eperm() {
        let line = r#"kill(1234, SIGKILL) = -1 EPERM (Operation not permitted)"#;
        let events = parse_syscall_events(line);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].syscall, "kill");
        assert!(events[0].denied);
    }

    #[test]
    fn parse_audit_line_with_number() {
        let line = "type=SYSCALL msg=audit(1234): arch=c000003e syscall=59 success=yes";
        let events = parse_syscall_events(line);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].syscall, "execve");
        assert!(!events[0].denied);
    }

    #[test]
    fn parse_audit_line_denied() {
        let line = "audit: seccomp syscall=101 action=blocked denied";
        let events = parse_syscall_events(line);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].syscall, "ptrace");
        assert!(events[0].denied);
    }

    #[test]
    fn parse_audit_line_with_name() {
        let line = "seccomp: syscall=connect denied";
        let events = parse_syscall_events(line);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].syscall, "connect");
        assert!(events[0].denied);
    }

    #[test]
    fn parse_no_syscall_events() {
        let output = "total 64\ndrwxr-xr-x  10 user staff  320 Feb 28 12:00 .\n";
        let events = parse_syscall_events(output);
        assert!(events.is_empty());
    }

    #[test]
    fn parse_multiple_strace_lines() {
        let output = r#"read(3, "hello", 5) = 5
write(1, "hello", 5) = 5
close(3) = 0
"#;
        let events = parse_syscall_events(output);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].syscall, "read");
        assert_eq!(events[1].syscall, "write");
        assert_eq!(events[2].syscall, "close");
    }

    #[test]
    fn detector_clean_baseline_events() {
        let config = SyscallAnomalyConfig::default();
        let mut detector = SyscallAnomalyDetector::new(config);

        let output = r#"read(3, "data", 1024) = 1024
write(1, "output", 6) = 6
close(3) = 0
"#;
        let verdict = detector.analyze(output);
        assert_eq!(verdict, AnomalyVerdict::Clean);
    }

    #[test]
    fn detector_flags_unknown_syscall() {
        let config = SyscallAnomalyConfig::default();
        let mut detector = SyscallAnomalyDetector::new(config);

        let output = r#"ptrace(PTRACE_ATTACH, 1234) = 0"#;
        match detector.analyze(output) {
            AnomalyVerdict::Alert { alerts } => {
                assert!(alerts.iter().any(|a| a.contains("ptrace")));
            }
            other => panic!("expected Alert, got {other:?}"),
        }
    }

    #[test]
    fn detector_flags_denied_event() {
        let config = SyscallAnomalyConfig::default();
        let mut detector = SyscallAnomalyDetector::new(config);

        let output = r#"openat(AT_FDCWD, "/root/.ssh/id_rsa", O_RDONLY) = -1 EACCES (Permission denied)"#;
        match detector.analyze(output) {
            AnomalyVerdict::Alert { alerts } => {
                assert!(alerts.iter().any(|a| a.contains("denied")));
            }
            other => panic!("expected Alert, got {other:?}"),
        }
    }

    #[test]
    fn detector_disabled_always_clean() {
        let config = SyscallAnomalyConfig {
            enabled: false,
            ..Default::default()
        };
        let mut detector = SyscallAnomalyDetector::new(config);
        let output = r#"ptrace(PTRACE_ATTACH, 1234) = 0"#;
        assert_eq!(detector.analyze(output), AnomalyVerdict::Clean);
    }

    #[test]
    fn detector_alert_budget_exhaustion() {
        let config = SyscallAnomalyConfig {
            max_alerts_per_minute: 2,
            alert_cooldown_secs: 10,
            ..Default::default()
        };
        let mut detector = SyscallAnomalyDetector::new(config);

        let bad_output = r#"ptrace(PTRACE_ATTACH, 1234) = 0"#;

        // First two alerts should go through
        assert!(matches!(detector.analyze(bad_output), AnomalyVerdict::Alert { .. }));
        assert!(matches!(detector.analyze(bad_output), AnomalyVerdict::Alert { .. }));

        // Third should be suppressed
        match detector.analyze(bad_output) {
            AnomalyVerdict::Suppressed { reason } => {
                assert!(reason.contains("budget exhausted"));
            }
            other => panic!("expected Suppressed, got {other:?}"),
        }
    }

    #[test]
    fn detector_reset_clears_state() {
        let config = SyscallAnomalyConfig::default();
        let mut detector = SyscallAnomalyDetector::new(config);

        let output = r#"ptrace(PTRACE_ATTACH, 1234) = 0"#;
        detector.analyze(output);
        assert!(!detector.syscall_counts().is_empty());

        detector.reset();
        assert!(detector.syscall_counts().is_empty());
    }

    #[test]
    fn detector_no_alert_when_unknown_disabled() {
        let config = SyscallAnomalyConfig {
            alert_on_unknown_syscall: false,
            ..Default::default()
        };
        let mut detector = SyscallAnomalyDetector::new(config);

        let output = r#"connect(3, {sa_family=AF_INET}, 16) = 0"#;
        assert_eq!(detector.analyze(output), AnomalyVerdict::Clean);
    }

    #[test]
    fn detector_denied_rate_limit() {
        let config = SyscallAnomalyConfig {
            max_denied_events_per_minute: 2,
            ..Default::default()
        };
        let mut detector = SyscallAnomalyDetector::new(config);

        let denied = r#"openat(AT_FDCWD, "/secret", O_RDONLY) = -1 EACCES (Permission denied)"#;
        // First two denied events — alerts for denied syscall
        detector.analyze(denied);
        detector.analyze(denied);

        // Third denied event should trigger rate limit alert
        match detector.analyze(denied) {
            AnomalyVerdict::Alert { alerts } => {
                assert!(alerts.iter().any(|a| a.contains("denied event rate")));
            }
            other => panic!("expected rate limit Alert, got {other:?}"),
        }
    }
}
