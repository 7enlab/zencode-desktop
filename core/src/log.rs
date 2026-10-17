use crate::types::{Error, Message, Role, ToolCall};

/// One append-only record of the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A message entered the model-visible context.
    Context(Message),
    /// The model requested a tool call.
    Call(ToolCall),
    /// An error that ended or interrupted the turn.
    Error(String),
}

impl Event {
    /// The message this event adds to the model's context, if any.
    pub fn as_context(&self) -> Option<&Message> {
        match self {
            Event::Context(m) => Some(m),
            _ => None,
        }
    }
}

/// Append-only session log. No update or delete: history is rebuilt by replay.
pub trait SessionLog {
    fn append(&mut self, event: Event) -> Result<(), Error>;
    fn events(&self) -> &[Event];

    /// Rebuild the exact model-visible context from the log.
    fn replay_context(&self) -> Vec<Message> {
        self.events()
            .iter()
            .filter_map(Event::as_context)
            .cloned()
            .collect()
    }
}

/// In-memory log, used by tests and as the base for the JSONL log.
#[derive(Debug, Default, Clone)]
pub struct MemoryLog {
    events: Vec<Event>,
}

impl MemoryLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fork: a new log holding the first `upto` events (clamped), for branching a session.
    pub fn fork(&self, upto: usize) -> MemoryLog {
        let n = upto.min(self.events.len());
        MemoryLog {
            events: self.events[..n].to_vec(),
        }
    }
}

impl SessionLog for MemoryLog {
    fn append(&mut self, event: Event) -> Result<(), Error> {
        self.events.push(event);
        Ok(())
    }
    fn events(&self) -> &[Event] {
        &self.events
    }
}

/// Helper used by the loop: log a message and push it into the live context together,
/// so the "model-visible means logged" invariant holds by construction.
pub(crate) fn see(
    log: &mut dyn SessionLog,
    context: &mut Vec<Message>,
    role: Role,
    content: String,
) -> Result<(), Error> {
    let m = Message::new(role, content);
    log.append(Event::Context(m.clone()))?;
    context.push(m);
    Ok(())
}
