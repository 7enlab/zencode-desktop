//! R3 Ask-First + Scope Guard (v0.4.0 "Trust" roadmap).
//!
//! Before acting, the agent DECLARES the files/dirs it will touch
//! ([`ScopeDecl`]). [`ScopeGuard`] implements [`PermissionChecker`]: every
//! Write/Edit/MultiEdit/Bash call is checked against the declared scope;
//! anything outside is BLOCKED with a loud denial and a `scope_violation`
//! system event. Ambiguous declarations (wildcards, `..`, absolute paths
//! outside the project root, more than [`MAX_AUTO_FILES`] files) require
//! explicit approval BEFORE the agent starts ([`needs_approval`]).
//!
//! Merge note: PR #36 (plan mode) and PR #54 (approval flow / ApprovalGate)
//! are not on main yet. When they land:
//!   - feed [`needs_approval`] results into the approval card UI;
//!   - let plan mode populate [`ScopeDecl`] automatically from the plan;
//!   - keep this module as the enforcement point (it owns the deny path).

use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use zen_core::{AgentEvent, EventSink, PermissionChecker, PermissionDecision, ToolContext};

/// Declarations touching more than this many files need explicit approval.
pub const MAX_AUTO_FILES: usize = 20;

/// Patterns that are never allowed in Bash, scope or not.
const DESTRUCTIVE_PATTERNS: &[&str] = &[
    "rm -rf /",
    "rm -rf /*",
    "rm -fr /",
    "sudo ",
    "mkfs",
    "dd if=",
    "> /dev/",
    "chmod -R /",
    ":(){",
];

/// What the agent declares it will touch before acting.
#[derive(Clone, Debug, Default)]
pub struct ScopeDecl {
    pub files: Vec<PathBuf>,
    pub dirs: Vec<PathBuf>,
    pub description: String,
}

/// Why a declaration needs explicit approval before the agent starts.
#[derive(Clone, Debug, PartialEq)]
pub enum ApprovalReason {
    WildcardPattern(String),
    PathEscape(String),
    OutsideRoot(String),
    TooManyFiles(usize),
}

impl ApprovalReason {
    pub fn message(&self) -> String {
        match self {
            ApprovalReason::WildcardPattern(p) => {
                format!("declaration contains wildcard pattern '{p}' - confirm the real file list")
            }
            ApprovalReason::PathEscape(p) => {
                format!("declaration contains path escape '{p}' - confirm this is intentional")
            }
            ApprovalReason::OutsideRoot(p) => {
                format!("declaration contains path outside the project root: '{p}'")
            }
            ApprovalReason::TooManyFiles(n) => {
                format!(
                    "declaration touches {n} files (limit {MAX_AUTO_FILES}) - confirm the scope"
                )
            }
        }
    }
}

/// Check whether a declaration is ambiguous/risky enough to require
/// explicit approval BEFORE the agent starts. Returns the reasons
/// (empty = auto-approved).
pub fn needs_approval(decl: &ScopeDecl, root: &Path) -> Vec<ApprovalReason> {
    let mut reasons = Vec::new();
    let mut count = 0;
    for p in decl.files.iter().chain(decl.dirs.iter()) {
        count += 1;
        let s = p.to_string_lossy();
        if s.contains('*') || s.contains('?') {
            reasons.push(ApprovalReason::WildcardPattern(s.to_string()));
        }
        if has_escape(p) {
            reasons.push(ApprovalReason::PathEscape(s.to_string()));
        }
        if p.is_absolute() && !p.starts_with(root) {
            reasons.push(ApprovalReason::OutsideRoot(s.to_string()));
        }
    }
    if count > MAX_AUTO_FILES {
        reasons.push(ApprovalReason::TooManyFiles(count));
    }
    reasons
}

/// True if the path contains `..` (after normalization it would escape).
fn has_escape(p: &Path) -> bool {
    p.components().any(|c| matches!(c, Component::ParentDir))
}

