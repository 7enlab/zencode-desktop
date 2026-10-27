use crate::cows_gen::COWS;

pub struct Face {
    pub eyes: String,
    pub tongue: String,
    pub thoughts: &'static str,
}

pub const MODES: &[(char, &str, &str)] = &[
    ('b', "==", "  "),
    ('d', "xx", "U "),
    ('g', "$$", "  "),
    ('p', "@@", "  "),
    ('s', "**", "U "),
    ('t', "--", "  "),
    ('w', "OO", "  "),
    ('y', "..", "  "),
];

pub fn names() -> Vec<&'static str> {
    COWS.iter().map(|c| c.0).collect()
}

pub fn load(name: &str) -> Result<String, String> {
    if name.contains('/') || name.contains('\\') {
        return std::fs::read_to_string(name)
            .map_err(|e| format!("cannot read cowfile {}: {}", name, e));
    }
    COWS.iter()
        .find(|c| c.0 == name)
        .map(|c| c.1.to_string())
        .ok_or_else(|| format!("Could not find {} cowfile!", name))
}

fn extract(cow: &str) -> String {
    let c = cow
        .strip_prefix('\u{FEFF}')
        .unwrap_or(cow)
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let Some(p) = c.find("$the_cow") else {
        return c;
    };
    let rest = &c[p + 8..];

    let Some(nl) = rest.find('\n') else { return c };
    let header: String = rest[..nl]
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect();
    let ok =
        header.starts_with("=<<") && header[3..].trim_matches(|ch| ch == '"' || ch == ';') == "EOC";
    let body = &rest[nl + 1..];
    let Some(end) = body.find("\nEOC\n") else {
        return c;
    };
    if !ok {
        return c;
    }
    body[..end]
        .replace("\\\\", "\\")
        .replace("\\@", "@")
        .replace("\\$", "$")
}

pub fn render(cow: &str, f: &Face) -> String {
    let mut s = if cow.contains("$the_cow") {
        extract(cow)
    } else {
        cow.to_string()
    };

    let esc = f.eyes.replace('$', "$$");
    let mut eye = esc.chars();
    let (l, r) = (
        eye.next().map(String::from).unwrap_or_default(),
        eye.next().map(String::from).unwrap_or_default(),
    );
    s = s
        .replace("$thoughts", f.thoughts)
        .replace("$eyes", &f.eyes)
        .replace("$tongue", &f.tongue)
        .replace("${eyes}", &f.eyes);
    s = s.replacen("$eye", &l, 1).replacen("$eye", &r, 1);
    s.replace("${tongue}", &f.tongue)
}
