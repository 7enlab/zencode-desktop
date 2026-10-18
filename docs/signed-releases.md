# Signed Releases (R15)

Every ZenCode Desktop release is cryptographically signed with
[minisign](https://jedisct1.github.io/minisign/) (Ed25519). Users can verify
that a downloaded artifact was built by the ZenCode team and was not tampered
with in transit.

## Trust model

- **One release keypair** for the whole project. The **secret key** is held
  offline by Hark 2 (project lead). The **public key** is committed in
  `desktop/src-tauri/tauri.conf.json` (`plugins.updater.pubkey`) and mirrored
  in `docs/signed-releases.md` below.
- The Tauri auto-updater **refuses to install** any update whose signature
  does not verify against the embedded public key. There is no bypass.
- Git tags are informational only; the signature on the artifact is what
  matters.

> **BLOCKER — Hark 2 must do this before the first public release:**
> 1. On an offline machine, run `minisign -G -p zencode-release.pub -s zencode-release.key`
>    (or `tauri signer generate -w ~/.tauri/zencode.key` — same Ed25519 format).
> 2. Store the secret key offline (encrypted USB / hardware key). Never commit it.
> 3. Put the public key in `desktop/src-tauri/tauri.conf.json`
>    (`plugins.updater.pubkey`) replacing the `PLACEHOLDER` value.
> 4. Paste the public key into the "Release public key" section below.
>
> Until this is done, `tools/sign-release.sh` refuses to run and the
> `check-release-signing` CI job fails the build.

## Release public key

```
UNSET — Hark 2: paste the minisign public key here after generating the keypair.
Format: untrusted comment: ...\n<RW... base64 ...>
```

## Signing a release (maintainer)

Prerequisites: the release secret key (`zencode-release.key`), `minisign`
installed, artifacts already built (`.AppImage`, `.dmg`, `.msi`/`.exe`,
`latest.json`).

```bash
# From the repo root. Signs every artifact in dist/ and writes dist/SHA256SUMS.
MINISIGN_KEY=/secure/path/zencode-release.key tools/sign-release.sh v0.4.0 dist/
```

What the script does:
1. Refuses to run if `MINISIGN_KEY` is unset or the file does not exist.
2. Refuses to run if `tauri.conf.json` still contains the `PLACEHOLDER` pubkey
   (signing without a published public key is pointless).
3. For each artifact file: `minisign -S -x <file>.minisig -s $MINISIGN_KEY -m <file>`
   (trusted comment records the version and filename).
4. Writes `SHA256SUMS` and signs that too (`SHA256SUMS.minisig`), so a single
   signature covers the whole release.

The signed files (`*.minisig`, `SHA256SUMS`, `SHA256SUMS.minisig`,
`latest.json`) are uploaded to the release page. The secret key never leaves
the maintainer's machine.

## Verifying a download (user)

```bash
# 1. Get the release public key (from this file or the website).
# 2. Verify:
tools/verify-release.sh zencode-release.pub ZenCode-0.4.0.AppImage
```

The script checks the `.minisig` next to the file and prints
`Signature verified` on success. Any mismatch (wrong key, modified file)
fails loudly.

Manual equivalent:

```bash
minisign -V -p zencode-release.pub -m ZenCode-0.4.0.AppImage
```

## Updater integration

- `desktop/src-tauri/tauri.conf.json` → `plugins.updater`:
  - `pubkey`: the release public key (currently `PLACEHOLDER` — see blocker).
  - `endpoints`: release metadata URLs. **Must point at GitLab**
    (`https://gitlab.com/7enlab/zencode-desktop/-/releases/...`) while the
    GitHub account is suspended; GitHub endpoints are kept as fallback only.
  - `active: true`, `dialog: true` — the user always sees and confirms updates.
- `desktop/src-tauri/src/main.rs` registers `tauri_plugin_updater`. The plugin
  verifies the Ed25519 signature of the downloaded bundle **before**
  installation; a bad signature aborts the update with an error dialog.
- `engine/src/release_signing.rs` (this crate) contains the offline
  verification logic used by tests and by any future non-Tauri install path:
  `verify_file(public_key, file)` → `Ok(())` / `Err(VerifyError)`.

## Key rotation

If the secret key is ever suspected compromised:
1. Hark 2 generates a new keypair offline.
2. The new public key ships in the **next** release (signed with the **old**
   key, so the updater trusts it).
3. `docs/signed-releases.md` records the rotation with dates; the old public
   key is kept in the "Retired keys" section for verifying old artifacts.

### Retired keys

(none yet)

## Files

| File | Purpose |
|---|---|
| `tools/sign-release.sh` | Maintainer: sign all artifacts for a release |
| `tools/verify-release.sh` | User/CI: verify one artifact against a public key |
| `engine/src/release_signing.rs` | Rust verification logic + tests (test keypair only) |
| `desktop/src-tauri/tauri.conf.json` | Updater `pubkey` + `endpoints` |
