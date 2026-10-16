# ZenCode Desktop — Rust pilot (cowsay@1.6.0 rebuild)

Std-only Rust rebuild of npm `cowsay@1.6.0` (MIT), byte-for-byte compatible by default, for Linux, macOS and Windows.

- `audit/` behaviour audit of the original, `spec/` spec + golden cases (`NN.json` argv, `NN.out` exact stdout).
- `rebuild/` crate `cowsay_bubble` (library `say(text, &Options)`) and binary `cowsay`.
- Extensions (opt-in): `--word-wrap`, `-W auto`, `-r/--random`, `-l`, `-f`, modes `-bdgpstwy`, `-e`, `-T`, `--think`.
- `RESEARCH.md` research, ideas and results log.

```
cd rebuild && cargo test --release && ./target/release/cowsay "Hello"
```
Cowfiles in `rebuild/cows/` come from cowsay@1.6.0 (MIT, see LICENSE-cowsay).
