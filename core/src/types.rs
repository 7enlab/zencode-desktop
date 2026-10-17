use crate::sandbox::Sandbox;
use std::fmt;

/// Who authored a message in the model's context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }
}

/// One entry of the model-visible context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Message {
            role,
            content: content.into(),
        }
    }
}

/// A tool invocation requested by the model. `input` is raw (usually JSON) text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub name: String,
    pub input: String,
}

/// What the model answered for one step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// Final text for the user; ends the turn.
    Text(String),
    /// The model wants a tool run; the loop feeds the result back.
    Call(ToolCall),
}

/// Description of a tool as advertised to the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
}

/// Errors surfaced by the core. Never swallowed: the loop logs and returns them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Llm(String),
    Tool(String),
    Denied(String),
    Log(String),
    StepLimit(usize),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Llm(m) => write!(f, "llm error: {m}"),
            Error::Tool(m) => write!(f, "tool error: {m}"),
            Error::Denied(m) => write!(f, "sandbox denied: {m}"),
            Error::Log(m) => write!(f, "session log error: {m}"),
            Error::StepLimit(n) => write!(f, "step limit reached ({n})"),
        }
    }
}

impl std::error::Error for Error {}

/// A chat model. Implementations must be pure with respect to `context`:
/// whatever they send to the provider must come from `context` and `tools`,
/// so that the session log (which records exactly those) stays complete.
pub trait Llm {
    fn complete(&mut self, context: &[Message], tools: &[ToolSpec]) -> Result<Reply, Error>;
}

/// A callable tool. Tools receive the sandbox and must ask it before side effects.
pub trait Tool {
    fn spec(&self) -> ToolSpec;
    fn call(&mut self, input: &str, sandbox: &dyn Sandbox) -> Result<String, Error>;
}
