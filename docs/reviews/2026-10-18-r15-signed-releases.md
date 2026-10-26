# Review: ZD Leader `lead/signed-releases-r15` (R15 signed releases)

- Reviewer: Z App - 2026-10-18
- Branch head: `394875e` (GitLab), base `main` = `3aa1f70`
- Verdict: **merged** (`--no-ff`) with a follow-up fix commit.

## Scope check
- Touches only `engine/`, `desktop/src-tauri/` (config + one comment), `docs/`, `tools/` - Desktop scope (updater / release integrity). `core/` (frozen) untouched.
- No secret committed: only a test public key and one test signature in `release_signing.rs`. The release secret key is kept outside the repository.
- Updater endpoints: GitLab release permalink added first, GitHub kept as fallback - OK while the GitHub account is suspended.

## What was good
- `verify_bytes` rejects tampered files, wrong keys, malformed keys/signatures; `allow_legacy = false` (prehashed signatures only); the trusted comment is covered by the global signature. No code path skips verification.
- Scripts use `set -euo pipefail`, never print the key, refuse to run without a key or with the placeholder pubkey.
- Test claim confirmed: 100/100 engine tests passed on the branch as submitted (91 lib + 3 + 6).

## Issues found and fixed (follow-up commit)
1. **Heavy dependency**: `minisign 0.10` pulled 17 transitive crates (incl. `rpassword`, `scrypt`, terminal password prompting) into a library that only verifies. Replaced by `minisign-verify 0.3` (zero dependencies, MIT, verification only - the same crate `tauri-plugin-updater` already uses).
2. **Placeholder check too weak**: `contains("PLACEHOLDER")` accepted an empty, missing or malformed key, and a bare `RW...` key (Tauri expects base64 of the whole `.pub` file). New `updater_public_key()` parses the JSON, unwraps and validates the key; `updater_pubkey_is_placeholder()` now means "no usable key".
3. **Trusted comment not usable by callers**: added `verify_bytes_with_comment()` returning the verified trusted comment, so an install path can bind it to the expected version/file (anti-downgrade / substitution).
4. **Docs claimed a `check-release-signing` CI job that does not exist**. Replaced by a real gate: test `release_key_is_wired` (fails if `tauri.conf.json` and `docs/keys/zencode-release.pub` disagree or the key is missing).
5. `sign-release.sh`: a missing `tauri.conf.json` silently skipped the placeholder check (`grep ... 2>/dev/null`) → now a hard error; `sha256sum $(ls | grep ...)` broke on file names with spaces → `find -print0 | xargs -0`; every new signature is re-verified against the published public key, so a wrong secret key fails the run.
6. Tests used fixed temp dirs → per-process names.
7. Release keypair generated (key id `A459EE7B787B86FF`); public key committed at `docs/keys/zencode-release.pub`, wired into `tauri.conf.json` (base64 of the `.pub` file) and `docs/signed-releases.md`; stale placeholder comment in `main.rs` updated.

## Verification
- Engine: `cargo fmt --check` and `cargo clippy --locked --all-targets -- -D warnings` clean.
- Engine: `cargo test --locked` → **102 passed, 0 failed** (93 lib + 3 + 6).
- Scripts end-to-end with the real key: signing 2 artifacts (one name with a space) + SHA256SUMS → OK; `verify-release.sh` OK; tampered file → exit 1; signing with a different key → exit 1 at the self-check.
- Desktop crate (`desktop/src-tauri`): `cargo fmt --check` only; not built - webkit2gtk / pkg-config are not available in the review sandbox. `tauri.conf.json` validated as JSON.
- `cargo deny` / `cargo audit` are not installed and not in CI; the new crate was checked by hand (MIT, no dependencies).

## Left for ZD Leader
- Release pipeline: put `base64(<artifact>.minisig)` into the `latest.json` `signature` fields (Tauri format) and confirm with a real `tauri build` + updater run that the plugin accepts our key and signatures (and that `tauri signer sign` accepts an unencrypted minisign secret key, if that path is used).
- Wire `verify_bytes_with_comment` into any non-Tauri install path and check the comment against the expected version.
- Add a GitLab CI job for `cargo deny check` and the release gate test.
- The secret key is unencrypted (no password prompt available); encrypt it or move it to a hardware token before the first public release.
