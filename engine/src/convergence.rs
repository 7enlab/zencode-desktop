//! Convergence guard (v0.4.0 "Trust" roadmap, R6): detects repair loops and forces a
//! re-plan instead of silently burning tokens.
//!
//! Motivation: runaway token burn is the universal complaint about coding agents — a Cline
//! user burned $187 overnight in one stuck session, a Roo Code user $47 in a single
//! afternoon, and repair loops of 20-40 iterations "leave the workspace in a worse state".
//!
//! The guard sits between the tool loop and the verify step (PR #32). Feed it one record
//! per tool round:
//! - [`LoopDetector::record_edit`] after every Write/Edit tool call,
//! - [`LoopDetector::record_verify`] after every verify/test outcome,
//! - [`LoopDetector::record_stagnant_round`] for tool rounds that changed no files.
//!
//! When a loop pattern fires, [`LoopDetector::check`] returns it exactly once; the caller
//! stops the loop, records a `loop_detected` system event via
//! [`LoopDetector::emit_loop_detected`], and hands the agent the forced re-plan prompt from
//! [`LoopDetector::replan_prompt`]. The optional `restore_point` is a checkpoint id from
//! PR #31's `CheckpointManager` — this crate does not depend on it, the caller passes the
//! id through so the re-plan prompt can reference it.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use zen_core::{AgentEvent, EventSink};

/// Which loop pattern fired.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LoopPattern {
    /// The same file was edited `attempts` times, every verify in between failing.
    SameFileFailingEdits { path: String, attempts: u32 },
    /// The same (normalized) error kept coming back `attempts` times in a row.
    RepeatingError { signature: String, attempts: u32 },
    /// `rounds` consecutive tool rounds changed no files.
    StagnantRounds { rounds: u32 },
}

impl LoopPattern {
    /// Short human-readable description for the re-plan prompt.
    pub fn describe(&self) -> String {
        match self {
            LoopPattern::SameFileFailingEdits { path, attempts } => format!(
                "edited '{path}' {attempts} times in a row, and verification failed after every edit"
            ),
            LoopPattern::RepeatingError { signature, attempts } => format!(
                "the same error kept repeating {attempts} times in a row: '{signature}'"
            ),
            LoopPattern::StagnantRounds { rounds } => {
                format!("{rounds} tool rounds in a row changed no files (no progress)")
            }
        }
    }
}

/// Tunable thresholds. Defaults match the community-observed failure modes.
#[derive(Debug, Clone)]
pub struct ConvergencePolicy {
    /// Same file edited this many times with failing verify between edits.
    pub max_same_file_edits: u32,
    /// Same normalized error signature this many times in a row.
    pub max_same_error: u32,
    /// Tool rounds with no file changes before calling it stagnant.
    pub max_stagnant_rounds: u32,
}

impl Default for ConvergencePolicy {
    fn default() -> Self {
        Self {
            max_same_file_edits: 3,
            max_same_error: 3,
            max_stagnant_rounds: 5,
        }
    }
}

impl ConvergencePolicy {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.max_same_file_edits == 0 {
            anyhow::bail!("max_same_file_edits must be >= 1");
        }
        if self.max_same_error == 0 {
            anyhow::bail!("max_same_error must be >= 1");
        }
        if self.max_stagnant_rounds == 0 {
            anyhow::bail!("max_stagnant_rounds must be >= 1");
        }
        Ok(())
    }
}

/// One edit -> verify attempt, kept for the re-plan prompt.
#[derive(Debug, Clone)]
struct AttemptRecord {
    path: String,
    verify_passed: bool,
    error_preview: Option<String>,
}

/// JSON body of the `loop_detected` system event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LoopDetectedPayload {
    pub pattern: LoopPattern,
    pub attempts: u32,
    pub files: Vec<String>,
    /// Checkpoint id (PR #31) the agent may restore, if the caller took one.
    pub restore_point: Option<String>,
}

/// Name of the system event emitted when a loop is detected.
pub const LOOP_DETECTED_EVENT: &str = "loop_detected";

/// Chars of an error kept in history / prompts.
pub const ERROR_PREVIEW_LIMIT: usize = 500;

/// Tracks tool rounds and detects repair loops. Not thread-safe by design: one
/// detector per task run, driven by the single-threaded tool loop.
pub struct LoopDetector {
    policy: ConvergencePolicy,
    /// Consecutive failing edits per path (reset when a verify passes).
    failing_edits: HashMap<String, u32>,
    /// Consecutive occurrences of the current normalized error signature.
    error_counts: HashMap<String, u32>,
    /// Consecutive tool rounds with no file changes.
    stagnant_rounds: u32,
    /// Full attempt history for the re-plan prompt.
    history: Vec<AttemptRecord>,
    /// Paths edited since the last verify (to reset on pass).
    pending_paths: Vec<String>,
    /// Once a pattern fired, `check` stays quiet until `reset` (the task stops).
    triggered: bool,
}

