use std::path::Path;

/// How strongly a sandbox actually isolates. Reported honestly, never inflated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Isolation {
    /// No OS-level isolation at all.
    None,
    /// Some restrictions applied; the string says what is missing (e.g. old kernel).
    Partial(String),
    /// Full isolation as designed for this platform.
    Full,
}

/// Policy + enforcement seam. Fail-closed: anything not explicitly allowed is denied.
pub trait Sandbox {
    fn isolation(&self) -> Isolation;
    fn allow_read(&self, path: &Path) -> bool;
    fn allow_write(&self, path: &Path) -> bool;
    fn allow_exec(&self, program: &str) -> bool;
}

/// Development placeholder: allows nothing and admits it isolates nothing.
/// Fail-closed by default so no tool can act until a real sandbox is wired in.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoSandbox;

impl Sandbox for NoSandbox {
    fn isolation(&self) -> Isolation {
        Isolation::None
    }
    fn allow_read(&self, _: &Path) -> bool {
        false
    }
    fn allow_write(&self, _: &Path) -> bool {
        false
    }
    fn allow_exec(&self, _: &str) -> bool {
        false
    }
}