/// Resolve `target` (absolute or relative to `root`) to a normalized
/// absolute path. Returns `None` when it escapes the root.
fn resolve(root: &Path, target: &str) -> Option<PathBuf> {
    let joined = if Path::new(target).is_absolute() {
        PathBuf::from(target)
    } else {
        root.join(target)
    };
    let mut norm = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::ParentDir => {
                if !norm.pop() {
                    return None;
                }
            }
            Component::CurDir => {}
            _ => norm.push(c.as_os_str()),
        }
    }
    if norm.starts_with(root) {
        Some(norm)
    } else {
        None
    }
}

/// A blocked out-of-scope attempt (also emitted as a `scope_violation`
/// system event).
#[derive(Clone, Debug)]
pub struct ScopeViolation {
    pub tool: String,
    pub target: String,
    pub reason: String,
}

/// Guards tool calls against the declared scope. Construct with
/// [`ScopeGuard::new`]; read-only tools always pass.
pub struct ScopeGuard {
    root: PathBuf,
    scope: Mutex<ScopeDecl>,
    sink: Arc<dyn EventSink>,
    violations: Mutex<Vec<ScopeViolation>>,
}

impl ScopeGuard {
    pub fn new(root: PathBuf, decl: ScopeDecl, sink: Arc<dyn EventSink>) -> Self {
        Self {
            root,
            scope: Mutex::new(decl),
            sink,
            violations: Mutex::new(Vec::new()),
        }
    }

    /// All recorded violations (for tests / session summaries).
    pub fn violations(&self) -> Vec<ScopeViolation> {
        self.violations.lock().unwrap().clone()
    }

    /// Approve a scope expansion: the new paths are added and the
    /// expansion is logged as a `scope_expanded` system event.
    /// Returns the number of paths added.
    pub fn expand_scope(&self, files: &[PathBuf], dirs: &[PathBuf]) -> usize {
        let mut scope = self.scope.lock().unwrap();
        let mut added = 0;
        for f in files {
            if !scope.files.contains(f) {
                scope.files.push(f.clone());
                added += 1;
            }
        }
        for d in dirs {
            if !scope.dirs.contains(d) {
                scope.dirs.push(d.clone());
                added += 1;
            }
        }
        drop(scope);
        self.emit(
            "scope_expanded",
            &format!("scope expanded: {added} new path(s) approved by the user"),
        );
        added
    }

    /// Is `target` (as written in a tool call) inside the declared scope?
    pub fn in_scope(&self, target: &str) -> bool {
        let Some(abs) = resolve(&self.root, target) else {
            return false;
        };
        let scope = self.scope.lock().unwrap();
        if scope
            .files
            .iter()
            .any(|f| resolve(&self.root, &f.to_string_lossy()) == Some(abs.clone()))
        {
            return true;
        }
        scope.dirs.iter().any(|d| {
            let pat = d.to_string_lossy();
            // Approved wildcard "dir/*" behaves as a dir prefix.
            let prefix = pat.strip_suffix("/*").unwrap_or(&pat);
            match resolve(&self.root, prefix) {
                Some(dir_abs) => abs.starts_with(&dir_abs),
                None => false,
            }
        })
    }

    fn deny(&self, tool: &str, target: &str, reason: &str) -> PermissionDecision {
        let v = ScopeViolation {
            tool: tool.to_string(),
            target: target.to_string(),
            reason: reason.to_string(),
        };
        self.violations.lock().unwrap().push(v);
        self.emit(
            "scope_violation",
            &format!("agent tried to {tool} '{target}' - not in declared scope ({reason})"),
        );
        PermissionDecision::Deny(format!(
            "Blocked by scope guard: {tool} '{target}' is outside the declared scope. {reason}"
        ))
    }

    fn emit(&self, event: &str, text: &str) {
        self.sink.emit(AgentEvent::SystemMessage {
            event: event.to_string(),
            text: text.to_string(),
        });
    }

