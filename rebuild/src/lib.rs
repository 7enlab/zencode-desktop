//! ZenCode Desktop pilot library: cowsay@1.6.0-compatible speech bubbles, std only.
//!
//! ```
//! let out = cowsay_bubble::say("Hello", &cowsay_bubble::Options::default()).unwrap();
//! assert!(out.starts_with(" _______\n< Hello >\n"));
//! ```
pub mod bubble;
pub mod cow;
mod cows_gen;
pub mod width;
pub mod wrap;

/// How to wrap the message.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Wrap {
    /// No wrapping (`-n`).
    None,
    /// cowsay@1.6.0 hard cut every N UTF-16 units (`-W N`, default 40).
    Hard(usize),
    /// Break at spaces by display columns (`--word-wrap`).
    Words(usize),
}

/// Options for [`say`]; `Default` matches plain `cowsay`.
#[derive(Clone, Debug)]
pub struct Options {
    pub wrap: Wrap,
    pub cow: String,
    pub eyes: String,
    pub tongue: String,
    pub think: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { wrap: Wrap::Hard(40), cow: "default".into(), eyes: "oo".into(), tongue: "  ".into(), think: false }
    }
}

/// Render `text` as a full cowsay picture (bubble + cow), ending with a newline.
pub fn say(text: &str, o: &Options) -> Result<String, String> {
    let lines = match o.wrap {
        Wrap::None => wrap::wrap(text, None),
        Wrap::Hard(w) => wrap::wrap(text, Some(w)),
        Wrap::Words(0) => wrap::wrap(text, None),
        Wrap::Words(w) => wrap::wrap_words(text, w),
    };
    let template = cow::load(&o.cow)?;
    let face = cow::Face { eyes: o.eyes.clone(), tongue: o.tongue.clone(), thoughts: if o.think { "o" } else { "\\" } };
    Ok(format!("{}\n{}\n", bubble::bubble(&lines, o.think), cow::render(&template, &face)))
}

/// Wrap column for `-W auto`: terminal width from $COLUMNS minus the 4 border columns, else 40.
pub fn auto_width() -> usize {
    std::env::var("COLUMNS").ok().and_then(|c| c.trim().parse::<usize>().ok())
        .filter(|&c| c > 8).map(|c| c - 4).unwrap_or(40)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn library_say_think() {
        let o = Options { think: true, ..Default::default() };
        let s = say("hi", &o).unwrap();
        assert!(s.starts_with(" ____\n( hi )\n ----\n        o"));
        assert!(say("x", &Options { cow: "nope".into(), ..Default::default() }).is_err());
    }
}
