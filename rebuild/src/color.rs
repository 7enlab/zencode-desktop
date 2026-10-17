//! Optional ANSI colours (ZenCode extension, off by default so output stays byte-identical
//! to cowsay@1.6.0). Inspired by Neo-cowsay's `--rainbow` / `--aurora` and charasay's
//! colour support, but std-only.
//!
//! Rules (see <https://no-color.org> and <https://bixense.com/clicolors/>):
//! `--color=never` never paints; `--color=always` or `CLICOLOR_FORCE` (non-`0`) always paints;
//! `auto` (default) paints only when stdout is a terminal and `NO_COLOR` is unset/empty.

/// What to paint with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paint {
    /// One SGR foreground code (31..=37, 90..=97) or 1 for bold.
    Solid(u8),
    /// Cycle red, yellow, green, cyan, blue, magenta per line.
    Rainbow,
}

/// When to paint (`--color=WHEN`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    Auto,
    Always,
    Never,
}

/// Colour names accepted by `--cow-color`.
pub const NAMES: &[(&str, Paint)] = &[
    ("red", Paint::Solid(31)),
    ("green", Paint::Solid(32)),
    ("yellow", Paint::Solid(33)),
    ("blue", Paint::Solid(34)),
    ("magenta", Paint::Solid(35)),
    ("cyan", Paint::Solid(36)),
    ("white", Paint::Solid(37)),
    ("gray", Paint::Solid(90)),
    ("bold", Paint::Solid(1)),
    ("rainbow", Paint::Rainbow),
];

const RAINBOW: [u8; 6] = [31, 33, 32, 36, 34, 35];

/// Look up a colour by name (case-insensitive).
pub fn parse(name: &str) -> Option<Paint> {
    let n = name.trim().to_ascii_lowercase();
    NAMES.iter().find(|(k, _)| *k == n).map(|(_, p)| *p)
}

/// Parse `--color=WHEN`.
pub fn parse_when(s: &str) -> Option<When> {
    match s.trim().to_ascii_lowercase().as_str() {
        "auto" => Some(When::Auto),
        "always" | "yes" | "force" => Some(When::Always),
        "never" | "no" | "none" => Some(When::Never),
        _ => None,
    }
}

/// Decide whether to paint, given the environment and whether stdout is a terminal.
pub fn enabled(when: When, is_tty: bool, no_color: Option<&str>, force: Option<&str>) -> bool {
    match when {
        When::Never => false,
        When::Always => true,
        When::Auto => {
            if force.is_some_and(|f| !f.is_empty() && f != "0") {
                return true;
            }
            is_tty && no_color.is_none_or(|v| v.is_empty())
        }
    }
}

/// Paint every non-empty line; each line is closed with a reset so nothing bleeds.
/// Newlines are preserved exactly, so stripping the escapes gives back the input.
pub fn paint(s: &str, p: Paint) -> String {
    let mut out = String::with_capacity(s.len() + s.len() / 4);
    for (i, line) in s.split_inclusive('\n').enumerate() {
        let (body, nl) = match line.strip_suffix('\n') {
            Some(b) => (b, "\n"),
            None => (line, ""),
        };
        if body.is_empty() {
            out.push_str(nl);
            continue;
        }
        let code = match p {
            Paint::Solid(c) => c,
            Paint::Rainbow => RAINBOW[i % RAINBOW.len()],
        };
        out.push_str(&format!("\x1b[{}m{}\x1b[0m{}", code, body, nl));
    }
    out
}

/// Remove SGR escapes (`ESC [ ... m`); used by tests and handy for callers.
pub fn strip(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\x1b' && it.peek() == Some(&'[') {
            for d in it.by_ref() {
                if d == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// On Windows, turn on ANSI processing for the console (no-op elsewhere).
/// Returns false when the console refused (legacy conhost), so callers can skip colour.
pub fn enable_vt() -> bool {
    imp::enable_vt()
}

#[cfg(windows)]
mod imp {
    use std::os::raw::{c_int, c_void};
    #[link(name = "kernel32")]
    extern "system" {
        fn GetStdHandle(n: u32) -> *mut c_void;
        fn GetConsoleMode(h: *mut c_void, mode: *mut u32) -> c_int;
        fn SetConsoleMode(h: *mut c_void, mode: u32) -> c_int;
    }
    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;
    pub fn enable_vt() -> bool {
        // SAFETY: plain Win32 calls with a valid out-pointer.
        unsafe {
            let h = GetStdHandle(-11i32 as u32); // STD_OUTPUT_HANDLE
            let mut mode = 0u32;
            if GetConsoleMode(h, &mut mode) == 0 {
                return true; // not a console (pipe/file): escapes pass through untouched
            }
            mode & ENABLE_VIRTUAL_TERMINAL_PROCESSING != 0
                || SetConsoleMode(h, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn enable_vt() -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paint_roundtrip_and_resets() {
        let s = " ____\n< hi >\n ----\n\n";
        for p in [Paint::Solid(31), Paint::Rainbow] {
            let c = paint(s, p);
            assert_eq!(strip(&c), s);
            assert_eq!(c.matches("\x1b[0m").count(), 3);
        }
        assert!(paint("a\nb\n", Paint::Rainbow).starts_with("\x1b[31ma\x1b[0m\n\x1b[33mb"));
    }
    #[test]
    fn names_and_rules() {
        assert_eq!(parse("Cyan"), Some(Paint::Solid(36)));
        assert_eq!(parse("nope"), None);
        assert_eq!(parse_when("ALWAYS"), Some(When::Always));
        assert!(!enabled(When::Auto, false, None, None));
        assert!(enabled(When::Auto, true, None, None));
        assert!(!enabled(When::Auto, true, Some("1"), None));
        assert!(enabled(When::Auto, true, Some(""), None));
        assert!(enabled(When::Auto, false, None, Some("1")));
        assert!(!enabled(When::Auto, false, None, Some("0")));
        assert!(!enabled(When::Never, true, None, Some("1")));
        assert!(enabled(When::Always, false, Some("1"), None));
    }
}