impl LoopDetector {
    pub fn new(policy: ConvergencePolicy) -> anyhow::Result<Self> {
        policy.validate()?;
        Ok(Self {
            policy,
            failing_edits: HashMap::new(),
            error_counts: HashMap::new(),
            stagnant_rounds: 0,
            history: Vec::new(),
            pending_paths: Vec::new(),
            triggered: false,
        })
    }

    /// Record a Write/Edit tool call against `path`.
    pub fn record_edit(&mut self, path: &str) {
        let path = normalize_path(path);
        *self.failing_edits.entry(path.clone()).or_insert(0) += 1;
        if !self.pending_paths.contains(&path) {
            self.pending_paths.push(path);
        }
        // Any file change is progress: stagnation ends.
        self.stagnant_rounds = 0;
    }

    /// Record the outcome of a verify/test step.
    ///
    /// `error` is the raw failure output (stderr or failing test names); pass `None`
    /// when there is nothing to record (e.g. verify passed).
    pub fn record_verify(&mut self, passed: bool, error: Option<&str>) {
        if passed {
            // Success breaks every loop pattern: the approach worked.
            for path in self.pending_paths.drain(..) {
                self.failing_edits.remove(&path);
            }
            self.error_counts.clear();
            self.history.push(AttemptRecord {
                path: String::new(),
                verify_passed: true,
                error_preview: None,
            });
            return;
        }
        let error_preview = error.map(|e| truncate(e, ERROR_PREVIEW_LIMIT));
        for path in self.pending_paths.drain(..) {
            self.history.push(AttemptRecord {
                path,
                verify_passed: false,
                error_preview: error_preview.clone(),
            });
        }
        if let Some(raw) = error {
            let sig = normalize_error(raw);
            // Consecutive: a different signature restarts the count.
            self.error_counts.retain(|k, _| *k == sig);
            *self.error_counts.entry(sig).or_insert(0) += 1;
        }
    }

    /// Record a tool round that changed no files (read-only tools, bash probes, ...).
    pub fn record_stagnant_round(&mut self) {
        self.stagnant_rounds += 1;
    }

    /// Check for a loop. Returns the fired pattern at most once; after that the
    /// caller is expected to stop the task and re-plan, then call [`Self::reset`].
    pub fn check(&mut self) -> Option<LoopPattern> {
        if self.triggered {
            return None;
        }
        // (a) same file, failing edits
        if let Some((path, &count)) = self
            .failing_edits
            .iter()
            .find(|(_, &c)| c >= self.policy.max_same_file_edits)
        {
            self.triggered = true;
            return Some(LoopPattern::SameFileFailingEdits {
                path: path.clone(),
                attempts: count,
            });
        }
        // (b) repeating error signature
        if let Some((sig, &count)) = self
            .error_counts
            .iter()
            .find(|(_, &c)| c >= self.policy.max_same_error)
        {
            self.triggered = true;
            return Some(LoopPattern::RepeatingError {
                signature: sig.clone(),
                attempts: count,
            });
        }
        // (c) stagnation
        if self.stagnant_rounds >= self.policy.max_stagnant_rounds {
            self.triggered = true;
            return Some(LoopPattern::StagnantRounds {
                rounds: self.stagnant_rounds,
            });
        }
        None
    }

    /// Start tracking fresh (e.g. after the agent re-planned with a new approach).
    pub fn reset(&mut self) {
        self.failing_edits.clear();
        self.error_counts.clear();
        self.stagnant_rounds = 0;
        self.history.clear();
        self.pending_paths.clear();
        self.triggered = false;
    }

    /// Build the forced re-plan prompt for the agent: what was tried, what failed,
    /// and the instruction to try something DIFFERENT. Mentions the restore point
    /// when the caller supplies one.
    pub fn replan_prompt(&self, pattern: &LoopPattern, restore_point: Option<&str>) -> String {
        let mut out = String::from(
            "CONVERGENCE GUARD: I stopped your current approach because it is looping.\n\n",
        );
        out.push_str(&format!("Pattern detected: {}.\n\n", pattern.describe()));
        out.push_str("What was tried:\n");
        let recent: Vec<&AttemptRecord> = self.history.iter().rev().take(6).collect();
        if recent.is_empty() {
            out.push_str("  - (no edit history recorded)\n");
        }
        for rec in recent.into_iter().rev() {
            if rec.verify_passed {
                out.push_str("  - a verify step passed (approach worked at that point)\n");
            } else {
                out.push_str(&format!("  - edit '{}'", rec.path));
                if let Some(err) = &rec.error_preview {
                    out.push_str(&format!(" -> verify FAILED: {}", first_line(err)));
                } else {
                    out.push_str(" -> verify FAILED");
                }
                out.push('\n');
            }
        }
        if let Some(id) = restore_point {
            out.push_str(&format!(
                "\nA checkpoint '{id}' was saved before the loop started; you can restore \
                 the workspace to it instead of untangling the failed edits.\n"
            ));
        }
        out.push_str(
            "\nYou must now propose a DIFFERENT approach. Do NOT retry the same edit, \
             do NOT re-run the same failing command. First explain, in 2-3 sentences, \
             what you think the real root cause is and what you will do differently, \
             then act.",
        );
        out
    }

