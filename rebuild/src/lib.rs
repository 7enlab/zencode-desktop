pub mod bubble;
pub mod color;
pub mod cow;
mod cows_gen;
pub mod term;
pub mod width;
pub mod wrap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Wrap {
    None,

    Hard(usize),

    Words(usize),
}

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
        Options {
            wrap: Wrap::Hard(40),
            cow: "default".into(),
            eyes: "oo".into(),
            tongue: "  ".into(),
            think: false,
        }
    }
}

pub fn say(text: &str, o: &Options) -> Result<String, String> {
    let lines = match o.wrap {
        Wrap::None => wrap::wrap(text, None),
        Wrap::Hard(w) => wrap::wrap(text, Some(w)),
        Wrap::Words(0) => wrap::wrap(text, None),
        Wrap::Words(w) => wrap::wrap_words(text, w),
    };
    let template = cow::load(&o.cow)?;
    let face = cow::Face {
        eyes: o.eyes.clone(),
        tongue: o.tongue.clone(),
        thoughts: if o.think { "o" } else { "\\" },
    };
    Ok(format!(
        "{}\n{}\n",
        bubble::bubble(&lines, o.think),
        cow::render(&template, &face)
    ))
}

pub fn auto_width() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|c| c.trim().parse::<usize>().ok())
        .or_else(term::terminal_columns)
        .filter(|&c| c > 8)
        .map(|c| c - 4)
        .unwrap_or(40)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn library_say_think() {
        let o = Options {
            think: true,
            ..Default::default()
        };
        let s = say("hi", &o).unwrap();
        assert!(s.starts_with(" ____\n( hi )\n ----\n        o"));
        assert!(say(
            "x",
            &Options {
                cow: "nope".into(),
                ..Default::default()
            }
        )
        .is_err());
    }
}
