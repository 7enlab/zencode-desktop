


pub fn normalize(text: &str) -> String {
    let t = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let mut out = String::with_capacity(t.len());
    let mut it = t.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\r' => {
                if it.peek() == Some(&'\n') {
                    it.next();
                }
                out.push('\n');
            }
            '\u{2028}' | '\u{2029}' => out.push('\n'),
            '\t' => out.push_str("        "),
            _ => out.push(c),
        }
    }
    out
}



pub fn wrap(text: &str, width: Option<usize>) -> Vec<String> {
    let text = normalize(text);
    let w = match width {
        Some(w) if w > 0 => w,
        _ => return text.split('\n').map(String::from).collect(),
    };
    let mut lines = Vec::new();
    let last = text.matches('\n').count();
    for (i, seg) in text.split('\n').enumerate() {

        let is_last = i == last;
        if seg.is_empty() {
            if !is_last {
                lines.push(String::new());
            }
            continue;
        }
        let mut cur = String::new();
        let mut units = 0;
        for c in seg.chars() {
            let u = c.len_utf16();
            if units + u > w && !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
                units = 0;
            }
            cur.push(c);
            units += u;
        }
        lines.push(cur);
    }
    lines
}




pub fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let text = normalize(text);
    let w = width.max(1);
    let mut lines = Vec::new();
    let last = text.matches('\n').count();
    for (i, seg) in text.split('\n').enumerate() {
        if seg.trim().is_empty() {
            if i != last || (i == 0 && !seg.is_empty()) {
                lines.push(String::new());
            }
            continue;
        }
        let mut cur = String::new();
        let mut cw = 0;
        for word in seg.split(' ').filter(|s| !s.is_empty()) {
            let ww = crate::width::width(word);
            if cw > 0 && cw + 1 + ww <= w {
                cur.push(' ');
                cur.push_str(word);
                cw += 1 + ww;
                continue;
            }
            if cw > 0 {
                lines.push(std::mem::take(&mut cur));
                cw = 0;
            }

            for c in word.chars() {
                let x = crate::width::width(c.encode_utf8(&mut [0; 4]));
                if cw + x > w && cw > 0 {
                    lines.push(std::mem::take(&mut cur));
                    cw = 0;
                }
                cur.push(c);
                cw += x;
            }
        }
        lines.push(cur);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hard_cut() {
        assert_eq!(wrap("abcdef", Some(4)), vec!["abcd", "ef"]);
    }
    #[test]
    fn blank_and_trailing() {
        assert_eq!(wrap("a\n\nb\n", Some(40)), vec!["a", "", "b"]);
        assert!(wrap("", Some(40)).is_empty());
        assert_eq!(wrap("", None), vec![""]);
    }
    #[test]
    fn word_wrap() {
        let t = "The quick brown fox jumps over the lazy dog and keeps running far away";
        assert_eq!(
            wrap_words(t, 40),
            vec![
                "The quick brown fox jumps over the lazy",
                "dog and keeps running far away"
            ]
        );
        assert_eq!(
            wrap_words("abcdefghij xy", 4),
            vec!["abcd", "efgh", "ij", "xy"]
        );
        assert_eq!(wrap_words("a  b\n\nc\n", 40), vec!["a b", "", "c"]);
        assert_eq!(wrap_words("你好世界 ok", 4), vec!["你好", "世界", "ok"]);
        assert!(wrap_words("", 40).is_empty());
    }
    #[test]
    fn crlf_tab() {
        assert_eq!(wrap("a\r\nb\tc", None), vec!["a", "b        c"]);
    }
}