    /// Emit the `loop_detected` system event to the session log (keeps the
    /// `DesktopEvent` schema untouched, like `verify_attempt` in PR #32).
    pub fn emit_loop_detected(
        &self,
        pattern: &LoopPattern,
        restore_point: Option<&str>,
        sink: &dyn EventSink,
    ) -> anyhow::Result<()> {
        let attempts = match pattern {
            LoopPattern::SameFileFailingEdits { attempts, .. } => *attempts,
            LoopPattern::RepeatingError { attempts, .. } => *attempts,
            LoopPattern::StagnantRounds { rounds } => *rounds,
        };
        let mut files: Vec<String> = self.failing_edits.keys().cloned().collect();
        files.sort();
        let payload = LoopDetectedPayload {
            pattern: pattern.clone(),
            attempts,
            files,
            restore_point: restore_point.map(str::to_string),
        };
        let text = serde_json::to_string(&payload)?;
        sink.emit(AgentEvent::SystemMessage {
            event: LOOP_DETECTED_EVENT.to_string(),
            text,
        });
        Ok(())
    }
}

/// Normalize a path for comparison: trim whitespace, unify separators.
fn normalize_path(p: &str) -> String {
    p.trim().replace('\\', "/")
}

/// Normalize an error into a repeatable signature: first non-empty line,
/// lowercased, digit runs and hex hashes folded, truncated.
fn normalize_error(raw: &str) -> String {
    let first = raw
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let mut sig = String::with_capacity(first.len());
    let mut in_digits = false;
    for ch in first.chars() {
        if ch.is_ascii_digit() {
            if !in_digits {
                sig.push('#');
                in_digits = true;
            }
        } else {
            in_digits = false;
            sig.push(ch.to_ascii_lowercase());
        }
    }
    truncate(&sig, 160)
}

