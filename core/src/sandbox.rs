use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Isolation {
    None,

    Partial(String),

    Full,
}

pub trait Sandbox {
    fn isolation(&self) -> Isolation;
    fn allow_read(&self, path: &Path) -> bool;
    fn allow_write(&self, path: &Path) -> bool;
    fn allow_exec(&self, program: &str) -> bool;
}

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
