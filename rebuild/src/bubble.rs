use crate::width::width;

pub fn bubble(lines: &[String], think: bool) -> String {
    let max = lines.iter().map(|l| width(l)).max().unwrap_or(0);
    let pad = |l: &str| format!("{}{}", l, " ".repeat(max - width(l)));
    let mut out = vec![format!(" {}", "_".repeat(max + 2))];
    if lines.len() == 1 {
        let (a, z) = if think { ('(', ')') } else { ('<', '>') };
        out.push(format!("{} {} {}", a, lines[0], z));
    } else {
        let n = lines.len();
        for (i, l) in lines.iter().enumerate() {
            let (a, b) = if think {
                ('(', ')')
            } else if i == 0 {
                ('/', '\\')
            } else if i == n - 1 {
                ('\\', '/')
            } else {
                ('|', '|')
            };
            out.push(format!("{} {} {}", a, pad(l), b));
        }
    }
    out.push(format!(" {}", "-".repeat(max + 2)));
    out.join("\n")
}