    fn check_path(&self, tool: &str, target: &str) -> PermissionDecision {
        if target.is_empty() {
            return self.deny(tool, "(empty)", "no target path given");
        }
        if has_escape(Path::new(target)) && resolve(&self.root, target).is_none() {
            return self.deny(tool, target, "path escapes the project root");
        }
        if self.in_scope(target) {
            PermissionDecision::Allow
        } else {
            self.deny(tool, target, "declare it first with an expanded scope")
        }
    }

    fn check_bash(&self, cmd: &str) -> PermissionDecision {
        let lower = cmd.to_lowercase();
        for pat in DESTRUCTIVE_PATTERNS {
            if lower.contains(pat) {
                return self.deny(
                    "Bash",
                    cmd,
                    &format!("destructive pattern '{pat}' is never allowed"),
                );
            }
        }
        // Mutating commands get their path-like tokens checked against scope.
        const MUTATING: &[&str] = &[
            "rm ", "mv ", "cp ", "mkdir ", "touch ", "tee ", "sed -i", ">", ">>", "chmod ",
            "chown ",
        ];
        if !MUTATING.iter().any(|m| lower.contains(m)) {
            return PermissionDecision::Allow; // read-only-ish command
        }
        for token in cmd.split_whitespace() {
            let t = token.trim_matches(|c| c == '"' || c == '\'' || c == ';' || c == '&');
            if t.is_empty() || t.starts_with('-') {
                continue;
            }
            let looks_like_path = t.contains('/')
                || t.contains('\\')
                || (t.contains('.') && !t.parse::<f64>().is_ok());
            if !looks_like_path {
                continue;
            }
            if has_escape(Path::new(t)) && resolve(&self.root, t).is_none() {
                return self.deny(
                    "Bash",
                    cmd,
                    &format!("path token '{t}' escapes the project root"),
                );
            }
            if !self.in_scope(t) {
                return self.deny(
                    "Bash",
                    cmd,
                    &format!("path token '{t}' is outside the declared scope"),
                );
            }
        }
        PermissionDecision::Allow
    }
}