fn truncate(s: &str, limit: usize) -> String {
    if s.len() <= limit {
        s.to_string()
    } else {
        // Cut on a char boundary.
        let mut end = limit;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &s[..end])
    }
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("").trim()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zen_core::EventSink;

    fn detector() -> LoopDetector {
        LoopDetector::new(ConvergencePolicy::default()).unwrap()
    }

    #[test]
    fn detects_repeated_failing_edits_on_same_file() {
        let mut d = detector();
        for _ in 0..3 {
            d.record_edit("src/main.rs");
            d.record_verify(false, Some("test foo FAILED"));
        }
        match d.check() {
            Some(LoopPattern::SameFileFailingEdits { path, attempts }) => {
                assert_eq!(path, "src/main.rs");
                assert_eq!(attempts, 3);
            }
            other => panic!("expected SameFileFailingEdits, got {other:?}"),
        }
    }

    #[test]
    fn detects_repeating_error_signature() {
        let mut d = detector();
        // Different files, same underlying error -> pattern (b) fires.
        let files = ["a.rs", "b.rs", "c.rs"];
        for f in files {
            d.record_edit(f);
            d.record_verify(
                false,
                Some("error[E0308]: mismatched types\n  --> src/lib.rs:42"),
            );
        }
        match d.check() {
            Some(LoopPattern::RepeatingError {
                signature,
                attempts,
            }) => {
                assert_eq!(attempts, 3);
                assert!(signature.contains("mismatched types"), "sig: {signature}");
            }
            other => panic!("expected RepeatingError, got {other:?}"),
        }
    }

    #[test]
    fn no_false_positive_on_two_attempts() {
        let mut d = detector();
        for _ in 0..2 {
            d.record_edit("src/main.rs");
            d.record_verify(false, Some("boom"));
        }
        assert_eq!(d.check(), None);
        // Two stagnant rounds is also fine (default is 5).
        d.record_stagnant_round();
        d.record_stagnant_round();
        assert_eq!(d.check(), None);
    }

    #[test]
    fn passing_verify_resets_counters() {
        let mut d = detector();
        for _ in 0..2 {
            d.record_edit("src/main.rs");
            d.record_verify(false, Some("boom"));
        }
        d.record_edit("src/main.rs");
        d.record_verify(true, None);
        assert_eq!(d.check(), None);
    }

    #[test]
    fn stagnant_rounds_trigger_and_edit_resets() {
        let mut d = detector();
        for _ in 0..4 {
            d.record_stagnant_round();
        }
        assert_eq!(d.check(), None);
        // An edit is progress: stagnation counter resets.
        d.record_edit("src/main.rs");
        for _ in 0..5 {
            d.record_stagnant_round();
        }
        match d.check() {
            Some(LoopPattern::StagnantRounds { rounds }) => assert_eq!(rounds, 5),
            other => panic!("expected StagnantRounds, got {other:?}"),
        }
    }

    #[test]
    fn check_fires_only_once_until_reset() {
        let mut d = detector();
        for _ in 0..3 {
            d.record_edit("src/main.rs");
            d.record_verify(false, Some("boom"));
        }
        assert!(d.check().is_some());
        assert_eq!(d.check(), None);
        d.reset();
        for _ in 0..3 {
            d.record_edit("src/main.rs");
            d.record_verify(false, Some("boom"));
        }
        assert!(d.check().is_some());
    }

    #[test]
    fn replan_prompt_contains_attempt_history() {
        let mut d = detector();
        for _ in 0..3 {
            d.record_edit("src/broken.rs");
            d.record_verify(false, Some("error[E0308]: mismatched types"));
        }
        let pattern = d.check().expect("loop should fire");
        let prompt = d.replan_prompt(&pattern, Some("abc1234"));
        assert!(prompt.contains("src/broken.rs"), "prompt: {prompt}");
        assert!(prompt.contains("mismatched types"), "prompt: {prompt}");
        assert!(prompt.contains("abc1234"), "prompt: {prompt}");
        assert!(prompt.contains("DIFFERENT"), "prompt: {prompt}");
    }

    #[test]
    fn replan_prompt_without_restore_point() {
        let mut d = detector();
        for _ in 0..5 {
            d.record_stagnant_round();
        }
        let pattern = d.check().expect("loop should fire");
        let prompt = d.replan_prompt(&pattern, None);
        assert!(!prompt.contains("checkpoint"), "prompt: {prompt}");
        assert!(prompt.contains("no progress"), "prompt: {prompt}");
    }

    #[test]
    fn emit_loop_detected_writes_system_event() {
        struct VecSink(std::sync::Mutex<Vec<AgentEvent>>);
        impl EventSink for VecSink {
            fn emit(&self, ev: AgentEvent) {
                self.0.lock().unwrap().push(ev);
            }
        }
        let sink = VecSink(std::sync::Mutex::new(Vec::new()));
        let mut d = detector();
        for _ in 0..3 {
            d.record_edit("src/main.rs");
            d.record_verify(false, Some("boom"));
        }
        let pattern = d.check().expect("loop should fire");
        d.emit_loop_detected(&pattern, Some("abc1234"), &sink)
            .unwrap();
        let events = sink.0.lock().unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            AgentEvent::SystemMessage { event, text } => {
                assert_eq!(event, LOOP_DETECTED_EVENT);
                let payload: LoopDetectedPayload = serde_json::from_str(text).unwrap();
                assert_eq!(payload.attempts, 3);
                assert_eq!(payload.restore_point.as_deref(), Some("abc1234"));
                assert_eq!(payload.files, vec!["src/main.rs".to_string()]);
                assert!(matches!(
                    payload.pattern,
                    LoopPattern::SameFileFailingEdits { .. }
                ));
            }
            other => panic!("expected SystemMessage, got {other:?}"),
        }
    }

    #[test]
    fn error_normalization_folds_numbers() {
        // Same error at different line numbers must produce one signature.
        let mut d = detector();
        let files = ["a.rs", "b.rs", "c.rs"];
        for (i, f) in files.iter().enumerate() {
            d.record_edit(f);
            d.record_verify(
                false,
                Some(&format!(
                    "error[E0308]: mismatched types\n  --> src/lib.rs:{}",
                    40 + i
                )),
            );
        }
        match d.check() {
            Some(LoopPattern::RepeatingError { attempts, .. }) => assert_eq!(attempts, 3),
            other => panic!("expected RepeatingError, got {other:?}"),
        }
    }

    #[test]
    fn policy_validation_rejects_zeros() {
        let bad = ConvergencePolicy {
            max_same_file_edits: 0,
            ..ConvergencePolicy::default()
        };
        assert!(LoopDetector::new(bad).is_err());
    }
}
