//! ZenCode Desktop agent core (std-only).
//!
//! Four seams, each a trait so backends can be swapped without touching the loop:
//! - [`Llm`]: a chat model (OpenAI-compatible free channel later; scripted fakes in tests).
//! - [`Tool`]: something the model may call (shell, str_replace, ...).
//! - [`Sandbox`]: decides what a tool may touch and reports its isolation honestly.
//! - [`SessionLog`]: append-only record; invariant: **everything the model sees is logged**.
//!
//! [`run_turn`] is the minimal agent loop wiring them together.

mod agent;
mod log;
mod sandbox;
mod types;

pub use agent::{run_turn, TurnOutcome};
pub use log::{Event, MemoryLog, SessionLog};
pub use sandbox::{Isolation, NoSandbox, Sandbox};
pub use types::{Error, Llm, Message, Reply, Role, Tool, ToolCall, ToolSpec};
