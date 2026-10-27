use crate::sandbox::Sandbox;
use std::fmt;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub name: String,
    pub input: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    Text(String),

    Call(ToolCall),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
}

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

pub trait Llm {
    fn complete(&mut self, context: &[Message], tools: &[ToolSpec]) -> Result<Reply, Error>;
}

pub trait Tool {
    fn spec(&self) -> ToolSpec;
    fn call(&mut self, input: &str, sandbox: &dyn Sandbox) -> Result<String, Error>;
}
