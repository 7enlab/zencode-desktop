# DV-4 tier 3 — Runbook: running the real binary with a mock LLM (30/10/2026)

**CI NOTE — the tier-3 live capture is NOT reproducible by CI.** The live
test is marked `#[ignore]`; CI does not run it. Reproducing the capture
requires the local mock LLM stub at `127.0.0.1:18123` and a locally built
CLI binary (see §§1–4). CI only runs `contract_harness` (33 tests) and
`stream_json` (24 tests); it does not run the live capture.

Tier 3 pins the CLI on both sides per decisions in messages 1242/1269:
the real CLI binary runs as a subprocess just like the real Desktop, the
LLM is simulated by a local HTTP stub, and the stream-json output is
verified by the Desktop parser. This doc records enough to rerun from
scratch on the ZD Developer box; first-run figures are in §5.

## 1. Locations (decision 1242: binary runs on the ZD Developer box in `~/muse2`)

| Component | Path on the box | Note |
|---|---|---|
| CLI source | `~/muse2/zencode` | Clone `github.com/7enlabs/zencode` @ pinned rev `290154c` |
| Binary | `~/muse2/zen` | Copied from `~/muse2/zencode/target/debug/zen`, debug profile |
| Mock stub | `~/muse2/mock_llm_stub.py` | Python http.server, binds only `127.0.0.1:18123` |
| Captured output | `~/muse2/tier3-output.jsonl` | Stdout of the §4 run |
| Tier-3 test | `engine/tests/tier3_live_cli.rs` | On the DV-4 branch, `#[ignore]`, reads `DV4_TIER3_CAPTURE` |

Path choice: design doc §5 does not pin paths, so they follow decision
1242 (`~/muse2`): source at `~/muse2/zencode`, binary at `~/muse2/`.

## 2. Build the binary (debug profile — the design doc is silent on profile, so debug was chosen for speed)

```sh
cd ~/muse2
git clone --depth 100 https://github.com/7enlabs/zencode.git zencode
cd zencode && git checkout -q 290154c && git rev-parse HEAD
# expected: 290154c1da24e482cd888f7099a01b036597c15f
cargo build -p zen-cli   # first build needs network to fetch a crate missing from the cache (webpki-roots 1.0.10); later builds work offline
cp target/debug/zen ~/muse2/zen
~/muse2/zen --version   # expected: zen 0.1.0
```

Note: `cargo build --offline` failed at first because the box cache only
had webpki-roots 1.0.9/1.0.5 while the lockfile needed 1.0.10 — build
online once to fetch that crate. That network use serves the build only
(crates.io); it has nothing to do with the LLM path: the stub never calls
the outside network.

## 3. Mock LLM stub (read the client code before writing it)

The 290154c CLI's LLM client (`crates/zen-agent/src/client.rs:264`) POSTs
to `{base_url}/chat/completions` following the OpenAI chat.completion
schema. The stub mimics exactly the CLI repo's own mock test
(`crates/zen-cli/tests/common/mod.rs`, `Reply::text` + `MockServer`): a
single canned response for every request, no randomness, no clock, no
request-dependent content — the same request always yields the same
byte-identical response (proven by 2 identical curls, see §5).

```sh
nohup python3 ~/muse2/mock_llm_stub.py > /tmp/mock-stub.out 2>&1 &
# the stub logs requests at /tmp/mock-stub.log (method + path + sequence number only)
```

Point the binary at the stub with the CLI's official flags (no CLI
source patch): `--model mock-model --base-url http://127.0.0.1:18123/v1`,
plus `ZEN_API_KEY=dummy` (a dummy key; the stub does not check it).

## 4. Run the tier-3 harness (the way the real Desktop calls the CLI, via subprocess)

```sh
rm -rf /tmp/dv4-tier3-home /tmp/dv4-tier3-cwd && mkdir -p /tmp/dv4-tier3-home /tmp/dv4-tier3-cwd
cd /tmp/dv4-tier3-cwd
env -u ZEN_MODEL -u ZEN_BASE_URL -u OPENAI_API_KEY -u ZEN_SETTINGS \
  HOME=/tmp/dv4-tier3-home ZEN_API_KEY=dummy ZEN_TRUST_FOLDER=1 \
  ~/muse2/zen -p --output-format stream-json \
  --model mock-model --base-url http://127.0.0.1:18123/v1 hello \
  > ~/muse2/tier3-output.jsonl 2> /tmp/dv4-tier3-stderr.txt
echo "exit=$?"
```

Isolated HOME + `ZEN_TRUST_FOLDER=1` mimic the CLI repo's own test
approach (`ZenRun`) so the run does not pick up user settings; the
`--model` / `--base-url` flags are the CLI's official provider-pointing
path, not a patch.

Verify with the real parser (test `#[ignore]`, not run by plain `cargo
test` so CI does not need the capture file):

```sh
cd ~/zencode-desktop/engine
DV4_TIER3_CAPTURE=~/muse2/tier3-output.jsonl \
  cargo test --offline --locked --test tier3_live_cli -- --ignored
```

## 5. First-run evidence (30/10/2026, ZD Developer box)

- Build: `cargo build -p zen-cli` — done, binary `~/muse2/zen`,
  `zen 0.1.0`. Built rev: `290154c1da24e482cd888f7099a01b036597c15f`.
- Stub: `127.0.0.1:18123`, 2 identical curl requests gave a
  byte-identical response (`DETERMINISTIC-IDENTICAL`); the stub log
  recorded 3 requests (2 verification curls + 1 call from the binary) —
  the binary called the LLM exactly once, all over localhost, no outside
  network calls.
- Subprocess run: exit code 0, empty stderr, 3 stdout lines, all `v = 1`:
  `system/init` (tools, model `mock-model`) → `assistant`
  (content `Hello from the mock LLM.`) → `result` (subtype `success`,
  `is_error false`, `num_turns 1`, usage 7/3 matching the stub's usage
  exactly).
- Parser check (`tier3_live_cli`, 1 passed, 0 failed): 3 lines parse `Ok`,
  order `system → assistant → result`, each line projects to ≥ 1 event,
  system → `SystemMessage cli_system:init`, assistant →
  `AssistantMessage` with the exact mock text, result → `SessionEnd`
  success, `num_turns 1`, model `mock-model`, input 7 / output 3,
  `interpret_exit_code(0) = Success`.

Varies from run to run (normal, not a finding): `session_id`, `uuid`,
`duration_ms` differ each time — the CLI repo itself normalises these
fields to placeholders in its own contract test. Content (mock text,
usage, order, subtype) is deterministic.

## 6. Divergences between design doc §5 and decisions 1242/1269 (decisions win)

- §5 left open "needs a Leader decision on where to run the CLI binary
  and which shared mock LLM" — now settled: box `~/muse2` + local HTTP
  stub (decisions 1242/1269).
- §5 mentions the CLI-side mechanism "regenerate fixture + update
  `docs/stream-json-v1.md` in the same PR" for deliberate changes — this
  tier 3 does NOT regenerate fixtures, does NOT commit
  `docs/stream-json-v1.md`: the fixture is the standard, the binary must
  match the fixture; landing the doc still awaits a separate Leader
  decision (the harness's immutable rule).
- §5 requires "both ends present on the same main" — now true: main
  `4b71b23` already had the parser + fixtures after PR #3 merged.

## 7. Finding: none

All tier-3 checks passed on the first run; no check failed to keep as a
finding, no blocker (the stub LLM path is clean via official flags, no
CLI source patch needed).
