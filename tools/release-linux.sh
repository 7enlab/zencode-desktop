#!/bin/bash
# tools/release-linux.sh — build, sign and stage the Linux updater release (0.3-4).
#
# Usage (CI only, protected tag pipeline):
#   TAURI_SIGNING_PRIVATE_KEY=... TAURI_SIGNING_PRIVATE_KEY_PASSWORD=... \
#     tools/release-linux.sh v0.3.1 dist/
#
# Signing runs ONLY in GitLab CI: the minisign key lives in the protected,
# masked CI variables TAURI_SIGNING_PRIVATE_KEY (+ _PASSWORD), which GitLab only
# exposes to protected branches/tags. Never run this with a key on a laptop.
#
# Steps:
#  1. Refuse unless the tag version equals the app version in tauri.conf.json.
#  2. `cargo tauri build --bundles appimage` (createUpdaterArtifacts=true).
#  3. Copy the AppImage to a versioned, URL-safe name
#     (zencode-desktop_<version>_<arch>.AppImage) and sign THAT file with
#     `cargo tauri signer sign`, so the signed trusted comment
#     ("file:zencode-desktop_<version>_...") carries the version. The app's
#     signed-version check (engine release_signing::check_update_offer) refuses
#     any manifest whose version is not the one in the signed file name.
#  4. Write latest.json (static Tauri manifest, platform linux-x86_64) whose url
#     is the GitLab generic-package download of that file.
#  5. Self-check: the signature's trusted comment must name the versioned file.
set -euo pipefail

TAG="${1:?usage: $0 <tag vX.Y.Z> <dist-dir>}"
DIST="${2:?usage: $0 <tag vX.Y.Z> <dist-dir>}"
VERSION="${TAG#v}"

: "${TAURI_SIGNING_PRIVATE_KEY:?TAURI_SIGNING_PRIVATE_KEY is not set (protected CI variable; is this a protected tag?)}"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CONF="$ROOT/desktop/src-tauri/tauri.conf.json"
APP_VERSION="$(sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$CONF" | tail -n 1)"
if [ "$APP_VERSION" != "$VERSION" ]; then
  echo "ERROR: tag $TAG does not match tauri.conf.json version $APP_VERSION" >&2
  exit 1
fi

PROJECT_URL="${CI_API_V4_URL:-https://gitlab.com/api/v4}/projects/${CI_PROJECT_ID:-87760189}"
PKG_URL="$PROJECT_URL/packages/generic/zencode-desktop/$TAG"

(cd "$ROOT/desktop/src-tauri" && cargo tauri build --bundles appimage)

BUNDLE="$ROOT/desktop/src-tauri/target/release/bundle/appimage"
SRC="$(find "$BUNDLE" -maxdepth 1 -name '*.AppImage' | head -n 1)"
[ -n "$SRC" ] || { echo "ERROR: no AppImage in $BUNDLE" >&2; exit 1; }
ARCH="$(basename "$SRC" .AppImage | sed 's/.*_//')"
NAME="zencode-desktop_${VERSION}_${ARCH}.AppImage"

mkdir -p "$DIST"
cp "$SRC" "$DIST/$NAME"
(cd "$ROOT/desktop/src-tauri" && cargo tauri signer sign "$DIST/$NAME")
SIG_FILE="$DIST/$NAME.sig"
[ -s "$SIG_FILE" ] || { echo "ERROR: signer wrote no $SIG_FILE" >&2; exit 1; }

# Self-check: the signed trusted comment must name the versioned file.
if ! base64 -d < "$SIG_FILE" | grep -q "^trusted comment: .*file:$NAME\$"; then
  echo "ERROR: signature trusted comment does not name $NAME" >&2
  base64 -d < "$SIG_FILE" | sed -n 's/^trusted comment: /  got: /p' >&2
  exit 1
fi

PUB_DATE="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
SIGNATURE="$(tr -d '\n' < "$SIG_FILE")"
cat > "$DIST/latest.json" <<EOF
{
  "version": "$VERSION",
  "notes": "ZenCode Desktop $TAG — see CHANGELOG.md",
  "pub_date": "$PUB_DATE",
  "platforms": {
    "linux-x86_64": {
      "signature": "$SIGNATURE",
      "url": "$PKG_URL/$NAME"
    }
  }
}
EOF
echo "OK: $DIST/$NAME (+ .sig) and $DIST/latest.json for $TAG"
