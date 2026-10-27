use crate::types::{Error, Message, Role, ToolCall};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {

    Context(Message),

    Call(ToolCall),

    Error(String),
}

impl Event {

    pub fn as_context(&self) -> Option<&Message> {
        match self {
            Event::Context(m) => Some(m),
            _ => None,
        }
    }
}

pub trait SessionLog {
    fn append(&mut self, event: Event) -> Result<(), Error>;
    fn events(&self) -> &[Event];

    fn replay_context(&self) -> Vec<Message> {
        self.events()
            .iter()
            .filter_map(Event::as_context)
            .cloned()
            .collect()
    }
}

#[derive(Debug, Default, Clone)]
pub struct MemoryLog {
    events: Vec<Event>,
}

impl MemoryLog {
    pub fn new() -> Self {
        Self::default()
    }

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
