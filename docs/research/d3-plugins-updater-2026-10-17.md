# D3 — Day-One Plugins + Updater Keypair (design)

*Research · Ideas · Results — for Hark 2 (g6 Tauri shell). Docs only, no code.*

## Research

### Tauri 2 plugin system

Tauri 2 moved almost everything out of core into official plugins (`tauri-plugin-*`
crates, all real and maintained by the Tauri team). The app declares them in
`src-tauri/Cargo.toml` + `src-tauri/capabilities/` (least-privilege permissions).
Key plugins for ZenCode Desktop v0.3.0:

| Plugin | Crate | Why day one |
|---|---|---|
| updater | `tauri-plugin-updater` | auto-update — **must ship in v1 or users can never auto-update later** |
| dialog | `tauri-plugin-dialog` | open/save file dialogs |
| fs | `tauri-plugin-fs` | scoped filesystem access (capabilities-gated) |
| notification | `tauri-plugin-notification` | task-complete toasts (D10) |
| process | `tauri-plugin-process` | relaunch after update, exit |
| shell | `tauri-plugin-shell` | spawn `zen` sidecar / open URLs |
| store | `tauri-plugin-store` | persist settings (model picker, theme) |
| log | `tauri-plugin-log` | frontend+backend logs to disk |
| window-state | `tauri-plugin-window-state` | remember size/position |
| single-instance | `tauri-plugin-single-instance` | one app instance, focus on second launch |
| deep-link | `tauri-plugin-deep-link` | `zencode://` URLs |

### Why the updater keypair cannot wait

`tauri-plugin-updater` verifies every update artifact against a **public key baked
into the binary** (`plugins.updater.pubkey` in `tauri.conf.json`). The matching
**private key** signs artifacts in CI (`TAURI_SIGNING_PRIVATE_KEY` env).

The trap: if v0.3.0 ships *without* the updater plugin (or with a throwaway key),
every installed copy is permanently stuck — it has no code path to fetch and verify
updates, and no trusted key to verify them with. The only fix is asking every user
to manually download the new installer. Generating the real keypair and wiring the
plugin **before the first public release** costs one afternoon; retrofitting costs
your entire installed base.

Signing uses **minisign** (`tauri signer generate -w` creates the keypair). The
update server serves a small JSON manifest per platform:

```json
{
  "version": "v0.4.0",
  "notes": "…",
  "pub_date": "2026-11-01T00:00:00Z",
  "platforms": {
    "darwin-aarch64": { "signature": "…minisign sig…", "url": "https://…" }
  }
}
```

Note: updater signing (minisign, free) is **independent** of OS code signing
(Apple Developer $99/yr, Windows EV cert — expensive, deferred per project decision
in `docs/desktop-architecture.md` §5). Unsigned-but-updater-signed builds install
fine; users just see the OS "unidentified developer" prompt once.

## Ideas

**Plugin set for v0.3.0:** the 11 plugins in the table above. Add `capabilities/`
files from day one — default-deny, then allow exactly what each window needs
(e.g. `fs` scoped to workspace dirs only). Retrofitting least-privilege later
means auditing every `invoke` call.

**Keypair generation + custody:**
1. One person (Zen) runs `tauri signer generate -w ~/.config/tauri/zencode.key`
   **once**, offline. This key signs every future release.
2. Private key → GitHub Actions secret `TAURI_SIGNING_PRIVATE_KEY`
   (+ `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`). Never in git, never in chat.
3. Public key → committed in `tauri.conf.json` (`plugins.updater.pubkey`).
4. Document the fingerprint somewhere durable (this repo's `docs/` + Notion)
   so a future key rotation can be announced and verified.

**CI signing flow:** `tauri-action` builds per OS → signs artifacts with the CI
secret → uploads manifest + binaries to the update endpoint (GitHub Releases
works as the update server — no extra infra).

**Update channels:** two manifests — `stable` (GitHub Releases, what the app
checks) and `beta` (pre-releases, opt-in via a settings toggle). The app only
ever trusts signatures from the baked-in key, regardless of channel.

**Rollback policy:** keep the previous version's artifacts on the update server
for 30 days. The updater plugin itself has no downgrade path — rollback =
publish the old version number as a new "update" (re-signed). Document this;
don't build custom rollback UI in v0.3.0.

## Results

### Checklist for Hark 2 — do before the first public release

- [ ] `tauri signer generate -w` → keypair created, private key in CI secrets,
      public key in `tauri.conf.json`, fingerprint recorded in `docs/`
- [ ] `tauri-plugin-updater` added with `pubkey` + `endpoints` (stable manifest URL)
- [ ] The other 10 plugins added with minimal `capabilities/` (default-deny)
- [ ] `tauri-action` workflow builds + signs on push to release tags
- [ ] Beta channel manifest + settings toggle (can slip to v0.4.0 if needed —
      but the *keypair* cannot slip)

### Open questions

1. **Update server:** GitHub Releases as the manifest host (zero infra), or a
   dedicated endpoint for staged rollouts? (Recommendation: Releases for v0.3.0.)
2. **Key custody:** does Zen hold the only private-key copy, or do we need a
   second custodian / hardware key from day one?
3. **Windows/macOS code signing:** deferred for cost reasons — but should the
   *updater manifest* note the "unidentified developer" prompt so users aren't
   surprised on first install?
