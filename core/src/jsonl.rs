//! Append-only JSONL session log on disk (std-only JSON for flat string objects).
use crate::log::{Event, SessionLog};
use crate::types::{Error, Message, Role, ToolCall};
use std::collections::hash_map::DefaultHasher;
use std::fs::{File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::Path;

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn encode(e: &Event) -> String {
    match e {
        Event::Context(m) => format!(
            "{{\"t\":\"ctx\",\"role\":{},\"content\":{}}}",
            esc(m.role.as_str()),
            esc(&m.content)
        ),
        Event::Call(c) => format!(
            "{{\"t\":\"call\",\"name\":{},\"input\":{}}}",
            esc(&c.name),
            esc(&c.input)
        ),
        Event::Error(m) => format!("{{\"t\":\"err\",\"msg\":{}}}", esc(m)),
    }
}

/// Parse a flat JSON object whose values are all strings.
fn parse_flat(line: &str) -> Option<Vec<(String, String)>> {
    let mut it = line.trim().chars().peekable();
    if it.next()? != '{' {
        return None;
    }
    let mut out = Vec::new();
    loop {
        match it.peek()? {
            '}' => {
                it.next();
                break;
            }
            ',' | ' ' => {
                it.next();
            }
            '"' => {
                let k = parse_str(&mut it)?;
                if it.next()? != ':' {
                    return None;
                }
                if *it.peek()? != '"' {
                    return None;
                }
                let v = parse_str(&mut it)?;
                out.push((k, v));
            }
            _ => return None,
        }
    }
    if it.next().is_some() {
        return None;
    }
    Some(out)
}

fn hex4(it: &mut std::iter::Peekable<std::str::Chars>) -> Option<u32> {
    let mut n = 0;
    for _ in 0..4 {
        n = n * 16 + it.next()?.to_digit(16)?;
    }
    Some(n)
}

fn parse_str(it: &mut std::iter::Peekable<std::str::Chars>) -> Option<String> {
    if it.next()? != '"' {
        return None;
    }
    let mut s = String::new();
    loop {
        match it.next()? {
            '"' => return Some(s),
            '\\' => match it.next()? {
                '"' => s.push('"'),
                '\\' => s.push('\\'),
                '/' => s.push('/'),
                'n' => s.push('\n'),
                'r' => s.push('\r'),
                't' => s.push('\t'),
                'u' => {
                    let mut cp = hex4(it)?;
                    if (0xD800..0xDC00).contains(&cp) {
                        if it.next()? != '\\' || it.next()? != 'u' {
                            return None;
                        }
                        let lo = hex4(it)?;
                        if !(0xDC00..0xE000).contains(&lo) {
                            return None;
                        }
                        cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                    }
                    s.push(char::from_u32(cp)?);
                }
                _ => return None,
            },
            c => s.push(c),
        }
    }
}

fn decode(line: &str) -> Option<Event> {
    let f = parse_flat(line)?;
    let get = |k: &str| f.iter().find(|(a, _)| a == k).map(|(_, v)| v.clone());
    match get("t")?.as_str() {
        "ctx" => {
            let role = match get("role")?.as_str() {
                "system" => Role::System,
                "user" => Role::User,
                "assistant" => Role::Assistant,
                "tool" => Role::Tool,
                _ => return None,
            };
            Some(Event::Context(Message::new(role, get("content")?)))
        }
        "call" => Some(Event::Call(ToolCall {
            name: get("name")?,
            input: get("input")?,
        })),
        "err" => Some(Event::Error(get("msg")?)),
        _ => None,
    }
}

/// Session log persisted as one JSON object per line. Each append is a single
/// `write_all` of a full line and is flushed, so a crash can only truncate the last line.
pub struct JsonlLog {
    file: File,
    events: Vec<Event>,
}

impl JsonlLog {
    /// Open (creating if needed) and load existing events. A corrupt *final* line
    /// (interrupted write) is ignored; a corrupt line elsewhere is an error.
    pub fn open(path: &Path) -> Result<Self, Error> {
        let text = match std::fs::read(path) {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(Error::Log(format!("read {}: {e}", path.display()))),
        };
        let mut events = Vec::new();
        let mut good_end = 0usize; // byte offset just after the last valid line
        let mut pos = 0usize;
        let mut bad: Option<usize> = None;
        for (n, raw) in text.split_inclusive('\n').enumerate() {
            let end = pos + raw.len();
            pos = end;
            if raw.trim().is_empty() {
                continue;
            }
            if let Some(b) = bad {
                return Err(Error::Log(format!("corrupt line {} in log", b + 1)));
            }
            match decode(raw) {
                Some(e) => {
                    events.push(e);
                    good_end = end;
                }
                None => bad = Some(n),
            }
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| Error::Log(format!("open {}: {e}", path.display())))?;
        if bad.is_some() {
            // Interrupted final write: cut the torn tail so later appends stay valid.
            // Separate write handle: Windows append-only handles cannot truncate.
            OpenOptions::new()
                .write(true)
                .open(path)
                .and_then(|f| f.set_len(good_end as u64))
                .map_err(|e| Error::Log(e.to_string()))?;
        }
        Ok(JsonlLog { file, events })
    }
}

impl SessionLog for JsonlLog {
    fn append(&mut self, event: Event) -> Result<(), Error> {
        let mut line = encode(&event);
        line.push('\n');
        self.file
            .write_all(line.as_bytes())
            .and_then(|_| self.file.flush())
            .map_err(|e| Error::Log(e.to_string()))?;
        self.events.push(event);
        Ok(())
    }
    fn events(&self) -> &[Event] {
        &self.events
    }
}

/// Keep only the first of any instruction files (AGENTS.md, CLAUDE.md, ...) with
/// identical content (ignoring surrounding whitespace), so it is injected once.
pub fn dedupe_instructions(files: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for (p, c) in files {
        let mut h = DefaultHasher::new();
        c.trim().hash(&mut h);
        let k = h.finish();
        if !seen.contains(&k) {
            seen.push(k);
            out.push((p, c));
        }
    }
    out
}
