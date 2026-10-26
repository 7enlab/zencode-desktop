








#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paint {

    Solid(u8),

    Rainbow,


    Aurora,
}


#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Depth {

    Ansi256,

    TrueColor,
}


pub fn depth(colorterm: Option<&str>) -> Depth {
    match colorterm.map(|v| v.trim().to_ascii_lowercase()) {
        Some(v) if v == "truecolor" || v == "24bit" => Depth::TrueColor,
        _ => Depth::Ansi256,
    }
}


fn hue_rgb(h: f32) -> (u8, u8, u8) {
    let h = h.rem_euclid(360.0) / 60.0;
    let v = 242.0_f32;
    let x = v * (1.0 - ((h % 2.0) - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (v, x, 0.0),
        1 => (x, v, 0.0),
        2 => (0.0, v, x),
        3 => (0.0, x, v),
        4 => (x, 0.0, v),
        _ => (v, 0.0, x),
    };
    (r.round() as u8, g.round() as u8, b.round() as u8)
}


fn rgb_256(r: u8, g: u8, b: u8) -> u8 {
    let q = |c: u8| ((c as u16 * 5 + 127) / 255) as u8;
    16 + 36 * q(r) + 6 * q(g) + q(b)
}

fn sgr_rgb(out: &mut String, (r, g, b): (u8, u8, u8), d: Depth) {
    match d {
        Depth::TrueColor => out.push_str(&format!("\x1b[38;2;{};{};{}m", r, g, b)),
        Depth::Ansi256 => out.push_str(&format!("\x1b[38;5;{}m", rgb_256(r, g, b))),
    }
}


#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    Auto,
    Always,
    Never,
}


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
    ("aurora", Paint::Aurora),
];

const RAINBOW: [u8; 6] = [31, 33, 32, 36, 34, 35];


pub fn parse(name: &str) -> Option<Paint> {
    let n = name.trim().to_ascii_lowercase();
    NAMES.iter().find(|(k, _)| *k == n).map(|(_, p)| *p)
}


pub fn parse_when(s: &str) -> Option<When> {
    match s.trim().to_ascii_lowercase().as_str() {
        "auto" => Some(When::Auto),
        "always" | "yes" | "force" => Some(When::Always),
        "never" | "no" | "none" => Some(When::Never),
        _ => None,
    }
}


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



pub fn paint(s: &str, p: Paint) -> String {
    paint_depth(s, p, Depth::Ansi256)
}


pub fn paint_depth(s: &str, p: Paint, d: Depth) -> String {
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
            Paint::Aurora => {
                let mut last = None;
                for (j, ch) in body.chars().enumerate() {
                    if ch != ' ' {

                        let rgb = hue_rgb(((i + j / 2) * 12) as f32);
                        if last != Some(rgb) {
                            sgr_rgb(&mut out, rgb, d);
                            last = Some(rgb);
                        }
                    }
                    out.push(ch);
                }
                out.push_str("\x1b[0m");
                out.push_str(nl);
                continue;
            }
        };
        out.push_str(&format!("\x1b[{}m{}\x1b[0m{}", code, body, nl));
    }
    out
}


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

        unsafe {
            let h = GetStdHandle(-11i32 as u32);
            let mut mode = 0u32;
            if GetConsoleMode(h, &mut mode) == 0 {
                return true;
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
        for d in [Depth::Ansi256, Depth::TrueColor] {
            let c = paint_depth(s, Paint::Aurora, d);
            assert_eq!(strip(&c), s);
            assert_eq!(c.matches("\x1b[0m").count(), 3);
        }
        assert!(
            paint_depth("ab", Paint::Aurora, Depth::TrueColor).starts_with("\x1b[38;2;242;0;0ma")
        );
        assert!(paint_depth("ab", Paint::Aurora, Depth::Ansi256).starts_with("\x1b[38;5;196ma"));
        assert!(paint("a\nb\n", Paint::Rainbow).starts_with("\x1b[31ma\x1b[0m\n\x1b[33mb"));
    }
    #[test]
    fn names_and_rules() {
        assert_eq!(parse("Cyan"), Some(Paint::Solid(36)));
        assert_eq!(parse("nope"), None);
        assert_eq!(parse("AURORA"), Some(Paint::Aurora));
        assert_eq!(depth(Some("truecolor")), Depth::TrueColor);
        assert_eq!(depth(Some("24BIT")), Depth::TrueColor);
        assert_eq!(depth(Some("1")), Depth::Ansi256);
        assert_eq!(depth(None), Depth::Ansi256);
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
