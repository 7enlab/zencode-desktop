use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

fn parse_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '"' {
            continue;
        }
        let mut cur = String::new();
        while let Some(c) = it.next() {
            match c {
                '"' => break,
                '\\' => match it.next().unwrap() {
                    'n' => cur.push('\n'),
                    't' => cur.push('\t'),
                    'r' => cur.push('\r'),
                    'u' => {
                        let h: String = it.by_ref().take(4).collect();
                        cur.push(char::from_u32(u32::from_str_radix(&h, 16).unwrap()).unwrap());
                    }
                    o => cur.push(o),
                },
                o => cur.push(o),
            }
        }
        out.push(cur);
    }
    out
}

#[test]
fn golden_cases() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../spec/cases");
    let mut n = 0;
    for e in fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let args = parse_args(&fs::read_to_string(&p).unwrap());
        let stdin = fs::read(p.with_extension("stdin")).ok();
        let mut child = Command::new(env!("CARGO_BIN_EXE_cowsay"))
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        {
            let mut i = child.stdin.take().unwrap();
            if let Some(b) = &stdin {
                i.write_all(b).unwrap();
            }
        }
        let got = child.wait_with_output().unwrap().stdout;
        let exp = fs::read(p.with_extension("out")).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&got),
            String::from_utf8_lossy(&exp),
            "case {:?}",
            p
        );
        n += 1;
    }
    assert_eq!(n, 21);
}

#[test]
fn color_extension() {
    let run = |args: &[&str], env: &[(&str, &str)]| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_cowsay"));
        c.args(args)
            .env_remove("NO_COLOR")
            .env_remove("CLICOLOR_FORCE");
        for (k, v) in env {
            c.env(k, v);
        }
        String::from_utf8(c.output().unwrap().stdout).unwrap()
    };
    let plain = run(&["hi"], &[]);
    assert_eq!(run(&["--cow-color", "red", "hi"], &[]), plain);
    let red = run(&["--cow-color=red", "--color=always", "hi"], &[]);
    assert!(red.contains("\x1b[31m"));
    assert_eq!(cowsay_bubble::color::strip(&red), plain);
    let forced = run(
        &["--cow-color", "rainbow", "hi"],
        &[("CLICOLOR_FORCE", "1")],
    );
    assert!(forced.contains("\x1b[33m"));
    let tc = run(
        &["--cow-color=aurora", "--color=always", "hi"],
        &[("COLORTERM", "truecolor")],
    );
    assert!(tc.contains("\x1b[38;2;"));
    assert_eq!(cowsay_bubble::color::strip(&tc), plain);
    let c256 = run(
        &["--cow-color=aurora", "--color=always", "hi"],
        &[("COLORTERM", "")],
    );
    assert!(c256.contains("\x1b[38;5;") && !c256.contains("38;2;"));
    assert_eq!(
        run(
            &["--cow-color", "red", "--color=never", "hi"],
            &[("CLICOLOR_FORCE", "1")]
        ),
        plain
    );
}