impl PermissionChecker for ScopeGuard {
    fn check(
        &self,
        tool_name: &str,
        input: &Value,
        read_only: bool,
        _ctx: &ToolContext,
    ) -> PermissionDecision {
        if read_only {
            return PermissionDecision::Allow;
        }
        match tool_name {
            "Read" | "Glob" | "Grep" => PermissionDecision::Allow,
            "Write" | "Edit" | "MultiEdit" => {
                let target = input
                    .get("file_path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                self.check_path(tool_name, target)
            }
            "Bash" => {
                let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
                self.check_bash(cmd)
            }
            _ => PermissionDecision::Allow,
        }
    }
}

/// Emit a `scope_declared` system event describing the active scope
/// (call once when the run starts).
pub fn emit_scope_declared(sink: &Arc<dyn EventSink>, decl: &ScopeDecl) {
    let files: Vec<String> = decl.files.iter().map(|p| p.display().to_string()).collect();
    let dirs: Vec<String> = decl.dirs.iter().map(|p| p.display().to_string()).collect();
    sink.emit(AgentEvent::SystemMessage {
        event: "scope_declared".to_string(),
        text: format!(
            "agent declared scope: {} - files: [{}], dirs: [{}]",
            decl.description,
            files.join(", "),
            dirs.join(", ")
        ),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use zen_core::NullSink;

    fn guard() -> ScopeGuard {
        let decl = ScopeDecl {
            files: vec![PathBuf::from("src/main.rs")],
            dirs: vec![PathBuf::from("docs")],
            description: "fix the typo".to_string(),
        };
        ScopeGuard::new(PathBuf::from("/proj"), decl, Arc::new(NullSink))
    }

    fn ctx() -> ToolContext {
        ToolContext::new(PathBuf::from("/proj"), "test")
    }

    #[test]
    fn in_scope_write_passes() {
        let g = guard();
        let d = g.check(
            "Write",
            &json!({"file_path": "src/main.rs", "content": "x"}),
            false,
            &ctx(),
        );
        assert_eq!(d, PermissionDecision::Allow);
    }

    #[test]
    fn in_scope_dir_passes() {
        let g = guard();
        let d = g.check(
            "Write",
            &json!({"file_path": "docs/guide.md", "content": "x"}),
            false,
            &ctx(),
        );
        assert_eq!(d, PermissionDecision::Allow);
    }

    #[test]
    fn out_of_scope_blocked_and_logged() {
        let g = guard();
        let d = g.check(
            "Edit",
            &json!({"file_path": "src/other.rs", "old_string": "a", "new_string": "b"}),
            false,
            &ctx(),
        );
        assert!(matches!(d, PermissionDecision::Deny(_)));
        let v = g.violations();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].tool, "Edit");
        assert!(v[0].target.contains("other.rs"));
    }

    #[test]
    fn path_escape_blocked() {
        let g = guard();
        let d = g.check(
            "Write",
            &json!({"file_path": "../secret.txt", "content": "x"}),
            false,
            &ctx(),
        );
        assert!(matches!(d, PermissionDecision::Deny(_)));
    }

    #[test]
    fn read_always_passes() {
        let g = guard();
        let d = g.check("Read", &json!({"file_path": "/etc/passwd"}), false, &ctx());
        assert_eq!(d, PermissionDecision::Allow);
    }

    #[test]
    fn destructive_bash_blocked() {
        let g = guard();
        let d = g.check(
            "Bash",
            &json!({"command": "rm -rf /tmp/cache"}),
            false,
            &ctx(),
        );
        // "rm -rf /tmp/cache" is not the exact "rm -rf /" pattern; path check applies.
        // /tmp is outside scope -> denied via scope.
        assert!(matches!(d, PermissionDecision::Deny(_)));
        let d2 = g.check("Bash", &json!({"command": "rm -rf /"}), false, &ctx());
        assert!(matches!(d2, PermissionDecision::Deny(_)));
    }

    #[test]
    fn readonly_bash_passes() {
        let g = guard();
        let d = g.check("Bash", &json!({"command": "cargo test"}), false, &ctx());
        assert_eq!(d, PermissionDecision::Allow);
    }

    #[test]
    fn expansion_allows_new_file() {
        let g = guard();
        assert_eq!(g.expand_scope(&[PathBuf::from("src/new.rs")], &[]), 1);
        let d = g.check(
            "Write",
            &json!({"file_path": "src/new.rs", "content": "x"}),
            false,
            &ctx(),
        );
        assert_eq!(d, PermissionDecision::Allow);
    }

    #[test]
    fn needs_approval_flags_risky_decls() {
        let root = Path::new("/proj");
        let wild = ScopeDecl {
            files: vec![PathBuf::from("src/*.rs")],
            dirs: vec![],
            description: "x".into(),
        };
        let r = needs_approval(&wild, root);
        assert!(r
            .iter()
            .any(|x| matches!(x, ApprovalReason::WildcardPattern(_))));

        let esc = ScopeDecl {
            files: vec![PathBuf::from("../x")],
            dirs: vec![],
            description: "x".into(),
        };
        assert!(needs_approval(&esc, root)
            .iter()
            .any(|x| matches!(x, ApprovalReason::PathEscape(_))));

        let outside = ScopeDecl {
            files: vec![PathBuf::from("/etc/hosts")],
            dirs: vec![],
            description: "x".into(),
        };
        assert!(needs_approval(&outside, root)
            .iter()
            .any(|x| matches!(x, ApprovalReason::OutsideRoot(_))));

        let many = ScopeDecl {
            files: (0..25).map(|i| PathBuf::from(format!("f{i}.rs"))).collect(),
            dirs: vec![],
            description: "x".into(),
        };
        assert!(needs_approval(&many, root)
            .iter()
            .any(|x| matches!(x, ApprovalReason::TooManyFiles(25))));

        let clean = ScopeDecl {
            files: vec![PathBuf::from("a.rs")],
            dirs: vec![PathBuf::from("docs")],
            description: "x".into(),
        };
        assert!(needs_approval(&clean, root).is_empty());
    }
}
